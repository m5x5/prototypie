//! Card pictures from proto-web (docs/api-web.md): which cards load (the reference's "in view for
//! 180 ms" rule, decided from the camera, not per-element callbacks), requesting their snapshots in
//! `poll`, and keeping the latest picture per design for the views to draw.
//!
//! Gestures stay cheap (the canvas camera is live-only): targets come from the cards model (per
//! snapshot version and space, never per camera frame); the in-view test runs at most every
//! `CHECK_S` while the camera moves; resolution follows the zoom in power-of-two buckets, only
//! upwards and only after the zoom has held still; `request` is called only when a card's
//! (content key, size) changes or a render finished; and `poll` reports a change only when a new
//! picture arrived.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use proto_core::{PreviewInputs, Snapshot as Snap};
use proto_web::{Snapshot, SnapshotLimits, Snapshots, Thumb, ThumbSpec};
use rune::{Tx, Waker};

use super::model::Model;
use super::pictures::Pictures;
use super::theme::{NODE_H, NODE_W, PORTAL_H, PORTAL_W};
use crate::canvas::camera::Camera;

/// How long a card must be in view before it loads.
const IN_VIEW_S: f64 = 0.18;
/// Margin around the canvas that counts as in view (`rootMargin: 100px`).
const MARGIN: f32 = 100.0;
/// Device pixels per point assumed for `out_w` (Retina). Over-asking on a 1× screen only costs memory.
pub const BACKING: f32 = 2.0;
/// Bounded memo of preview inputs (html + content key) per design.
const INPUTS_CAP: usize = 512;
/// The in-view test runs at most this often while the camera moves (plus once after it stops).
const CHECK_S: f64 = 0.1;
/// A higher resolution is asked for only after the zoom held still this long.
const ZOOM_SETTLE_S: f64 = 0.15;
/// A lower resolution is used only after the zoom rested zoomed out this long.
const LOWER_REST_S: f64 = 1.5;
/// Held pictures shrunk per poll. Each copies its bytes out on the main thread (~1.1 ms,
/// worker-3's stress run), so 3 keep a frame under ~3.5 ms while the user may still pan; unpaced,
/// all copies at once spiked the footprint (227 pictures: 753 MiB).
const REDUCE_PER_POLL: usize = 3;
/// Zoom buckets: powers of two in this range.
const BUCKET_MIN: f32 = 0.125;
const BUCKET_MAX: f32 = 2.0;

/// A card that wants a picture: its design, world rect, the box width it draws at, its layout.
struct Target {
    id: String,
    world: [f32; 4],
    box_w: f32,
    viewport_w: u32,
    viewport_h: Option<u32>,
}

/// Lazy loading per design, as a state machine (AGENTS.md: no bool clusters).
#[derive(Clone, Copy, Debug, PartialEq)]
enum Load {
    /// In view since `since` (poll clock, s); loads at `since + IN_VIEW_S`.
    Waiting { since: f64 },
    /// Loaded for good (it stays loaded when it scrolls out, like the reference).
    Loaded,
}

/// The zoom bucket a zoom asks for: the next power of two at or above it, clamped.
pub fn bucket_for(zoom: f32) -> f32 {
    let z = zoom.clamp(BUCKET_MIN, BUCKET_MAX);
    2f32.powf(z.log2().ceil()).clamp(BUCKET_MIN, BUCKET_MAX)
}

