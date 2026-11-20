//! The canvas camera: `screen = world * zoom + pan`, in canvas points (the world element draws
//! with `origin(0, 0)`, so its live scale applies about its top-left). The camera lives in Live
//! values, so pans and pinches move the world and the dot grid without re-running the view.

use std::cell::Cell;
use std::rc::Rc;

use rune::{GesturePhase, Live, MagnifyEvent};

use super::inertia::{Coast, Tracker};

pub const MIN_ZOOM: f32 = 0.05;
pub const MAX_ZOOM: f32 = 2.0;
/// World spacing of the dot grid (React Flow's `gap={24}`).
pub const DOT_GAP: f32 = 24.0;
/// The dot grid's on-screen spacing never drops below `DOT_MIN`: when zooming out past it the grid
/// shows every 2nd, 4th… world dot, so the count of drawn dots stays bounded.
pub const DOT_MIN: f32 = 16.0;

/// Called by `Camera::set` with each applied pan and zoom (the minimap's same-frame sync).
pub type Follower = Rc<dyn Fn([f32; 2], f32)>;

pub struct Camera {
    pub pan: [f32; 2],
    pub zoom: f32,
    pub live_pan: Live<[f32; 2]>,
    pub live_zoom: Live<f32>,
    /// The dot layer's own transform: a lattice phase-shifted by `pan` and scaled by the zoom's
    /// level of detail (see `dot_transform`).
    pub live_dots_offset: Live<[f32; 2]>,
    pub live_dots_scale: Live<f32>,
    /// The dots' opacity: React Flow's dots are `1 × zoom` wide, so they shrink below a pixel and
    /// fade when zoomed out; ours keep a minimum drawn size, so they fade by the area they'd lose.
    pub live_dots_opacity: Live<f32>,
    /// Called with the new pan and zoom after every applied `set`: lets derived Live values (the
    /// minimap's viewport) follow in the same frame. Hot path: Live sets only, no allocation.
    follower: Option<Follower>,
    /// The current pinch's recent steps, and the zoom coasting after one (`inertia`).
    pinch_track: Tracker,
    coast: Option<Coast>,
    /// The current scroll gesture zooms (⌘/Ctrl was down when it began): its momentum keeps
    /// zooming even after ⌘ is let go, and a plain scroll's momentum keeps panning.
    scroll_zooms: bool,
    /// Reduce Motion is on (the canvas view records it): pinches don't coast.
    pub reduced_motion: Cell<bool>,
}

impl Default for Camera {
    fn default() -> Self {
        let (offset, scale) = dot_transform([0.0, 0.0], 1.0);
        Camera {
            pan: [0.0, 0.0],
            zoom: 1.0,
            live_pan: Live::new([0.0, 0.0]),
            live_zoom: Live::new(1.0),
            live_dots_offset: Live::new(offset),
            live_dots_scale: Live::new(scale),
            live_dots_opacity: Live::new(1.0),
            follower: None,
            pinch_track: Tracker::default(),
            coast: None,
            scroll_zooms: false,
            reduced_motion: Cell::new(false),
        }
    }
}

/// The dot layer's offset and scale for a camera. The layer is a lattice with local spacing
/// `DOT_GAP` starting one cell above-left of the canvas; drawn at `scale`, its cells are
/// `DOT_GAP * zoom * 2^n` apart on screen (at least `DOT_MIN`, n ≥ 0), and its offset is the pan
/// modulo one cell, so the dots stay on world lattice points.
pub fn dot_transform(pan: [f32; 2], zoom: f32) -> ([f32; 2], f32) {
    // Never below 0: zoomed in, the dots just spread out (every world dot shows).
    let n = (DOT_MIN / (DOT_GAP * zoom)).log2().ceil().max(0.0);
    let cell = DOT_GAP * zoom * n.exp2();
    let phase = |p: f32| p.rem_euclid(cell) - cell;
    ([phase(pan[0]), phase(pan[1])], cell / DOT_GAP)
}

