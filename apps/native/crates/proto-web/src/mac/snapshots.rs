//! `Snapshots`: the thumbnail cache in front of the renderer (docs/api-web.md).
//!
//! UI thread: lookups, the entry state machine and the LRU. Renderer (main run loop): WebKit.
//! Worker thread: CGImage → RGBA. Finished work comes back through a channel; `wake` tells the
//! app to call `poll`, which turns results into `rune::Pixels`.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};

use objc2::MainThreadMarker;

use super::image;
use super::js;
use super::render::{Job, RawSnap, Renderer};
use crate::spec::{self, ThumbSpec};

pub type Wake = Arc<dyn Fn() + Send + Sync>;

#[derive(Clone, Copy, Debug)]
pub struct SnapshotLimits {
    pub max_entries: usize,
    /// Decoded RGBA bytes across all entries.
    pub max_bytes: usize,
    /// Web views rendering in parallel.
    pub web_views: usize,
}

impl Default for SnapshotLimits {
    fn default() -> Self {
        SnapshotLimits { max_entries: 400, max_bytes: 384 << 20, web_views: 3 }
    }
}

/// A finished thumbnail.
#[derive(Clone, Debug)]
pub struct Snapshot {
    /// Straight-alpha sRGB, already cropped to `rect`.
    pub pixels: rune::Pixels,
    /// Captured area in viewport CSS px: x, y, w, h.
    pub rect: [f32; 4],
    /// Some when the content crop applied (then equal to `rect`).
    pub crop: Option<[f32; 4]>,
    pub doc_h: f32,
    /// The `rev` passed to `request` that this picture was rendered for.
    pub rev: u64,
    /// Unique per finished render (also changes when only the size re-rendered): compare it to
    /// tell a new picture from the one already shown.
    pub serial: u64,
}

#[derive(Clone, Debug)]
pub enum Thumb {
    Pending,
    Ready(Snapshot),
    Failed(String),
}

/// The layout-relevant part of a spec: a change re-renders regardless of size.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Shape {
    viewport_w: u32,
    viewport_h: Option<u32>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Want {
    rev: u64,
    shape: Shape,
    bucket: u32,
}

enum State {
    /// Nothing in flight; `failed` holds the rev that last failed and why (no retry at that rev).
    Idle { failed: Option<(u64, String)> },
    Rendering { ticket: u64, want: Want },
}

impl Want {
    /// Whether a picture rendered for `self` serves a request for `want` at `out_w` px.
    fn serves(&self, want: &Want, out_w: u32) -> bool {
        self.rev == want.rev && self.shape == want.shape && spec::serves(self.bucket, out_w)
    }
}

/// Whether a request must start a render, given what is shown and what is in flight.
fn must_render(have: Option<&Want>, state: &State, want: &Want, out_w: u32) -> bool {
    match state {
        State::Rendering { want: w, .. } => !w.serves(want, out_w),
        State::Idle { failed: Some((rev, _)) } if *rev == want.rev => false,
        State::Idle { .. } => !have.is_some_and(|h| h.serves(want, out_w)),
    }
}

/// Whether `reduce(out_w)` shrinks the shown picture (`shown_w` px wide), and to what width:
/// only an idle entry whose picture is wider than a request at `out_w` would render
/// (`bucket(out_w)`, served up to `REUSE_UP_TO`). Latest wins: a render in flight beats a reduce.
fn must_reduce(have: Option<&Want>, state: &State, shown_w: u32, out_w: u32) -> Option<u32> {
    let target = spec::bucket(out_w);
    match (have, state) {
        (Some(_), State::Idle { .. }) if shown_w as f64 > target as f64 * spec::REUSE_UP_TO => Some(target),
        _ => None,
    }
}

struct Entry {
    /// What `shown` was rendered for (its bucket lowered by a reduce).
    have: Option<Want>,
    shown: Option<Snapshot>,
    bytes: usize,
    state: State,
    last_used: u64,
    /// The reduce in flight, if any (a newer reduce or a render supersedes it).
    reducing: Option<u64>,
}

/// Work for the proto-web worker thread.
enum Work {
    /// A finished WebKit render: CGImage → RGBA.
    Convert(String, u64, RawSnap),
    /// Shrink a shown picture's RGBA to `to_w` wide.
    Reduce { id: String, ticket: u64, rgba: Vec<u8>, w: u32, h: u32, to_w: u32 },
}