#[derive(Default)]
pub struct Thumbs {
    wake: Option<Waker>,
    snaps: Option<Snapshots>,
    inputs: Option<PreviewInputs>,
    load: HashMap<String, Load>,
    /// The latest picture per design (kept while a newer one renders), bounded (`pictures`).
    ready: Pictures<Snapshot>,
    /// Asked anew, a new render expected: the serial of the picture held when asking (0: none).
    /// Looked at again whenever a render finishes, until a different picture arrives.
    waiting: HashMap<String, u64>,
    /// The active space's picture targets, and those in view at the last check (never evicted).
    active: HashSet<String>,
    visible: HashSet<String>,
    /// Polls so far, and the poll each view outside the canvas last asked for a design in
    /// (`request_extra`: pinned while it keeps asking).
    frame: u64,
    extra_seen: HashMap<String, u64>,
    /// What was last asked per design: (content key, out_w). A request only goes out when it changes.
    asked: HashMap<String, (u64, u32)>,
    /// Designs whose render failed (logged once).
    failed: HashSet<String>,
    /// WebKit was prewarmed (after the first frame); Snapshots come one poll later.
    warmed: bool,
    /// The first picture arrived (launch timing mark).
    first_ready: bool,
    /// Bumped whenever a design's picture changes: part of its card's memo version.
    generation: HashMap<String, u64>,
    targets: Vec<Target>,
    /// (snapshot version, space) the targets are for.
    targets_for: Option<(u64, String)>,
    /// Camera at the last in-view test, and when it ran.
    checked: Option<([f32; 2], f32, [f32; 2], f64)>,
    /// The resolution bucket in use, and the zoom/time a pending raise was first seen at.
    bucket: f32,
    raise: Option<(f32, f64)>,
    /// The lower bucket the zoom rests at, and since when (poll clock).
    lower: Option<(f32, f64)>,
    /// Held card pictures to shrink to the bucket (design id, out_w), a few per poll.
    to_reduce: Vec<(String, u32)>,
}

impl Thumbs {
    /// From `AppState::setup`: keep the waker; Snapshots are created on the first poll (after the
    /// first frame, as proto-web asks).
    pub fn setup(&mut self, wake: Waker) {
        self.wake = Some(wake);
    }

    pub fn get(&self, design_id: &str) -> Option<&Snapshot> {
        self.ready.get(design_id)
    }

    /// Changes whenever `design_id`'s picture changes (memo versions).
    pub fn generation(&self, design_id: &str) -> u64 {
        self.generation.get(design_id).copied().unwrap_or(0)
    }

    /// Changes whenever any picture changes (the whole card layer's memo version).
    pub fn total_generation(&self) -> u64 {
        self.generation.values().sum()
    }


    /// A picture for a view outside the canvas (Review stage, Compare columns, sibling strips):
    /// call from your module's `poll` with `&mut AppState` for each design it shows. Same cache and
    /// latest-wins as the cards (a smaller size reuses a larger picture; a larger one renders while
    /// the old one keeps showing). Cheap to call every poll. True when a new picture arrived for
    /// `design_id`; read it with `get`. `out_w` = device px of your box width.
    pub fn request_extra(&mut self, snap: &Snap, design_id: &str, out_w: u32) -> bool {
        let (Some(snaps), Some(inputs)) = (self.snaps.as_mut(), self.inputs.as_mut()) else { return false };
        let (Some(d), Some(input)) = (snap.design(design_id), inputs.get(snap, design_id)) else { return false };
        let spec = ThumbSpec {
            viewport_w: d.viewport.unwrap_or(1280.0).max(1.0) as u32,
            viewport_h: d.viewport_height.map(|h| h.max(1.0) as u32),
            out_w: out_w.max(64),
        };
        self.extra_seen.insert(design_id.to_string(), self.frame);
        self.ready.touch(design_id);
        match snaps.request(design_id, input.key, &input.html, spec) {
            Thumb::Ready(s) => {
                // A new render only (proto-web never renders smaller than it shows for a rev, so a
                // smaller ask keeps the big picture).
                if self.ready.get(design_id).is_none_or(|old| !same_picture(old, &s)) {
                    self.store(design_id, s);
                    self.evict();
                    return true;
                }
                false
            }
            Thumb::Pending => false,
            Thumb::Failed(_) => false,
        }
    }

