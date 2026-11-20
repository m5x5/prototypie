//! What a thumbnail captures, as pure functions of the request and the page's measurements
//! (the reference app's `design-preview.tsx` rules). Platform independent and unit tested.

/// A card thumbnail request (see docs/api-web.md).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ThumbSpec {
    /// Layout width in CSS px (`design.viewport`, 1280 by default).
    pub viewport_w: u32,
    /// A fixed-size screen (`design.viewportHeight`).
    pub viewport_h: Option<u32>,
    /// Device px of the box width the card draws it at.
    pub out_w: u32,
}

/// Below this width a design is a phone screen.
pub const PHONE_W: u32 = 600;
/// Crop when the content spans at most this share of the viewport width.
pub const CROP_MAX_SHARE: f64 = 0.6;
/// Elements at least this share of the viewport wide are layout wrappers (unless media).
pub const WRAPPER_SHARE: f64 = 0.9;
pub const CROP_PAD: f64 = 24.0;
/// Never more device pixels than this per CSS px.
pub const MAX_DENSITY: f64 = 2.0;

impl ThumbSpec {
    pub fn phone(&self) -> bool {
        self.viewport_w < PHONE_W
    }

    /// The layout viewport height (what `100vh` means while rendering).
    pub fn layout_h(&self) -> u32 {
        match self.viewport_h {
            Some(h) => h.max(1),
            None if self.phone() => (self.viewport_w * 17).div_ceil(9),
            None => (self.viewport_w * 3).div_ceil(4),
        }
    }

    /// The content crop applies to thumbnails of non-phone, non-fixed screens.
    pub fn crops(&self) -> bool {
        self.viewport_h.is_none() && !self.phone()
    }
}

/// What the page reported after load (CSS px).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Measured {
    pub doc_w: f64,
    pub doc_h: f64,
    /// The padded content box when the crop rule applies (already `contentCrop`'s output).
    pub crop: Option<[f64; 4]>,
}

/// What to capture and at which pixel size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Plan {
    /// Captured area in viewport CSS px: x, y, w, h.
    pub rect: [f64; 4],
    pub crop: Option<[f64; 4]>,
    pub out_w: u32,
    pub out_h: u32,
}

/// The capture plan for a thumbnail, given the page's measurements.
pub fn plan(spec: &ThumbSpec, m: &Measured) -> Plan {
    let vw = spec.viewport_w.max(1) as f64;
    let crop = if spec.crops() { m.crop.filter(|c| c[2] > 0.0 && c[3] > 0.0) } else { None };
    let rect = match crop {
        // Tall content shows at most 3 widths of it (the card's "tall content" box).
        Some([x, y, w, h]) => [x, y, w, h.min(w * 3.0)],
        None => [0.0, 0.0, vw, spec.layout_h() as f64],
    };
    let (out_w, out_h) = out_size(spec.out_w, rect);
    Plan { rect, crop: crop.map(|_| rect), out_w, out_h }
}

/// Pixel size for a captured rect: the requested width, at most `MAX_DENSITY` px per CSS px.
pub fn out_size(want_w: u32, rect: [f64; 4]) -> (u32, u32) {
    let w = (want_w as f64).min((rect[2] * MAX_DENSITY).ceil()).max(1.0);
    let h = (w * rect[3] / rect[2].max(1.0)).round().max(1.0);
    (w as u32, h as u32)
}

/// Where relative URLs in designs (`/vendor/...`, `/imports/...`) resolve: the embedded
/// server's origin, overridable with `PROTOTYPIE_ASSET_BASE`.
pub fn default_asset_base() -> String {
    std::env::var("PROTOTYPIE_ASSET_BASE").unwrap_or_else(|_| "http://127.0.0.1:3124/".to_string())
}

/// Width buckets the cache renders at, so small zoom changes don't re-render.
pub const BUCKET: u32 = 256;
/// A cached snapshot serves requests up to this many times its rendered bucket.
pub const REUSE_UP_TO: f64 = 1.25;

pub fn bucket(out_w: u32) -> u32 {
    out_w.max(1).div_ceil(BUCKET) * BUCKET
}

/// Whether a snapshot rendered at `have` bucket serves a request for `want` px.
pub fn serves(have: u32, want: u32) -> bool {
    want as f64 <= have as f64 * REUSE_UP_TO
}

/// The reference's thumbnail CSS: animations frozen at their end, transitions off, tool chrome
/// hidden, phone screens without their page background.
pub fn preview_css(phone: bool) -> String {
    let mut css = String::from(
        "<style data-prototypie-preview>\
*,*::before,*::after{animation-delay:-60s!important;animation-play-state:paused!important;transition:none!important;scroll-behavior:auto!important}\
.dialkit-root,.dialkit-panel{display:none!important}",
    );
    if phone {
        css.push_str("html,body{background:transparent!important}");
    }
    css.push_str("</style>");
    css
}

/// Inserts the preview CSS before `</head>` (case-insensitive), or in front of the document.
pub fn for_preview(html: &str, phone: bool) -> String {
    let css = preview_css(phone);
    match find_ci(html, "</head>") {
        Some(i) => format!("{}{}{}", &html[..i], css, &html[i..]),
        None => format!("{css}{html}"),
    }
}

fn find_ci(hay: &str, needle: &str) -> Option<usize> {
    let n = needle.len();
    hay.as_bytes().windows(n).position(|w| w.eq_ignore_ascii_case(needle.as_bytes()))
}

#[cfg(test)]
#[path = "spec_tests.rs"]
mod tests;