/// How much of a dot shows at `zoom` when the grid draws at `scale`: the area ratio of React
/// Flow's `zoom`-wide dot to ours (`scale`-wide), at most 1.
pub fn dot_opacity(zoom: f32, scale: f32) -> f32 {
    (zoom / scale).min(1.0).powi(2)
}

impl Camera {
    /// Move the camera. Any move but the coast's own stops a coast (new input wins).
    pub fn set(&mut self, pan: [f32; 2], zoom: f32) {
        self.coast = None;
        let zoom = zoom.clamp(MIN_ZOOM, MAX_ZOOM);
        if !(pan[0].is_finite() && pan[1].is_finite() && zoom.is_finite()) {
            return;
        }
        (self.pan, self.zoom) = (pan, zoom);
        self.live_pan.set(pan);
        self.live_zoom.set(zoom);
        let (offset, scale) = dot_transform(pan, zoom);
        self.live_dots_offset.set(offset);
        self.live_dots_scale.set(scale);
        self.live_dots_opacity.set(dot_opacity(zoom, scale));
        if let Some(f) = &self.follower {
            f(pan, zoom);
        }
    }

    /// Install (or remove) the follower `set` calls with each applied pan and zoom.
    pub fn set_follower(&mut self, f: Option<Follower>) {
        self.follower = f;
    }

    /// The world point under canvas point `q`.
    pub fn world(&self, q: [f32; 2]) -> [f32; 2] {
        [(q[0] - self.pan[0]) / self.zoom, (q[1] - self.pan[1]) / self.zoom]
    }

    /// Where world point `p` shows on the canvas.
    pub fn screen(&self, p: [f32; 2]) -> [f32; 2] {
        [p[0] * self.zoom + self.pan[0], p[1] * self.zoom + self.pan[1]]
    }

    pub fn pan_by(&mut self, dx: f32, dy: f32) {
        self.set([self.pan[0] + dx, self.pan[1] + dy], self.zoom);
    }