struct Converted {
    id: String,
    ticket: u64,
    result: Done,
}

enum Done {
    Render(Result<(spec::Plan, spec::Measured, Vec<u8>), String>),
    Reduced { rgba: Vec<u8>, w: u32, h: u32 },
}

pub struct Snapshots {
    /// Made on the first render (`Snapshots::new` stays cheap at launch).
    renderer: Option<Renderer>,
    mtm: MainThreadMarker,
    entries: HashMap<String, Entry>,
    to_worker: Sender<Work>,
    done_tx: Sender<Converted>,
    done_rx: Receiver<Converted>,
    wake: Wake,
    limits: SnapshotLimits,
    clock: u64,
    tickets: u64,
    bytes: usize,
    base_url: Option<String>,
}

impl Snapshots {
    /// Create once, on the UI (main) thread, after the first frame: the first render starts
    /// WebKit's processes. `wake` runs on a worker thread when a result waits for `poll`.
    pub fn new(wake: Wake, limits: SnapshotLimits) -> Snapshots {
        let mtm = MainThreadMarker::new().expect("Snapshots must be created on the main thread");
        let (to_worker, work_rx) = channel::<Work>();
        let (done_tx, done_rx) = channel::<Converted>();
        let tx = done_tx.clone();
        let w = wake.clone();
        std::thread::Builder::new()
            .name("proto-web-rgba".into())
            .spawn(move || {
                for work in work_rx {
                    let done = match work {
                        Work::Convert(id, ticket, raw) => {
                            let rgba = image::to_rgba(&raw.image, raw.plan.out_w, raw.plan.out_h).ok_or_else(|| "pixel conversion failed".to_string());
                            Converted { id, ticket, result: Done::Render(rgba.map(|px| (raw.plan, raw.measured, px))) }
                        }
                        Work::Reduce { id, ticket, rgba, w, h, to_w } => {
                            let (rgba, nh) = crate::resample::downscale(&rgba, w, h, to_w);
                            Converted { id, ticket, result: Done::Reduced { rgba, w: to_w.min(w), h: nh } }
                        }
                    };
                    if tx.send(done).is_err() {
                        return;
                    }
                    w();
                }
            })
            .expect("spawn proto-web worker");
        Snapshots {
            renderer: None,
            mtm,
            entries: HashMap::new(),
            to_worker,
            done_tx,
            done_rx,
            wake,
            limits,
            clock: 0,
            tickets: 0,
            bytes: 0,
            base_url: Some(spec::default_asset_base()),
        }
    }

    /// Base URL for designs' relative URLs (default: `spec::default_asset_base()`). Applies to
    /// renders started afterwards.
    pub fn set_base_url(&mut self, base: Option<String>) {
        self.base_url = base;
    }

    /// The thumbnail for `design_id` at `rev`; starts a render when nothing usable is cached.
    /// Latest wins: while a newer rev or larger size renders, the previous picture is returned.
    pub fn request(&mut self, design_id: &str, rev: u64, html: &str, spec: ThumbSpec) -> Thumb {
        self.clock += 1;
        let want = Want { rev, shape: Shape { viewport_w: spec.viewport_w, viewport_h: spec.viewport_h }, bucket: spec::bucket(spec.out_w) };
        let entry = self.entries.entry(design_id.to_string()).or_insert(Entry { have: None, shown: None, bytes: 0, state: State::Idle { failed: None }, last_used: 0, reducing: None });
        entry.last_used = self.clock;

        if must_render(entry.have.as_ref(), &entry.state, &want, spec.out_w) {
            self.tickets += 1;
            let ticket = self.tickets;
            // Never render smaller than what is shown for the same page.
            let bucket = match entry.have {
                Some(h) if h.rev == want.rev && h.shape == want.shape => h.bucket.max(want.bucket),
                _ => want.bucket,
            };
            let want = Want { bucket, ..want };
            entry.state = State::Rendering { ticket, want };
            entry.reducing = None;
            self.start(design_id, ticket, html, ThumbSpec { out_w: bucket, ..spec });
        }

        let entry = &self.entries[design_id];
        match (&entry.shown, &entry.state) {
            (Some(s), _) => Thumb::Ready(s.clone()),
            (None, State::Idle { failed: Some((_, why)) }) => Thumb::Failed(why.clone()),
            (None, _) => Thumb::Pending,
        }
    }

