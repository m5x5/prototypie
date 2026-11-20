//! proto-web: HTML rendering for Prototypie on macOS via WKWebView (see docs/api-web.md).
//!
//! - [`spec`]: the thumbnail rules as pure functions (`ThumbSpec`, capture `Plan`).
//! - `Snapshots`: thumbnail cache + renderer + worker, producing `rune::Pixels`.
//! - `LiveView`: an interactive WKWebView over a rect of a Rune window.
//! - `mac::render`: the main-thread WKWebView snapshot renderer (offscreen, never shown).

pub mod resample;
pub mod spec;

#[cfg(target_os = "macos")]
pub mod mac;

pub use spec::ThumbSpec;

#[cfg(target_os = "macos")]
pub use mac::render::prewarm;
#[cfg(target_os = "macos")]
pub use mac::live::{LiveKey, LiveOptions, LivePage, LiveView};
#[cfg(target_os = "macos")]
pub use mac::annotate::{AnnotateEvent, WebPick, WebPin};
#[cfg(target_os = "macos")]
pub use mac::snapshots::{Snapshot, SnapshotLimits, Snapshots, Thumb, Wake};