    /// Drives loading and rendering. True only when a picture arrived or went away.
    pub fn poll(&mut self, snap: &Snap, model: &Model, cam: &Camera, canvas: [f32; 2], can_start: bool, clock: f64, tx: &mut Tx) -> bool {
        if self.snaps.is_none() {
            // Launch cost stays out of the first frames: nothing until the first frame is up, then
            // warm WebKit (starts its content process), then the renderer one poll later.
            let Some(wake) = self.wake.clone() else { return false };
            if !can_start {
                // Checked again on the next poll (the shell wakes when the server settles).
                tx.wake_after(std::time::Duration::from_millis(100));
                return false;
            }
            if !self.warmed {
                proto_web::prewarm();
                self.warmed = true;
                tx.wake_after(std::time::Duration::from_millis(16));
                return false;
            }
            self.snaps = Some(Snapshots::new(wake, SnapshotLimits::default()));
            let wrap: proto_core::preview::Wrap = Arc::new(|h: &str| proto_web::spec::for_preview(h, false));
            self.inputs = Some(PreviewInputs::new(Some(wrap), INPUTS_CAP));
            self.bucket = bucket_for(cam.zoom);
        }
        let rendered = self.snaps.as_mut().is_some_and(Snapshots::poll);
        self.frame += 1;

        // Targets: per snapshot version and space only.
        let for_key = (model.version, model.space.clone());
        let new_targets = self.targets_for.as_ref() != Some(&for_key);
        if new_targets {
            self.targets = targets(model);
            self.active = self.targets.iter().map(|t| t.id.clone()).collect();
            self.targets_for = Some(for_key);
            self.checked = None;
        }

        // Resolution: raise the bucket once the zoom held still; never lower it.
        let bucket_changed = match raise_bucket(self.bucket, &mut self.raise, cam.zoom, clock) {
            Raise::To(b) => {
                self.bucket = b;
                if log_sizes() {
                    eprintln!("thumbs: bucket {b} at zoom {}", cam.zoom);
                }
                true
            }
            Raise::WakeIn(d) => {
                tx.wake_after(std::time::Duration::from_secs_f64(d));
                false
            }
            Raise::Keep => false,
        };
        // Zoomed out and resting: shrink the held pictures to the lower bucket in place, no WebKit
        // (proto-web `reduce`: box filter of the picture it holds). Zooming in raises as above.
        match lower_bucket(self.bucket, &mut self.lower, cam.zoom, clock) {
            Raise::To(b) => {
                self.bucket = b;
                if log_sizes() {
                    eprintln!("thumbs: lowered to bucket {b} at zoom {}", cam.zoom);
                }
                self.queue_reductions();
            }
            Raise::WakeIn(d) => tx.wake_after(std::time::Duration::from_secs_f64(d)),
            Raise::Keep => {}
        }
        self.run_reductions(tx);

        // In view → waiting → loaded; at most every CHECK_S while the camera moves.
        let cam_key = (cam.pan, cam.zoom, canvas);
        let moved = self.checked.is_none_or(|(p, z, c, _)| (p, z, c) != cam_key);
        let due = self.checked.is_none_or(|(.., at)| clock - at >= CHECK_S);
        let waiting_any = self.load.values().any(|l| matches!(l, Load::Waiting { .. }));
        let mut newly_loaded = false;
        if (moved && due) || waiting_any || new_targets {
            if moved && due {
                self.checked = Some((cam.pan, cam.zoom, canvas, clock));
            }
            newly_loaded = self.check_in_view(cam, canvas, clock, tx);
        } else if moved {
            // The trailing check after the camera stops.
            tx.wake_after(std::time::Duration::from_secs_f64(CHECK_S));
        }

        if !(rendered || newly_loaded || new_targets || bucket_changed) {
            return false;
        }
        let changed = self.request(snap, rendered);
        // Space switches leave other spaces' pictures behind: evict those first.
        let evicted = self.evict();
        changed || evicted
    }