    fn start(&mut self, design_id: &str, ticket: u64, html: &str, spec: ThumbSpec) {
        let renderer = self.renderer.get_or_insert_with(|| Renderer::new(self.mtm, self.limits.web_views)).clone();
        // Latest wins: drop a queued older job for this design, stop a running one.
        renderer.retain_queued(|j| j.tag != design_id);
        renderer.cancel_running(design_id);
        let id = design_id.to_string();
        let to_worker = self.to_worker.clone();
        let done_tx = self.done_tx.clone();
        let wake = self.wake.clone();
        renderer.submit(Job {
            tag: design_id.to_string(),
            html: spec::for_preview(html, spec.phone()),
            base_url: self.base_url.clone(),
            viewport_w: spec.viewport_w,
            layout_h: spec.layout_h(),
            measure_js: js::measure(spec.viewport_w, spec.crops(), js::SETTLE_MS),
            plan: Box::new(move |m| spec::plan(&spec, m)),
            done: Box::new(move |result| match result {
                Ok(raw) => {
                    let _ = to_worker.send(Work::Convert(id, ticket, raw));
                }
                Err(why) => {
                    let _ = done_tx.send(Converted { id, ticket, result: Done::Render(Err(why)) });
                    wake();
                }
            }),
        });
    }

    /// Shrinks a design's shown picture to what a request at `out_w` would render, without
    /// WebKit: the pixels are area-filtered on the worker and replaced in place on a later `poll`
    /// (every holder of the `Pixels` sees it; `serial` stays, the size changes). A later larger
    /// request re-renders through WebKit. False (nothing started) unless the entry is idle with
    /// a picture wider than `bucket(out_w) × REUSE_UP_TO`. Latest wins: a newer reduce or a
    /// render supersedes one in flight.
    pub fn reduce(&mut self, design_id: &str, out_w: u32) -> bool {
        let Some(entry) = self.entries.get_mut(design_id) else { return false };
        let Some(shown) = &entry.shown else { return false };
        let (w, h) = shown.pixels.size();
        let Some(to_w) = must_reduce(entry.have.as_ref(), &entry.state, w, out_w) else { return false };
        // Copy out on this thread (a memcpy under the frame lock); filter on the worker.
        let rgba = shown.pixels.with_rgba(|px, _, _| px.to_vec());
        if rgba.len() != (w * h * 4) as usize {
            return false;
        }
        self.tickets += 1;
        let ticket = self.tickets;
        entry.reducing = Some(ticket);
        let _ = self.to_worker.send(Work::Reduce { id: design_id.to_string(), ticket, rgba, w, h, to_w });
        true
    }

    /// Applies finished renders and reduces. Call from `AppSpec::poll`; true when some `Thumb`
    /// changed.
    pub fn poll(&mut self) -> bool {
        let mut changed = false;
        while let Ok(done) = self.done_rx.try_recv() {
            let Some(entry) = self.entries.get_mut(&done.id) else { continue };
            let result = match done.result {
                Done::Reduced { rgba, w, h } => {
                    // Only onto the picture it was made from, while nothing newer started.
                    if entry.reducing != Some(done.ticket) || !matches!(entry.state, State::Idle { .. }) {
                        continue;
                    }
                    entry.reducing = None;
                    let (Some(shown), Some(have)) = (&entry.shown, entry.have.as_mut()) else { continue };
                    let bytes = rgba.len();
                    shown.pixels.set(w, h, rgba);
                    have.bucket = spec::bucket(w);
                    self.bytes = self.bytes - entry.bytes + bytes;
                    entry.bytes = bytes;
                    changed = true;
                    continue;
                }
                Done::Render(r) => r,
            };
            let State::Rendering { ticket, want } = entry.state else { continue };
            if ticket != done.ticket {
                continue;
            }
            match result {
                Ok((plan, measured, rgba)) => {
                    let pixels = rune::Pixels::new();
                    let bytes = rgba.len();
                    pixels.set(plan.out_w, plan.out_h, rgba);
                    let f = |r: [f64; 4]| r.map(|v| v as f32);
                    entry.shown = Some(Snapshot { pixels, rect: f(plan.rect), crop: plan.crop.map(f), doc_h: measured.doc_h as f32, rev: want.rev, serial: done.ticket });
                    self.bytes = self.bytes - entry.bytes + bytes;
                    entry.bytes = bytes;
                    entry.have = Some(want);
                    entry.state = State::Idle { failed: None };
                }
                Err(why) if why == "replaced" => continue,
                Err(why) => entry.state = State::Idle { failed: Some((want.rev, why)) },
            }
            changed = true;
        }
        if changed {
            self.evict();
        }
        changed
    }

