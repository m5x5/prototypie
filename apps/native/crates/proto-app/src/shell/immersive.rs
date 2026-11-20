//! Immersive mode: the window covers the whole display, up into the band beside the camera
//! housing on a notched MacBook. macOS full screen can't do that (it places a window below the
//! housing and blacks out the band, with no opt-out), so the app does it itself, as
//! pre-Lion full screen did: menu bar and Dock auto-hide (`presentationOptions`), the window turns
//! borderless (an ordinary titled window is kept below the menu bar) and takes the screen's whole
//! frame. The canvas runs edge to edge under the housing; the header's left and right groups sit
//! beside it (`notch`, from `NSScreen.auxiliaryTopLeftArea`/`auxiliaryTopRightArea`), so nothing
//! interactive is under it.
//!
//! Entered from the window's green traffic light (its action is taken over, so it never starts macOS
//! full screen) or ⌃⌘F; left the same way or with Esc. Headless runs never touch a window.

use rune::{Key, KeyEvent, Tx};

use crate::app::state::AppState;

/// The window's free top band beside the camera housing, in window points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Notch {
    /// Width free left of the housing.
    pub left_w: f32,
    /// Where the free area right of the housing starts.
    pub right_x: f32,
    /// The housing's height (the screen's top safe-area inset).
    pub band_h: f32,
}

/// What to put back on leaving: the window's frame (AppKit screen coordinates) and style mask.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Restore {
    frame: [f64; 4],
    style: usize,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub enum Immersive {
    #[default]
    Off,
    On { restore: Restore, notch: Option<Notch> },
}

pub fn active(s: &AppState) -> bool {
    matches!(s.shell.immersive, Immersive::On { .. })
}

/// The housing's band while immersive on a notched display.
pub fn notch(s: &AppState) -> Option<Notch> {
    match s.shell.immersive {
        Immersive::On { notch, .. } => notch,
        Immersive::Off => None,
    }
}

/// The fullscreen button and ⌃⌘F: leave macOS full screen if the window is in it (green light),
/// else enter or leave immersive mode.
pub fn toggle(s: &mut AppState) {
    if rune::is_headless() {
        return;
    }
    if mac::leave_full_screen() {
        return;
    }
    match s.shell.immersive {
        Immersive::On { restore, .. } => {
            mac::exit(&restore);
            s.shell.immersive = Immersive::Off;
        }
        Immersive::Off => {
            if let Some((restore, notch)) = mac::enter() {
                s.shell.immersive = Immersive::On { restore, notch };
            }
        }
    }
}

/// From `AppState::poll`: the green light was pressed → toggle immersive mode. The light's action is taken over
/// once the window exists.
pub fn poll(s: &mut AppState) -> bool {
    if rune::is_headless() {
        return false;
    }
    mac::install_zoom_hook();
    if mac::take_zoom_press() {
        toggle(s);
        return true;
    }
    false
}

/// Esc leaves immersive mode (when nothing before it took the key); ⌃⌘F toggles it.
pub fn on_key(s: &mut AppState, ev: &KeyEvent, tx: &mut Tx) -> bool {
    let chord = ev.mods.cmd && ev.mods.ctrl && matches!(&ev.key, Key::Text(t) if t.eq_ignore_ascii_case("f") || t == "\u{6}");
    let esc = ev.key == Key::Escape && active(s);
    if !(chord || esc) {
        return false;
    }
    toggle(s);
    tx.instant();
    true
}

/// AppKit's side, on the main thread (handlers run there).
#[cfg(target_os = "macos")]
mod mac {
    use objc2::MainThreadMarker;
    use objc2::rc::Retained;
    use objc2_app_kit::{NSApplication, NSApplicationPresentationOptions, NSWindow, NSWindowStyleMask};
    use objc2_foundation::{NSPoint, NSRect, NSSize};

    use std::sync::atomic::{AtomicBool, Ordering};

    use objc2::runtime::{AnyObject, NSObject};
    use objc2::{MainThreadOnly, define_class, msg_send, sel};
    use objc2_app_kit::NSWindowButton;

    use super::{Notch, Restore};

    static ZOOM_PRESSED: AtomicBool = AtomicBool::new(false);
    static HOOKED: AtomicBool = AtomicBool::new(false);

    define_class!(
        /// The green light's new target: it only records the press (`poll` acts on it).
        #[unsafe(super(NSObject))]
        #[thread_kind = MainThreadOnly]
        #[name = "ProtoZoomTarget"]
        struct ZoomTarget;

        impl ZoomTarget {
            #[unsafe(method(zoomPressed:))]
            fn zoom_pressed(&self, _sender: Option<&AnyObject>) {
                ZOOM_PRESSED.store(true, Ordering::SeqCst);
            }
        }
    );

