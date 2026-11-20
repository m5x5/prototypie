//! The fullscreen preview's live WKWebView (proto-web `LiveView`, docs/api-web.md, spec-cards §7.5).
//!
//! The web view is a native view drawn above all Rune content, the morph flight included, so it
//! is prepared (loaded hidden) when the view opens, shown only once the preview box has stopped
//! moving (the flight landed), moved on resize, and hidden in the same turn the view closes.

use proto_core::{PreviewInputs, Snapshot};
use proto_web::{LiveKey, LiveOptions, LivePage, LiveView};
use rune::{Tx, Waker};

/// How long the box must hold still before the web view goes over it.
const SETTLE_S: f64 = 0.06;

/// Where the web view is.
#[derive(Clone, Debug, PartialEq)]
enum Phase {
    Hidden,
    /// Loading `id` hidden; the box was last seen at `rect` since `since` (poll clock, s).
    Preparing { id: String, rect: [f32; 4], since: f64 },
    /// Over the box at `rect`, showing `id` at content key `rev`.
    Shown { id: String, rev: u64, rect: [f32; 4] },
}

pub struct LiveCtl {
    wake: Option<Waker>,
    view: Option<LiveView>,
    /// Attaching failed (headless, no window): don't retry every poll.
    unavailable: bool,
    inputs: PreviewInputs,
    /// The preview box's drawn rect in window points (fullscreen view binds it).
    pub rect: rune::Live<[f32; 4]>,
    phase: Phase,
    /// Snapshot version last looked at (a changed design may need a reload).
    seen_version: u64,
}

impl Default for LiveCtl {
    fn default() -> Self {
        LiveCtl {
            wake: None,
            view: None,
            unavailable: false,
            inputs: PreviewInputs::new(None, 32),
            rect: rune::Live::new([0.0; 4]),
            phase: Phase::Hidden,
            seen_version: 0,
        }
    }
}

/// What the keys queued by a focused web view ask for.
pub enum KeyAsk {
    Close,
    Step(i32),
}

impl LiveCtl {
    pub fn setup(&mut self, wake: Waker) {
        self.wake = Some(wake);
    }

    /// Hide now (the view is closing): synchronous, so it's gone before the flight back starts.
    pub fn hide(&mut self) {
        if let Some(v) = &self.view {
            v.hide();
        }
        self.phase = Phase::Hidden;
    }

    fn ensure_attached(&mut self) -> Option<&LiveView> {
        if self.view.is_none() && !self.unavailable {
            let (Some(win), Some(wake)) = (crate::shell::window(), self.wake.clone()) else {
                self.unavailable = crate::shell::window().is_none() && self.wake.is_some();
                return None;
            };
            match LiveView::attach(&win, wake) {
                Ok(v) => self.view = Some(v),
                Err(e) => {
                    eprintln!("fullscreen: live view unavailable: {e}");
                    self.unavailable = true;
                }
            }
        }
        self.view.as_ref()
    }

    /// Drives the web view for the open design `open` (None = closed). Returns the keys the web
    /// view consumed, translated, for the caller to apply.
    pub fn poll(&mut self, snap: &Snapshot, open: Option<&str>, clock: f64, tx: &mut Tx) -> Vec<KeyAsk> {
        let Some(id) = open else {
            if self.phase != Phase::Hidden {
                self.hide();
            }
            return Vec::new();
        };
        // Shown and nothing moved or changed: only drain keys (no per-frame AppKit or html work).
        if let Phase::Shown { id: shown, rect: at, .. } = &self.phase {
            if shown == id && *at == self.rect.get() && self.seen_version == snap.version {
                return self.take_keys();
            }
        }
        self.seen_version = snap.version;
        let (Some(d), Some(input)) = (snap.design(id), self.inputs.get(snap, id)) else { return Vec::new() };
        let page = LivePage {
            key: id,
            rev: input.key,
            html: &input.html,
            viewport_w: d.viewport.unwrap_or(1280.0).max(1.0) as u32,
            viewport_h: d.viewport_height.map(|h| h.max(1.0) as u32),
        };
        let opts = LiveOptions::default();
        let rect = self.rect.get();
        let phase = self.phase.clone();
        let Some(view) = self.ensure_attached() else { return Vec::new() };

        let next = match phase {
            Phase::Shown { id: shown, rev, rect: at } if shown == id => {
                if page.rev != rev {
                    // An AI edit landed: reload in place (show loads only on a key/rev change).
                    view.show(rect, &page, &opts);
                } else if at != rect && rect[2] > 0.0 {
                    view.move_to(rect);
                }
                Phase::Shown { id: shown, rev: page.rev, rect }
            }
            Phase::Shown { .. } => {
                // Stepped to another design: swap the page in place, no flight.
                view.show(rect, &page, &opts);
                Phase::Shown { id: id.to_string(), rev: page.rev, rect }
            }
            Phase::Preparing { id: p, rect: seen, since } if p == id => {
                if rect[2] > 0.0 && rect == seen && clock - since >= SETTLE_S {
                    view.show(rect, &page, &opts);
                    Phase::Shown { id: p, rev: page.rev, rect }
                } else {
                    tx.wake_after(std::time::Duration::from_secs_f64(SETTLE_S));
                    let since = if rect == seen { since } else { clock };
                    Phase::Preparing { id: p, rect, since }
                }
            }
            Phase::Preparing { .. } | Phase::Hidden => {
                view.prepare(&page, &opts);
                tx.wake_after(std::time::Duration::from_secs_f64(SETTLE_S));
                Phase::Preparing { id: id.to_string(), rect, since: clock }
            }
        };
        self.phase = next;
        self.take_keys()
    }

    /// The web view while it is over the box: (design id, view, rect in window points). None
    /// while hidden or still preparing.
    pub fn shown(&self) -> Option<(&str, &LiveView, [f32; 4])> {
        match (&self.phase, &self.view) {
            (Phase::Shown { id, rect, .. }, Some(v)) => Some((id.as_str(), v, *rect)),
            _ => None,
        }
    }

    fn take_keys(&self) -> Vec<KeyAsk> {
        let Some(view) = &self.view else { return Vec::new() };
        view.take_keys()
            .into_iter()
            .map(|k| match k {
                LiveKey::Escape => KeyAsk::Close,
                LiveKey::Left => KeyAsk::Step(-1),
                LiveKey::Right => KeyAsk::Step(1),
            })
            .collect()
    }
}