    /// Forgets a design (deleted).
    pub fn invalidate(&mut self, design_id: &str) {
        if let Some(e) = self.entries.remove(design_id) {
            self.bytes -= e.bytes;
        }
        if let Some(r) = &self.renderer {
            r.retain_queued(|j| j.tag != design_id);
            r.cancel_running(design_id);
        }
    }

    pub fn clear(&mut self) {
        let ids: Vec<String> = self.entries.keys().cloned().collect();
        for id in ids {
            self.invalidate(&id);
        }
    }

    /// Decoded bytes and entries held, for diagnostics.
    pub fn usage(&self) -> (usize, usize) {
        (self.bytes, self.entries.len())
    }

    /// Web views alive and render jobs waiting for a slot, for diagnostics.
    pub fn pool(&self) -> (usize, usize) {
        self.renderer.as_ref().map_or((0, 0), |r| (r.views(), r.queued()))
    }

    fn evict(&mut self) {
        while self.entries.len() > self.limits.max_entries || self.bytes > self.limits.max_bytes {
            let oldest = self
                .entries
                .iter()
                .filter(|(_, e)| !matches!(e.state, State::Rendering { .. }))
                .min_by_key(|(_, e)| e.last_used)
                .map(|(k, _)| k.clone());
            let Some(id) = oldest else { break };
            self.invalidate(&id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn want(rev: u64, bucket: u32) -> Want {
        Want { rev, shape: Shape { viewport_w: 1280, viewport_h: None }, bucket }
    }

    #[test]
    fn renders_once_then_serves_from_cache() {
        let idle = State::Idle { failed: None };
        assert!(must_render(None, &idle, &want(1, 768), 760));
        assert!(!must_render(Some(&want(1, 768)), &idle, &want(1, 768), 760));
        // Up to 1.25x larger is still served; beyond re-renders.
        assert!(!must_render(Some(&want(1, 768)), &idle, &want(1, 1024), 900));
        assert!(must_render(Some(&want(1, 768)), &idle, &want(1, 1024), 1000));
        // A new rev re-renders.
        assert!(must_render(Some(&want(1, 768)), &idle, &want(2, 768), 760));
    }

    #[test]
    fn in_flight_and_failed_dont_restart() {
        let rendering = State::Rendering { ticket: 1, want: want(2, 768) };
        assert!(!must_render(Some(&want(1, 768)), &rendering, &want(2, 768), 760));
        assert!(must_render(Some(&want(1, 768)), &rendering, &want(3, 768), 760));
        let failed = State::Idle { failed: Some((2, "x".into())) };
        assert!(!must_render(None, &failed, &want(2, 768), 760));
        assert!(must_render(None, &failed, &want(3, 768), 760));
    }

    #[test]
    fn reduce_shrinks_only_idle_pictures_too_wide_for_the_ask() {
        let idle = State::Idle { failed: None };
        // A 768 px picture, zoomed out to a 378 px ask: down to bucket(378) = 512.
        assert_eq!(must_reduce(Some(&want(1, 768)), &idle, 768, 378), Some(512));
        // 512 × 1.25 = 640 still serves a 600 px picture's ask: nothing to gain.
        assert_eq!(must_reduce(Some(&want(1, 512)), &idle, 600, 378), None);
        // Nothing shown yet, or a render in flight (latest wins): no reduce.
        assert_eq!(must_reduce(None, &idle, 768, 200), None);
        let rendering = State::Rendering { ticket: 1, want: want(2, 768) };
        assert_eq!(must_reduce(Some(&want(1, 768)), &rendering, 768, 200), None);
    }
}