    /// After lowering the bucket: every held card picture of the active space wider than its
    /// card now asks shrinks; the asks follow, so no request (and no WebKit render) goes out
    /// for the smaller size. Open views' pictures (fullscreen, compare, review) stay sharp.
    fn queue_reductions(&mut self) {
        self.to_reduce.clear();
        for t in &self.targets {
            if self.extra_seen.contains_key(&t.id) {
                continue;
            }
            let Some(p) = self.ready.get(&t.id) else { continue };
            let out_w = out_w_for(t.box_w, self.bucket);
            if !wider_than_needed(p.pixels.size().0, out_w) {
                continue;
            }
            if let Some((key, _)) = self.asked.get(&t.id).copied() {
                self.asked.insert(t.id.clone(), (key, out_w));
            }
            self.to_reduce.push((t.id.clone(), out_w));
        }
        // Visible cards shrink first (popped from the end).
        let visible = &self.visible;
        self.to_reduce.sort_by_key(|(id, _)| visible.contains(id));
    }

    /// Shrinks up to `REDUCE_PER_POLL` queued pictures, waking for the rest.
    fn run_reductions(&mut self, tx: &mut Tx) {
        let Some(snaps) = self.snaps.as_mut() else { return };
        for _ in 0..REDUCE_PER_POLL {
            let Some((id, out_w)) = self.to_reduce.pop() else { break };
            // An open view took it meanwhile: keep it sharp.
            if self.extra_seen.contains_key(&id) {
                continue;
            }
            reduce(snaps, &id, out_w);
        }
        if !self.to_reduce.is_empty() {
            tx.wake_after(std::time::Duration::from_millis(16));
        }
    }

    /// Keeps `s` as `id`'s picture.
    fn store(&mut self, id: &str, s: Snapshot) {
        let (w, h) = s.pixels.size();
        self.ready.insert(id, s, w as usize * h as usize * 4);
        *self.generation.entry(id.to_string()).or_default() += 1;
    }

    /// Bounds the held pictures (`pictures`). An evicted card forgets its ask and load state, so
    /// it loads again when it comes into view. True when one went.
    fn evict(&mut self) -> bool {
        let frame = self.frame;
        self.extra_seen.retain(|_, f| *f + 2 >= frame);
        let (active, visible, extra) = (&self.active, &self.visible, &self.extra_seen);
        let gone = self.ready.evict(|id| active.contains(id), |id| visible.contains(id) || extra.contains_key(id));
        for id in &gone {
            *self.generation.entry(id.clone()).or_default() += 1;
            self.asked.remove(id);
            self.load.remove(id);
            self.waiting.remove(id);
        }
        if !gone.is_empty() && log_sizes() {
            let (bytes, n) = self.ready.usage();
            eprintln!("thumbs: evicted {} (now {n} pictures, {} MB)", gone.len(), bytes >> 20);
        }
        !gone.is_empty()
    }

    /// Updates the load states; true when a card became loaded.
    fn check_in_view(&mut self, cam: &Camera, canvas: [f32; 2], clock: f64, tx: &mut Tx) -> bool {
        let view = [-MARGIN, -MARGIN, canvas[0] + MARGIN, canvas[1] + MARGIN];
        let mut newly_loaded = false;
        let mut next_wake: Option<f64> = None;
        self.visible.clear();
        for t in &self.targets {
            let a = cam.screen([t.world[0], t.world[1]]);
            let b = cam.screen([t.world[0] + t.world[2], t.world[1] + t.world[3]]);
            let visible = a[0] < view[2] && b[0] > view[0] && a[1] < view[3] && b[1] > view[1];
            if visible {
                self.visible.insert(t.id.clone());
                self.ready.touch(&t.id);
            }
            match (self.load.get(&t.id).copied(), visible) {
                (Some(Load::Loaded), _) | (None, false) => {}
                (None, true) => {
                    self.load.insert(t.id.clone(), Load::Waiting { since: clock });
                    next_wake = Some(next_wake.map_or(IN_VIEW_S, |w: f64| w.min(IN_VIEW_S)));
                }
                (Some(Load::Waiting { since }), true) if clock - since >= IN_VIEW_S => {
                    self.load.insert(t.id.clone(), Load::Loaded);
                    newly_loaded = true;
                }
                (Some(Load::Waiting { since }), true) => {
                    let left = IN_VIEW_S - (clock - since);
                    next_wake = Some(next_wake.map_or(left, |w| w.min(left)));
                }
                (Some(Load::Waiting { .. }), false) => {
                    self.load.remove(&t.id);
                }
            }
        }
        if let Some(s) = next_wake {
            tx.wake_after(std::time::Duration::from_secs_f64(s.max(0.01)));
        }
        newly_loaded
    }