    /// Point the green light at `ZoomTarget`, once a window has one.
    pub fn install_zoom_hook() {
        if HOOKED.load(Ordering::SeqCst) {
            return;
        }
        let Some(mtm) = MainThreadMarker::new() else { return };
        let Some(w) = window(mtm) else { return };
        let Some(button) = w.standardWindowButton(NSWindowButton::ZoomButton) else { return };
        let target: Retained<ZoomTarget> = unsafe { msg_send![ZoomTarget::alloc(mtm), init] };
        unsafe {
            button.setTarget(Some(&target));
            button.setAction(Some(sel!(zoomPressed:)));
        }
        // The button holds its target weakly: keep this one for the process's life.
        std::mem::forget(target);
        HOOKED.store(true, Ordering::SeqCst);
    }

    pub fn take_zoom_press() -> bool {
        ZOOM_PRESSED.swap(false, Ordering::SeqCst)
    }

    /// The key or main window. Never in unit tests: with one test thread they run on the main
    /// thread, and the result must not depend on the process's AppKit state.
    fn window(mtm: MainThreadMarker) -> Option<Retained<NSWindow>> {
        if cfg!(test) {
            return None;
        }
        let app = NSApplication::sharedApplication(mtm);
        app.keyWindow().or_else(|| app.mainWindow())
    }

    /// The window is in macOS full screen (green light): take it out; true when it was.
    pub fn leave_full_screen() -> bool {
        let Some(w) = MainThreadMarker::new().and_then(window) else { return false };
        if !w.styleMask().contains(NSWindowStyleMask::FullScreen) {
            return false;
        }
        w.toggleFullScreen(None);
        true
    }

    pub fn enter() -> Option<(Restore, Option<Notch>)> {
        let mtm = MainThreadMarker::new()?;
        let w = window(mtm)?;
        let screen = w.screen()?;
        let r = w.frame();
        let restore = Restore { frame: [r.origin.x, r.origin.y, r.size.width, r.size.height], style: w.styleMask().0 };
        let f = screen.frame();
        NSApplication::sharedApplication(mtm)
            .setPresentationOptions(NSApplicationPresentationOptions::AutoHideDock | NSApplicationPresentationOptions::AutoHideMenuBar);
        w.setStyleMask(NSWindowStyleMask::Borderless);
        // No window shadow: on a borderless window macOS draws it (and its hairline) as a frame around the app, which
        // the screen's edges and the camera housing then cut off.
        w.setHasShadow(false);
        w.setFrame_display(f, true);
        let top = screen.safeAreaInsets().top;
        let notch = (top > 0.0).then(|| {
            let (l, rr) = (screen.auxiliaryTopLeftArea(), screen.auxiliaryTopRightArea());
            Notch { left_w: l.size.width as f32, right_x: (f.size.width - rr.size.width) as f32, band_h: top as f32 }
        });
        Some((restore, notch))
    }

    pub fn exit(r: &Restore) {
        let Some(mtm) = MainThreadMarker::new().filter(|_| !cfg!(test)) else { return };
        if let Some(w) = window(mtm) {
            w.setStyleMask(NSWindowStyleMask(r.style));
            w.setHasShadow(true);
            let [x, y, wd, h] = r.frame;
            w.setFrame_display(NSRect::new(NSPoint::new(x, y), NSSize::new(wd, h)), true);
        }
        NSApplication::sharedApplication(mtm).setPresentationOptions(NSApplicationPresentationOptions::Default);
    }
}

#[cfg(not(target_os = "macos"))]
mod mac {
    use super::{Notch, Restore};
    pub fn leave_full_screen() -> bool {
        false
    }
    pub fn enter() -> Option<(Restore, Option<Notch>)> {
        None
    }
    pub fn exit(_: &Restore) {}
    pub fn install_zoom_hook() {}
    pub fn take_zoom_press() -> bool {
        false
    }
}

/// A notched 14" MacBook Pro (1512 × 982 points, 32 pt housing band), for `--snap` scenes.
pub fn scene_notch(s: &mut AppState) {
    s.shell.immersive = Immersive::On { restore: Restore::default(), notch: Some(Notch { left_w: 662.0, right_x: 850.0, band_h: 32.0 }) };
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(k: Key, cmd: bool, ctrl: bool) -> KeyEvent {
        KeyEvent { key: k, mods: rune::Mods { cmd, ctrl, ..Default::default() } }
    }

    #[test]
    fn esc_is_only_taken_while_immersive_and_the_chord_always() {
        let mut s = AppState::new();
        let mut tx = Tx::default();
        assert!(!on_key(&mut s, &key(Key::Escape, false, false), &mut tx), "Esc stays free when windowed");
        assert!(!on_key(&mut s, &key(Key::Text("f".into()), true, false), &mut tx), "⌘F isn't ours");
        assert!(on_key(&mut s, &key(Key::Text("f".into()), true, true), &mut tx), "⌃⌘F");
        scene_notch(&mut s);
        assert_eq!(notch(&s).map(|n| n.right_x > n.left_w), Some(true));
        assert!(on_key(&mut s, &key(Key::Escape, false, false), &mut tx), "Esc leaves");
        assert!(!active(&s) && notch(&s).is_none());
    }
}