    /// Zoom by `factor` keeping the world point under canvas point `q` where it is.
    pub fn zoom_about(&mut self, q: [f32; 2], factor: f32) {
        let p = self.world(q);
        let zoom = (self.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
        self.set([q[0] - p[0] * zoom, q[1] - p[1] * zoom], zoom);
    }

    /// A wheel or two-finger scroll, as React Flow handles it on a Mac (`panOnScroll`,
    /// `zoomOnPinch`): plain scrolls pan 1:1 with the fingers (the OS supplies momentum), a notched
    /// mouse wheel pans vertically and with Shift horizontally, and ⌘/Ctrl + scroll zooms about the
    /// cursor on d3-zoom's curve, `2^(-deltaY · 0.002 · 10)` for pixel deltas (`dy` here is
    /// `-deltaY`), gentler per line for a notched wheel.
    /// The mode is taken when a gesture's fingers move, so the OS momentum that follows carries
    /// on as it began (zooming after ⌘ is let go mid-coast; never cut short).
    pub fn wheel(&mut self, e: &rune::WheelEvent) {
        if !e.momentum {
            self.scroll_zooms = e.mods.cmd || e.mods.ctrl;
        }
        if self.scroll_zooms {
            let k = if e.precise { 0.02 } else { 0.005 };
            self.zoom_about([e.x, e.y], (e.dy * k).exp2());
        } else if !e.precise && e.mods.shift && e.dx == 0.0 {
            self.pan_by(e.dy, 0.0);
        } else {
            self.pan_by(e.dx, e.dy);
        }
    }

    /// A trackpad pinch by `delta` (scale by about `1 + delta`) about canvas point `q`.
    pub fn pinch(&mut self, q: [f32; 2], delta: f32) {
        self.zoom_about(q, delta.exp());
    }

    /// A trackpad pinch event at `now` (the frame clock): zooms like `pinch`, and on release coasts
    /// on at the pinch's end speed unless that's slow or Reduce Motion is on.
    pub fn magnify(&mut self, e: &MagnifyEvent, now: f64) {
        if e.phase == GesturePhase::Began {
            self.pinch_track.begin(now);
        }
        // Any pinch step stops a coast (`set`), even a zero one.
        self.coast = None;
        if e.delta != 0.0 {
            self.pinch([e.x, e.y], e.delta);
            self.pinch_track.push(now, e.delta);
        }
        if e.phase == GesturePhase::Ended && !self.reduced_motion.get() {
            let v = self.pinch_track.velocity(now);
            self.coast = Coast::new([e.x, e.y], now, self.zoom, v, MIN_ZOOM, MAX_ZOOM);
        }
    }

    /// Stop a coast (a press on the canvas or a node).
    pub fn stop(&mut self) {
        self.coast = None;
    }

    pub fn coasting(&self) -> bool {
        self.coast.is_some()
    }

    /// Step a coast to `now`. True while it still moves (call again next frame).
    pub fn tick(&mut self, now: f64) -> bool {
        let Some(c) = self.coast.take() else { return false };
        let (zoom, rest) = c.at(now);
        let p = self.world(c.q);
        self.set([c.q[0] - p[0] * zoom, c.q[1] - p[1] * zoom], zoom);
        if !rest {
            self.coast = Some(c);
        }
        !rest
    }

    /// Frame world `bounds` (`[x, y, w, h]`) inside a canvas of `size`, leaving `insets`
    /// (`[left, top, right, bottom]`, the floating header and inspector) free, with `padding` as a
    /// fraction of the free area on each side (React Flow's fitView padding). Never zooms in
    /// past `max_zoom`.
    pub fn fit(&mut self, bounds: [f32; 4], size: [f32; 2], insets: [f32; 4], padding: f32, max_zoom: f32) {
        let free_w = (size[0] - insets[0] - insets[2]).max(1.0);
        let free_h = (size[1] - insets[1] - insets[3]).max(1.0);
        let avail = [free_w * (1.0 - 2.0 * padding), free_h * (1.0 - 2.0 * padding)];
        let (bw, bh) = (bounds[2].max(1.0), bounds[3].max(1.0));
        let zoom = (avail[0] / bw).min(avail[1] / bh).clamp(MIN_ZOOM, max_zoom.min(MAX_ZOOM));
        let centre = [insets[0] + free_w / 2.0, insets[1] + free_h / 2.0];
        let mid = [bounds[0] + bw / 2.0, bounds[1] + bh / 2.0];
        self.set([centre[0] - mid[0] * zoom, centre[1] - mid[1] * zoom], zoom);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zoom_about_keeps_the_point_under_the_pointer() {
        let mut c = Camera::default();
        c.set([30.0, -10.0], 0.7);
        let q = [400.0, 250.0];
        let before = c.world(q);
        c.zoom_about(q, 1.8);
        let after = c.screen(before);
        assert!((after[0] - q[0]).abs() < 1e-3 && (after[1] - q[1]).abs() < 1e-3);
    }

    #[test]
    fn dots_stay_on_world_lattice_points() {
        for &(pan, zoom) in &[([0.0, 0.0], 1.0), ([37.5, -91.0], 0.33), ([-5.0, 12.0], 1.9), ([1000.0, 3.0], 0.05)] {
            let (offset, scale) = dot_transform(pan, zoom);
            let cell = DOT_GAP * scale;
            assert!(cell >= DOT_MIN - 1e-3 && (zoom > 1.0 || cell < 2.0 * DOT_MIN), "cell {cell} at zoom {zoom}");
            // Dot (i, j) of the layer shows at offset + i * cell; it must be a world lattice point.
            let world_x = (offset[0] + 3.0 * cell - pan[0]) / zoom;
            let k = world_x / DOT_GAP;
            assert!((k - k.round()).abs() < 1e-2, "x {world_x} is no lattice point");
        }
    }

    #[test]
    fn fit_centres_bounds_in_the_free_area() {
        let mut c = Camera::default();
        c.fit([100.0, 100.0, 400.0, 200.0], [1280.0, 800.0], [0.0, 48.0, 372.0, 0.0], 0.1, 1.0);
        let mid = c.screen([300.0, 200.0]);
        assert!((mid[0] - (1280.0 - 372.0) / 2.0).abs() < 1e-3);
        assert!((mid[1] - (48.0 + (800.0 - 48.0) / 2.0)).abs() < 1e-3);
        assert!(c.zoom <= 1.0);
    }

    fn wheel(dx: f32, dy: f32, precise: bool, mods: rune::Mods) -> rune::WheelEvent {
        rune::WheelEvent::new(dx, dy).at(500.0, 300.0).precise(precise).mods(mods)
    }

    #[test]
    fn trackpad_scroll_pans_one_to_one_at_any_zoom() {
        for zoom in [0.1, 0.5, 1.0, 1.9] {
            let mut c = Camera::default();
            c.set([10.0, 20.0], zoom);
            let p = c.world([300.0, 200.0]);
            c.wheel(&wheel(-30.0, 12.5, true, rune::Mods::default()));
            let q = c.screen(p);
            assert!((q[0] - 270.0).abs() < 1e-3 && (q[1] - 212.5).abs() < 1e-3, "zoom {zoom}: {q:?}");
            assert_eq!(c.zoom, zoom, "a scroll never zooms");
        }
    }

    #[test]
    fn a_mouse_wheel_pans_vertically_and_with_shift_horizontally() {
        let mut c = Camera::default();
        c.wheel(&wheel(0.0, -40.0, false, rune::Mods::default()));
        assert_eq!(c.pan, [0.0, -40.0]);
        c.wheel(&wheel(0.0, -40.0, false, rune::Mods { shift: true, ..Default::default() }));
        assert_eq!(c.pan, [-40.0, -40.0]);
    }

    #[test]
    fn cmd_scroll_and_pinch_zoom_about_the_cursor_without_drift() {
        let mut c = Camera::default();
        c.set([-120.0, 45.0], 0.8);
        let q = [500.0, 300.0];
        let p = c.world(q);
        let cmd = rune::Mods { cmd: true, ..Default::default() };
        for dy in [10.0, 10.0, -35.0, 4.0, 60.0] {
            c.wheel(&wheel(0.0, dy, true, cmd));
        }
        for d in [0.05, 0.12, -0.3, 0.02] {
            c.pinch(q, d);
        }
        let back = c.screen(p);
        assert!((back[0] - q[0]).abs() < 1e-2 && (back[1] - q[1]).abs() < 1e-2, "drifted to {back:?}");
    }

    fn mag(delta: f32, phase: GesturePhase) -> MagnifyEvent {
        MagnifyEvent::new(delta).at(500.0, 300.0).phase(phase)
    }

    /// A quick pinch out, 20 steps at 60 Hz, ending at t = `end`.
    fn quick_pinch(c: &mut Camera) -> f64 {
        c.magnify(&mag(0.0, GesturePhase::Began), 0.0);
        let mut t = 0.0;
        for _ in 0..20 {
            t += 1.0 / 60.0;
            c.magnify(&mag(0.05, GesturePhase::Moved), t);
        }
        c.magnify(&mag(0.0, GesturePhase::Ended), t);
        t
    }

    #[test]
    fn a_quick_pinch_coasts_about_its_centre_then_rests() {
        let mut c = Camera::default();
        c.set([-120.0, 45.0], 0.5);
        let p = c.world([500.0, 300.0]);
        let end = quick_pinch(&mut c);
        let released = c.zoom;
        assert!(c.coasting());
        let mut t = end;
        while c.tick(t) {
            t += 1.0 / 120.0;
            assert!(t - end < 0.6, "coasts too long");
        }
        assert!(c.zoom > released * 1.15, "coasted on: {released} → {}", c.zoom);
        let q = c.screen(p);
        assert!((q[0] - 500.0).abs() < 1e-2 && (q[1] - 300.0).abs() < 1e-2, "drifted to {q:?}");
        assert!(!c.coasting());
    }

    #[test]
    fn new_input_stops_a_coast_at_once() {
        let mut c = Camera::default();
        // A scroll, a new pinch, a press (stop) and a programmatic move each stop it.
        let inputs: [fn(&mut Camera); 4] = [
            |c| c.wheel(&wheel(0.0, 5.0, true, rune::Mods::default())),
            |c| c.magnify(&mag(0.0, GesturePhase::Began), 10.0),
            |c| c.stop(),
            |c| c.zoom_about([0.0, 0.0], 1.2),
        ];
        for input in inputs {
            c.set([0.0, 0.0], 0.5);
            let end = quick_pinch(&mut c);
            c.tick(end + 0.02);
            input(&mut c);
            let zoom = c.zoom;
            assert!(!c.coasting() && !c.tick(end + 0.05) && c.zoom == zoom);
        }
    }

    #[test]
    fn slow_or_reduced_motion_pinches_stop_dead() {
        let mut c = Camera::default();
        c.magnify(&mag(0.0, GesturePhase::Began), 0.0);
        for i in 1..=20 {
            c.magnify(&mag(0.004, GesturePhase::Moved), i as f64 / 60.0);
        }
        c.magnify(&mag(0.0, GesturePhase::Ended), 20.0 / 60.0);
        assert!(!c.coasting(), "slow pinch");
        c.reduced_motion.set(true);
        quick_pinch(&mut c);
        assert!(!c.coasting(), "Reduce Motion");
    }

    #[test]
    fn a_coast_eases_into_max_zoom() {
        let mut c = Camera::default();
        // The pinch ends at ×e ≈ 1.63; its coast (×1.3) would overshoot 2.0.
        c.set([0.0, 0.0], 0.6);
        let mut t = quick_pinch(&mut c);
        assert!(c.zoom < MAX_ZOOM && c.coasting(), "{}", c.zoom);
        while c.tick(t) {
            t += 1.0 / 120.0;
        }
        assert!((c.zoom - MAX_ZOOM).abs() < 1e-3, "{}", c.zoom);
    }

    #[test]
    fn cmd_scroll_momentum_keeps_zooming_after_cmd_is_released() {
        let mut c = Camera::default();
        let cmd = rune::Mods { cmd: true, ..Default::default() };
        c.wheel(&wheel(0.0, 10.0, true, cmd));
        let z = c.zoom;
        let momentum = |mods| {
            let mut e = wheel(0.0, 10.0, true, mods);
            e.momentum = true;
            e
        };
        c.wheel(&momentum(rune::Mods::default()));
        assert!(c.zoom > z && c.pan != [0.0, 10.0], "momentum zooms on: {} {:?}", c.zoom, c.pan);
        // A new plain scroll pans, and its momentum pans too, even with ⌘ pressed meanwhile.
        c.wheel(&wheel(0.0, 10.0, true, rune::Mods::default()));
        let z = c.zoom;
        c.wheel(&momentum(cmd));
        assert_eq!(c.zoom, z);
    }

    #[test]
    fn zoom_matches_react_flow_and_stays_in_bounds() {
        let mut c = Camera::default();
        // React Flow: ctrl + wheel deltaY -50 (pixels) → ×2^(50·0.02) = ×2.
        c.wheel(&wheel(0.0, 50.0, true, rune::Mods { ctrl: true, ..Default::default() }));
        assert!((c.zoom - 2.0).abs() < 1e-4, "{}", c.zoom);
        c.pinch([0.0, 0.0], 5.0);
        assert_eq!(c.zoom, MAX_ZOOM);
        for _ in 0..50 {
            c.pinch([0.0, 0.0], -1.0);
        }
        assert_eq!(c.zoom, MIN_ZOOM);
    }
}