    /// Asks for loaded cards whose (content key, size) changed, and collects finished renders.
    fn request(&mut self, snap: &Snap, rendered: bool) -> bool {
        let (Some(snaps), Some(inputs)) = (self.snaps.as_mut(), self.inputs.as_mut()) else { return false };
        let mut changed = false;
        for t in &self.targets {
            if self.load.get(&t.id) != Some(&Load::Loaded) {
                continue;
            }
            let Some(input) = inputs.get(snap, &t.id) else { continue };
            let out_w = out_w_for(t.box_w, self.bucket);
            let ask = (input.key, out_w);
            let new_ask = self.asked.get(&t.id) != Some(&ask);
            // proto-web keeps returning the previous picture (`Ready`) while a sharper size or
            // newer rev renders, so a card asked anew waits for a different picture, looked at
            // again whenever a render finishes. Only those: re-asking every card could re-render
            // what proto-web's own LRU evicted, round and round.
            if !new_ask && !(rendered && self.waiting.contains_key(&t.id)) {
                continue;
            }
            if new_ask {
                self.asked.insert(t.id.clone(), ask);
                self.waiting.insert(t.id.clone(), self.ready.get(&t.id).map_or(0, |p| p.serial));
                if log_sizes() {
                    eprintln!("thumbs: ask {} out_w {out_w} (bucket {})", t.id, self.bucket);
                }
            }
            let spec = ThumbSpec { viewport_w: t.viewport_w, viewport_h: t.viewport_h, out_w };
            match snaps.request(&t.id, input.key, &input.html, spec) {
                Thumb::Ready(s) => {
                    if self.ready.get(&t.id).is_some_and(|old| same_picture(old, &s)) {
                        // Still the held picture: the new render hasn't landed (or the cache
                        // served the ask with it, then it stays harmlessly listed until it does).
                        continue;
                    }
                    self.waiting.remove(&t.id);
                    if log_sizes() {
                        eprintln!("thumbs: got {} {}x{}", t.id, s.pixels.size().0, s.pixels.size().1);
                    }
                    if !self.first_ready {
                        self.first_ready = true;
                        crate::perf::mark("first-thumb");
                    }
                    self.failed.remove(&t.id);
                    let id = t.id.clone();
                    let (w, h) = s.pixels.size();
                    // Served from a bigger cached picture (zoomed out): shrink that too.
                    if wider_than_needed(w, out_w) && !self.extra_seen.contains_key(&id) {
                        self.to_reduce.push((id.clone(), out_w));
                    }
                    self.ready.insert(&id, s, w as usize * h as usize * 4);
                    *self.generation.entry(id).or_default() += 1;
                    changed = true;
                }
                Thumb::Pending => {}
                Thumb::Failed(why) => {
                    self.waiting.remove(&t.id);
                    if self.ready.remove(&t.id).is_some() {
                        *self.generation.entry(t.id.clone()).or_default() += 1;
                        changed = true;
                    }
                    if self.failed.insert(t.id.clone()) {
                        eprintln!("cards: thumbnail for {} failed: {why}", t.id);
                    }
                }
            }
        }
        changed
    }
}

/// Whether two snapshots are the same picture: proto-web gives each finished render its own
/// `serial` (a sharper size or a newer rev is a new render).
fn same_picture(a: &Snapshot, b: &Snapshot) -> bool {
    a.serial == b.serial
}

/// `PROTO_THUMB_LOG=1`: log bucket raises and requested sizes (scripted resolution checks).
fn log_sizes() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("PROTO_THUMB_LOG").is_some())
}

