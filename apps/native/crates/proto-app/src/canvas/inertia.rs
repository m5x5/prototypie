//! Zoom inertia after a trackpad pinch. macOS gives two-finger scrolls momentum but not pinches,
//! so the camera coasts itself: the pinch's speed in log-zoom over its last `WINDOW` carries on
//! after the fingers lift, decaying exponentially (`TAU`), about the pinch's last centre.
//!
//! The coast is analytic: `ln zoom(t) = from + amp · (1 − e^(−t/τ))`. Its total travel `amp` is
//! `v · τ`, cut short at the zoom limits, so the same curve eases into a limit instead of hitting
//! it. Small end speeds don't coast; it rests once what's left is under `REST`.

use std::collections::VecDeque;

/// The pinch's speed is measured over its last 80 ms.
pub const WINDOW: f64 = 0.08;
/// Decay time constant: a quick flick (~3 /s) rests in about 0.35 s and adds ×1.3.
pub const TAU: f64 = 0.09;
/// Below this end speed (log-zoom per second) a pinch just stops.
pub const MIN_SPEED: f32 = 0.6;
/// The coast rests once less than this (log-zoom) is left to travel.
const REST: f32 = 0.002;

/// The recent log-zoom steps of one pinch.
#[derive(Clone, Debug, Default)]
pub struct Tracker {
    began: f64,
    samples: VecDeque<(f64, f32)>,
}

impl Tracker {
    pub fn begin(&mut self, now: f64) {
        self.began = now;
        self.samples.clear();
    }

    pub fn push(&mut self, now: f64, dlog: f32) {
        self.samples.push_back((now, dlog));
        while self.samples.front().is_some_and(|&(t, _)| t < now - WINDOW) {
            self.samples.pop_front();
        }
    }

    /// Log-zoom per second over the last `WINDOW` before `now` (0 when the fingers held still).
    pub fn velocity(&self, now: f64) -> f32 {
        let sum: f32 = self.samples.iter().filter(|&&(t, _)| t >= now - WINDOW).map(|&(_, d)| d).sum();
        let span = (now - self.began).clamp(1.0 / 120.0, WINDOW);
        sum / span as f32
    }
}

/// A zoom coasting about canvas point `q`.
#[derive(Clone, Debug, PartialEq)]
pub struct Coast {
    pub q: [f32; 2],
    start: f64,
    from: f32,
    amp: f32,
}

impl Coast {
    /// A coast at speed `v` from `zoom`, staying inside `[min, max]`; None when it's too slow or
    /// already at the limit it heads for.
    pub fn new(q: [f32; 2], now: f64, zoom: f32, v: f32, min: f32, max: f32) -> Option<Coast> {
        if v.abs() < MIN_SPEED {
            return None;
        }
        let from = zoom.ln();
        let amp = (v * TAU as f32).clamp(min.ln() - from, max.ln() - from);
        (amp.abs() >= REST).then_some(Coast { q, start: now, from, amp })
    }

    /// The zoom at `now`, and whether the coast has come to rest (then it's the final zoom).
    pub fn at(&self, now: f64) -> (f32, bool) {
        let left = (-((now - self.start).max(0.0) / TAU)).exp() as f32;
        let rest = self.amp.abs() * left < REST;
        let done = if rest { 1.0 } else { 1.0 - left };
        ((self.from + self.amp * done).exp(), rest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pinch(tr: &mut Tracker, start: f64, steps: usize, dlog: f32) -> f64 {
        tr.begin(start);
        let mut t = start;
        for _ in 0..steps {
            t += 1.0 / 60.0;
            tr.push(t, dlog);
        }
        t
    }

    #[test]
    fn speed_comes_from_the_last_80_ms() {
        let mut tr = Tracker::default();
        let end = pinch(&mut tr, 0.0, 30, 0.05);
        assert!((tr.velocity(end) - 3.0).abs() < 0.4, "{}", tr.velocity(end));
        // Fingers held still for 200 ms before lifting: no speed left.
        assert_eq!(tr.velocity(end + 0.2), 0.0);
    }

    #[test]
    fn decays_monotonically_and_rests_within_half_a_second() {
        let c = Coast::new([0.0, 0.0], 1.0, 1.0, 3.0, 0.05, 2.0).unwrap();
        let mut last = 1.0;
        let mut rested_at = None;
        for i in 1..=60 {
            let t = 1.0 + i as f64 / 120.0;
            let (z, rest) = c.at(t);
            assert!(z >= last - 1e-6, "zooming in only: {z} after {last}");
            last = z;
            if rest && rested_at.is_none() {
                rested_at = Some(t - 1.0);
            }
        }
        let rested = rested_at.expect("rests");
        assert!((0.2..=0.45).contains(&rested), "rested after {rested} s");
        assert!((last - (3.0 * TAU as f32).exp()).abs() < 1e-3, "travels v·τ: {last}");
        // The step per frame shrinks: it eases out.
        let step = |t: f64| c.at(t + 1.0 / 120.0).0 - c.at(t).0;
        assert!(step(1.0) > 3.0 * step(1.15));
    }

    #[test]
    fn slow_pinches_dont_coast() {
        assert_eq!(Coast::new([0.0, 0.0], 0.0, 1.0, 0.3, 0.05, 2.0), None);
        assert_eq!(Coast::new([0.0, 0.0], 0.0, 1.0, -0.3, 0.05, 2.0), None);
    }

    #[test]
    fn eases_into_the_zoom_limits() {
        // Heading for 2.0 from 1.9 at a speed that would overshoot by far.
        let c = Coast::new([0.0, 0.0], 0.0, 1.9, 8.0, 0.05, 2.0).unwrap();
        let mut last = 1.9;
        for i in 1..=120 {
            let (z, _) = c.at(i as f64 / 120.0);
            assert!(z <= 2.0 + 1e-5 && z >= last - 1e-6, "{z}");
            last = z;
        }
        assert!((last - 2.0).abs() < 1e-4, "settles on the limit: {last}");
        // At the limit already: nothing to coast.
        assert_eq!(Coast::new([0.0, 0.0], 0.0, 2.0, 8.0, 0.05, 2.0), None);
    }
}
