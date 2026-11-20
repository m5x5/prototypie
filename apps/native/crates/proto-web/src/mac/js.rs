//! Scripts run inside rendered pages. Ported from the reference app's `design-preview.tsx`.

/// How long a thumbnail page may run after load and web fonts before it is frozen and captured.
pub const SETTLE_MS: u32 = 400;

/// Async function body for `callAsyncJavaScript`: waits for fonts and a settle, stops the page's
/// JS work (`quiesce`), measures the document and, when `crop` is true, the content crop
/// (`contentCrop(frame, viewport, 0.6, 24)`). Returns a JSON string `{docW, docH, crop}`.
pub fn measure(viewport_w: u32, crop: bool, settle_ms: u32) -> String {
    format!(
        r#"await document.fonts.ready;
await new Promise(r => setTimeout(r, {settle_ms}));
try {{
  const top = setTimeout(() => {{}}, 0);
  for (let id = 0; id <= top; id++) {{ clearTimeout(id); clearInterval(id); }}
  window.requestAnimationFrame = () => 0;
  window.setInterval = () => 0;
  document.getAnimations?.().forEach(a => a.pause());
  document.querySelectorAll("video,audio").forEach(m => m.pause());
}} catch (e) {{}}
const d = document.documentElement, b = document.body;
const docW = Math.max(d.scrollWidth, b ? b.scrollWidth : 0);
const docH = Math.max(d.scrollHeight, b ? b.scrollHeight : 0);
let crop = null;
if ({crop} && b) {{
  const vw = {viewport_w}, pad = {pad};
  let x0 = Infinity, x1 = -Infinity, y0 = Infinity, y1 = -Infinity;
  for (const el of b.querySelectorAll("*")) {{
    const r = el.getBoundingClientRect();
    if (r.width <= 0 || r.height <= 0) continue;
    if (r.width >= vw * {wrapper} && !el.matches("img,svg,canvas,video")) continue;
    if (r.right <= 0 || r.left >= vw) continue;
    x0 = Math.min(x0, Math.max(0, r.left));
    x1 = Math.max(x1, Math.min(vw, r.right));
    y0 = Math.min(y0, Math.max(0, r.top));
    y1 = Math.max(y1, r.bottom);
  }}
  if (x1 > x0 && x1 - x0 <= vw * {share}) {{
    y1 = Math.min(y1, d.scrollHeight);
    const x = Math.max(0, x0 - pad), y = Math.max(0, y0 - pad);
    for (const el of [d, b]) el.style.setProperty("background-image", "none", "important");
    crop = [x, y, Math.min(vw - x, x1 - x0 + pad * 2), y1 + pad - y];
  }}
}}
return JSON.stringify({{ docW, docH, crop }});"#,
        pad = crate::spec::CROP_PAD,
        wrapper = crate::spec::WRAPPER_SHARE,
        share = crate::spec::CROP_MAX_SHARE,
    )
}

/// Parses `measure`'s JSON result.
pub fn parse_measured(json: &str) -> Option<crate::spec::Measured> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let crop = v.get("crop").and_then(|c| c.as_array()).and_then(|a| {
        let n: Vec<f64> = a.iter().filter_map(|x| x.as_f64()).collect();
        (n.len() == 4).then(|| [n[0], n[1], n[2], n[3]])
    });
    Some(crate::spec::Measured { doc_w: v.get("docW")?.as_f64()?, doc_h: v.get("docH")?.as_f64()?, crop })
}

/// Injected into live views: reports whether an editable element has focus, so ←/→ go to it.
pub const FOCUS_TRACKER: &str = r#"(() => {
  const post = () => {
    let a = document.activeElement;
    // Into open shadow roots (the annotate popover's textarea lives in one).
    while (a && a.shadowRoot && a.shadowRoot.activeElement) a = a.shadowRoot.activeElement;
    const editable = !!a && (a.isContentEditable || /^(INPUT|TEXTAREA|SELECT)$/.test(a.tagName));
    try { window.webkit.messageHandlers.protoFocus.postMessage(editable); } catch (e) {}
  };
  addEventListener("focusin", post, true);
  addEventListener("focusout", () => setTimeout(post, 0), true);
})();"#;

#[cfg(test)]
mod tests {
    #[test]
    fn parses_measurements() {
        let m = super::parse_measured(r#"{"docW":1280,"docH":960.5,"crop":[1,2,3,4]}"#).unwrap();
        assert_eq!((m.doc_w, m.doc_h, m.crop), (1280.0, 960.5, Some([1.0, 2.0, 3.0, 4.0])));
        assert_eq!(super::parse_measured(r#"{"docW":1,"docH":2,"crop":null}"#).unwrap().crop, None);
    }

    #[test]
    fn measure_script_embeds_the_rule_constants() {
        let js = super::measure(1280, true, 400);
        assert!(js.contains("vw = 1280, pad = 24"));
        assert!(js.contains("vw * 0.6"));
    }
}