/// What the zoom asks of the resolution bucket.
#[derive(Debug, PartialEq)]
enum Raise {
    Keep,
    /// Not held still long enough yet: look again in this many seconds.
    WakeIn(f64),
    To(f32),
}

/// Raises `bucket` to the zoom's once the zoom held still `ZOOM_SETTLE_S` (`raise` remembers the
/// zoom/time a pending raise was first seen at); never lowers it.
fn raise_bucket(bucket: f32, raise: &mut Option<(f32, f64)>, zoom: f32, clock: f64) -> Raise {
    let want = bucket_for(zoom);
    if want <= bucket {
        *raise = None;
        return Raise::Keep;
    }
    match *raise {
        Some((z, since)) if z == zoom && clock - since >= ZOOM_SETTLE_S => {
            *raise = None;
            Raise::To(want)
        }
        Some((z, since)) if z == zoom => Raise::WakeIn((ZOOM_SETTLE_S - (clock - since)).max(0.01)),
        _ => {
            *raise = Some((zoom, clock));
            Raise::WakeIn(ZOOM_SETTLE_S)
        }
    }
}

/// Lowers `bucket` to the zoom's once it rested at a lower bucket for `LOWER_REST_S` (`lower`
/// remembers that bucket and since when). A zoom back up (or within the current bucket) cancels.
fn lower_bucket(bucket: f32, lower: &mut Option<(f32, f64)>, zoom: f32, clock: f64) -> Raise {
    let want = bucket_for(zoom);
    if want >= bucket {
        *lower = None;
        return Raise::Keep;
    }
    match *lower {
        Some((b, since)) if b == want && clock - since >= LOWER_REST_S => {
            *lower = None;
            Raise::To(want)
        }
        Some((b, since)) if b == want => Raise::WakeIn((LOWER_REST_S - (clock - since)).max(0.01)),
        _ => {
            *lower = Some((want, clock));
            Raise::WakeIn(LOWER_REST_S)
        }
    }
}

/// Whether a picture `w` px wide is worth shrinking for an `out_w` ask (proto-web's reuse slack).
fn wider_than_needed(w: u32, out_w: u32) -> bool {
    w as f64 > proto_web::spec::bucket(out_w) as f64 * proto_web::spec::REUSE_UP_TO
}

/// proto-web `Snapshots::reduce`: box-filters the picture it holds to `out_w` in place (no
/// WebKit); every holder, `ready` included, sees the smaller picture.
fn reduce(snaps: &mut Snapshots, id: &str, out_w: u32) -> bool {
    let started = snaps.reduce(id, out_w);
    if started && log_sizes() {
        eprintln!("thumbs: reduce {id} to {out_w}");
    }
    started
}

/// Device px a card box `box_w` pt wide asks for at `bucket`.
fn out_w_for(box_w: f32, bucket: f32) -> u32 {
    (box_w * bucket * BACKING).round().max(64.0) as u32
}

/// The cards in the model that show a design picture: ready design cards, and each portal's hero.
fn targets(model: &Model) -> Vec<Target> {
    let mut out = Vec::new();
    for e in model.designs.iter().filter(|e| e.design.is_ready()) {
        let d = &e.design;
        out.push(Target {
            id: d.id.clone(),
            world: [d.position.x as f32, d.position.y as f32, NODE_W, NODE_H],
            box_w: NODE_W - 2.0,
            viewport_w: d.viewport.unwrap_or(1280.0).max(1.0) as u32,
            viewport_h: d.viewport_height.map(|h| h.max(1.0) as u32),
        });
    }
    for p in &model.portals {
        let Some(hero) = &p.facts.hero else { continue };
        out.push(Target {
            id: hero.clone(),
            world: [p.pos[0], p.pos[1], PORTAL_W, PORTAL_H],
            box_w: PORTAL_W - 22.0,
            viewport_w: p.hero_vw.max(1.0) as u32,
            viewport_h: None,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// User feedback: low-res while zoomed out is fine, but zooming in must sharpen. Far out, then
    /// in to 200 % and held: the card asks for its on-screen device px (box × zoom × backing).
    #[test]
    fn zooming_in_and_holding_raises_to_on_screen_device_px() {
        let mut raise = None;
        let mut bucket = bucket_for(0.1);
        assert_eq!(bucket, 0.125);
        assert_eq!(raise_bucket(bucket, &mut raise, 0.1, 0.0), Raise::Keep);
        // The pinch: zoom keeps changing, nothing raises yet.
        for (i, z) in [0.3f32, 0.7, 1.2, 1.7, 2.0].into_iter().enumerate() {
            assert!(matches!(raise_bucket(bucket, &mut raise, z, 0.02 * i as f64), Raise::WakeIn(_)));
        }
        // Held at 2.0 for the settle time.
        let r = raise_bucket(bucket, &mut raise, 2.0, 0.08 + ZOOM_SETTLE_S + 0.001);
        assert_eq!(r, Raise::To(2.0));
        bucket = 2.0;
        let box_w = NODE_W - 2.0;
        let on_screen_px = box_w * 2.0 * BACKING;
        assert!(out_w_for(box_w, bucket) as f32 >= on_screen_px);
        // proto-web renders at a 256 bucket ≥ that, capped at 2 px per CSS px (2560 for 1280).
        let rendered = proto_web::spec::bucket(out_w_for(box_w, bucket));
        let plan_w = (rendered as f64).min(1280.0 * proto_web::spec::MAX_DENSITY);
        assert!(plan_w as f32 >= on_screen_px);
        // Zooming back out keeps the sharp picture.
        assert_eq!(raise_bucket(bucket, &mut raise, 0.2, 1.0), Raise::Keep);
    }

    /// Zoomed out and resting 1.5 s lowers the bucket; a zoom-in during the rest cancels; zooming
    /// within the current bucket never lowers.
    #[test]
    fn resting_zoomed_out_lowers_the_bucket() {
        let mut lower = None;
        assert_eq!(lower_bucket(2.0, &mut lower, 1.5, 0.0), Raise::Keep);
        assert!(matches!(lower_bucket(2.0, &mut lower, 0.2, 0.0), Raise::WakeIn(_)));
        // Pan/zoom within the same lower bucket keeps the rest going.
        assert!(matches!(lower_bucket(2.0, &mut lower, 0.22, 1.0), Raise::WakeIn(_)));
        assert_eq!(lower_bucket(2.0, &mut lower, 0.22, 1.51), Raise::To(0.25));
        // A zoom back in before the rest ends cancels it.
        let mut lower = None;
        lower_bucket(2.0, &mut lower, 0.2, 0.0);
        assert_eq!(lower_bucket(2.0, &mut lower, 1.8, 0.5), Raise::Keep);
        assert!(matches!(lower_bucket(2.0, &mut lower, 0.2, 1.6), Raise::WakeIn(_)));
    }

    #[test]
    fn only_clearly_wider_pictures_shrink() {
        let need = out_w_for(NODE_W - 2.0, 0.25); // 189 → bucket 256, slack to 320
        assert!(!wider_than_needed(320, need));
        assert!(wider_than_needed(1512, need));
    }

    /// Every rest zoom gets at least its on-screen device px.
    #[test]
    fn each_resting_zoom_is_covered() {
        for z in [0.13f32, 0.25, 0.4, 0.5, 0.75, 1.0, 1.3, 1.5, 2.0] {
            let box_w = NODE_W - 2.0;
            assert!(out_w_for(box_w, bucket_for(z)) as f32 >= (box_w * z * BACKING).floor(), "zoom {z}");
        }
    }

    #[test]
    fn zoom_buckets_are_powers_of_two_at_or_above_the_zoom() {
        assert_eq!(bucket_for(1.0), 1.0);
        assert_eq!(bucket_for(1.01), 2.0);
        assert_eq!(bucket_for(0.6), 1.0);
        assert_eq!(bucket_for(0.4), 0.5);
        assert_eq!(bucket_for(0.01), 0.125);
        assert_eq!(bucket_for(8.0), 2.0);
    }
}
