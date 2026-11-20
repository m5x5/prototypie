//! The four layouts (matrix, timeline with bands, swimlanes, cards with links) for two shapes and one relation,
//! as one display list (the reference's `sceneHtml`). `html` turns it into the card's scene markup; `rune`
//! draws the same list natively for the dialog's mini previews. Only items the relation connects are placed.

use std::collections::BTreeSet;

use rune::color::hex;
use rune::{Color, El, LineCap, Shape as Vector, TextStyle, canvas, el, text};

use super::model::{Item, Relation, Shape};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Layout {
    Matrix,
    Timeline,
    Lanes,
    Cards,
}

impl Layout {
    pub const ALL: [Layout; 4] = [Layout::Matrix, Layout::Timeline, Layout::Lanes, Layout::Cards];

    pub fn name(self) -> &'static str {
        match self {
            Layout::Matrix => "Matrix",
            Layout::Timeline => "Timeline mit Bändern",
            Layout::Lanes => "Swimlanes",
            Layout::Cards => "Karten mit Verweisen",
        }
    }
}

pub fn access_label(a: &str) -> &'static str {
    match a {
        "live" => "live · 200",
        "cache" => "App-Cache",
        "ref" => "nur Verweis",
        _ => "401",
    }
}

/// Why a layout can't be drawn for this pair, or None.
pub fn unavailable(l: Layout, a: &Shape, b: &Shape, r: Option<&Relation>) -> Option<&'static str> {
    if r.is_none() {
        return Some("Keine belegte Beziehung");
    }
    if l == Layout::Timeline && !a.is_timed() && !b.is_timed() {
        return Some("braucht Zeit · ical:due / Termin");
    }
    if a.items.is_empty() || b.items.is_empty() {
        return Some("Keine belegten Einträge");
    }
    None
}

/// One drawing primitive, in scene px.
#[derive(Clone, Debug, PartialEq)]
pub enum Prim {
    Label { x: f32, y: f32, w: Option<f32>, text: String },
    Item { x: f32, y: f32, w: f32, text: String, color: String, dashed: bool },
    Cell { x: f32, y: f32, w: f32, h: f32 },
    Dot { x: f32, y: f32, color: String },
    Band { y: f32 },
    Lane { y: f32, h: f32, color: String },
    Axis { y: f32 },
    Tick { x: f32, y: f32 },
    Link { x1: f32, y1: f32, x2: f32, y2: f32, label: String },
}

fn time_label(ms: f64, with_day: bool) -> String {
    let t = (ms / 1000.0) as i64;
    let local = t + crate::features::time::utc_offset_s(t);
    let (h, m) = (local.rem_euclid(86_400) / 3600, local.rem_euclid(3600) / 60);
    if !with_day {
        return format!("{h:02}:{m:02}");
    }
    let (_, mo, d) = crate::features::time::civil_from_days(local.div_euclid(86_400));
    format!("{d:02}.{mo:02}. {h:02}:{m:02}")
}

fn day_key(ms: f64) -> i64 {
    let t = (ms / 1000.0) as i64;
    (t + crate::features::time::utc_offset_s(t)).div_euclid(86_400)
}

fn related(r: Option<&Relation>, a: &str, ai: &str, bi: &str) -> bool {
    r.is_some_and(|r| r.links(a, ai, bi))
}

/// The display list of layout `l` in a `w`×`h` scene; `k` scales fixed sizes (1 = dialog preview, 2.6 = card).
pub fn scene(l: Layout, a: &Shape, b: &Shape, r: Option<&Relation>, w: f32, h: f32, k: f32) -> Vec<Prim> {
    let mut out = Vec::new();
    let lab = |x: f32, y: f32, t: String, w: Option<f32>| Prim::Label { x, y, w, text: t };
    let it = |s: &Shape, x: f32, y: f32, w: f32, t: String, dashed: bool| Prim::Item { x, y, w, text: t, color: s.color.clone(), dashed };
    match l {
        Layout::Matrix => {
            let head = 70.0 * k;
            let cw = (70.0 * k).min((w - head) / b.items.len().max(1) as f32);
            let top = 24.0 * k;
            let rh = (28.0 * k).min((h - top) / a.items.len().max(1) as f32);
            for (j, x) in b.items.iter().enumerate() {
                out.push(lab(head + j as f32 * cw, 6.0 * k, x.label.clone(), Some(cw - 4.0)));
            }
            for (i, y) in a.items.iter().enumerate() {
                out.push(lab(6.0 * k, top + i as f32 * rh + 6.0 * k, y.label.clone(), Some(head - 10.0 * k)));
                for (j, x) in b.items.iter().enumerate() {
                    out.push(Prim::Cell { x: head + j as f32 * cw, y: top + i as f32 * rh, w: cw - 2.0, h: rh - 2.0 });
                    if related(r, &a.id, &y.id, &x.id) {
                        out.push(Prim::Dot { x: head + j as f32 * cw + cw / 2.0, y: top + i as f32 * rh + rh / 2.0, color: a.color.clone() });
                    }
                }
            }
        }
        Layout::Timeline => {
            let both = a.is_timed() && b.is_timed();
            let (timed, other) = if a.is_timed() { (a, b) } else { (b, a) };
            let all: Vec<&Item> = [a, b].iter().filter(|s| s.is_timed()).flat_map(|s| s.items.iter()).filter(|x| x.start.is_some()).collect();
            if all.is_empty() {
                return out;
            }
            let stamps: Vec<f64> = all.iter().flat_map(|x| [x.start, x.end]).flatten().collect();
            let pad = 20.0 * 60_000.0;
            let from = stamps.iter().cloned().fold(f64::MAX, f64::min) - pad;
            let to = stamps.iter().cloned().fold(f64::MIN, f64::max) + pad;
            let with_day = stamps.iter().map(|m| day_key(*m)).collect::<BTreeSet<_>>().len() > 1;
            let xof = |m: f64| 10.0 * k + ((m - from) / (to - from)) as f32 * (w - 20.0 * k);
            out.push(Prim::Axis { y: h - 22.0 * k });
            let mut uniq: Vec<f64> = stamps.clone();
            uniq.sort_by(f64::total_cmp);
            uniq.dedup();
            for (i, m) in uniq.iter().enumerate() {
                out.push(Prim::Tick { x: xof(*m), y: h - 24.0 * k });
                if i == 0 || xof(*m) - xof(uniq[i - 1]) > if with_day { 52.0 } else { 30.0 } * k {
                    out.push(lab(xof(*m) - 12.0 * k, h - 16.0 * k, time_label(*m, with_day), None));
                }
            }
            let block = |s: &Shape, x: &Item, y: f32| {
                let start = x.start.expect("timed");
                let width = (52.0 * k).max(x.end.map_or(56.0 * k, |e| xof(e) - xof(start)));
                it(s, xof(start), y, width, format!("{} {}", time_label(start, false), x.label), false)
            };
            if both {
                for (row, s) in [a, b].iter().enumerate() {
                    let y = (6.0 + row as f32 * 42.0) * k;
                    out.push(Prim::Band { y: y + 34.0 * k });
                    out.push(lab(6.0 * k, y, s.name.clone(), None));
                    for x in s.items.iter().filter(|x| x.start.is_some()) {
                        out.push(block(s, x, y + 13.0 * k));
                    }
                }
                return out;
            }
            for (i, band) in other.items.iter().enumerate() {
                let y = (8.0 + i as f32 * 40.0) * k;
                out.push(Prim::Band { y: y + 30.0 * k });
                out.push(lab(6.0 * k, y, format!("{}: {}", other.name, band.label), None));
                for x in timed.items.iter().filter(|x| x.start.is_some()) {
                    let ok = if std::ptr::eq(timed, a) { related(r, &a.id, &x.id, &band.id) } else { related(r, &a.id, &band.id, &x.id) };
                    if ok {
                        out.push(block(timed, x, y + 12.0 * k));
                    }
                }
            }
        }
        Layout::Lanes => {
            let lh = (h - 8.0 * k) / b.items.len().max(1) as f32;
            for (i, lane) in b.items.iter().enumerate() {
                let y = 4.0 * k + i as f32 * lh;
                out.push(Prim::Lane { y, h: lh - 4.0 * k, color: b.color.clone() });
                out.push(lab(10.0 * k, y + 4.0 * k, format!("{} · {}", b.name, lane.label), None));
                for (j, x) in a.items.iter().filter(|x| related(r, &a.id, &x.id, &lane.id)).enumerate() {
                    out.push(it(a, 12.0 * k + j as f32 * 92.0 * k, y + 20.0 * k, 86.0 * k, x.label.clone(), false));
                }
            }
        }
        Layout::Cards => {
            let card_w = (82.0 * k).min((w - 28.0 * k) * 0.34);
            let (ax, bx, step) = (14.0 * k, w - card_w - 14.0 * k, 34.0 * k);
            let pred = r.map(|r| r.predicate.split(' ').next().unwrap_or("").to_string()).unwrap_or_default();
            for (i, x) in a.items.iter().enumerate() {
                for (j, y) in b.items.iter().enumerate() {
                    if related(r, &a.id, &x.id, &y.id) {
                        out.push(Prim::Link { x1: ax + card_w, y1: 30.0 * k + i as f32 * step, x2: bx, y2: 30.0 * k + j as f32 * step, label: pred.clone() });
                    }
                }
            }
            for (i, x) in a.items.iter().enumerate() {
                out.push(it(a, ax, 20.0 * k + i as f32 * step, card_w, x.label.clone(), a.access == "ref"));
            }
            for (j, y) in b.items.iter().enumerate() {
                out.push(it(b, bx, 20.0 * k + j as f32 * step, card_w, y.label.clone(), b.access == "ref"));
            }
        }
    }
    out
}

pub fn esc(s: &str) -> String {
    s.chars().map(|c| match c {
        '&' => "&amp;".into(),
        '<' => "&lt;".into(),
        '>' => "&gt;".into(),
        '"' => "&quot;".into(),
        '\'' => "&#39;".into(),
        c => c.to_string(),
    }).collect()
}

/// The reference's `SCENE_CSS` (theme tokens from the host page).
pub const SCENE_CSS: &str = r#"
.lfm{position:relative;overflow:hidden;font-size:var(--lfm-fs,9.5px);line-height:1.2}
.lfm .lab{position:absolute;color:var(--muted-foreground);white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
.lfm .it{position:absolute;padding:.2em .5em;border-radius:4px;background:var(--card);border:1px solid var(--border);border-left:3px solid var(--c);white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
.lfm .it.ref{border-style:dashed;border-left-style:solid}
.lfm .band{position:absolute;left:0;right:0;border-top:1px dashed var(--border)}
.lfm .lane{position:absolute;left:4px;right:4px;border-radius:6px;background:color-mix(in oklab,var(--c) 10%,var(--card));border:1px solid var(--border)}
.lfm .cell{position:absolute;border:1px solid var(--border);background:var(--card)}
.lfm .dot{position:absolute;width:10px;height:10px;border-radius:50%;background:var(--c);translate:-50% -50%}
.lfm .axis{position:absolute;left:8px;right:8px;height:1px;background:var(--ring)}
.lfm .tick{position:absolute;width:1px;height:5px;background:var(--ring)}
.lfm svg{position:absolute;inset:0;width:100%;height:100%;overflow:visible}
.lfm svg line{stroke:var(--ring);stroke-width:1.2}
.lfm svg text{fill:var(--muted-foreground);font:calc(var(--lfm-fs,9.5px) - 1px) ui-monospace,Menlo,monospace}
"#;

/// The display list as the scene's inner HTML.
pub fn html(prims: &[Prim]) -> String {
    let mut svg = String::new();
    let mut out = String::new();
    for p in prims {
        match p {
            Prim::Label { x, y, w, text } => {
                let wd = w.map(|w| format!(";width:{w}px")).unwrap_or_default();
                out += &format!(r#"<span class="lab" style="left:{x}px;top:{y}px{wd}">{}</span>"#, esc(text));
            }
            Prim::Item { x, y, w, text, color, dashed } => {
                out += &format!(r#"<span class="it{}" style="--c:{color};left:{x}px;top:{y}px;width:{w}px" title="{t}">{t}</span>"#, if *dashed { " ref" } else { "" }, t = esc(text));
            }
            Prim::Cell { x, y, w, h } => out += &format!(r#"<span class="cell" style="left:{x}px;top:{y}px;width:{w}px;height:{h}px"></span>"#),
            Prim::Dot { x, y, color } => out += &format!(r#"<span class="dot" style="--c:{color};left:{x}px;top:{y}px"></span>"#),
            Prim::Band { y } => out += &format!(r#"<span class="band" style="top:{y}px"></span>"#),
            Prim::Lane { y, h, color } => out += &format!(r#"<span class="lane" style="--c:{color};top:{y}px;height:{h}px"></span>"#),
            Prim::Axis { y } => out += &format!(r#"<span class="axis" style="top:{y}px"></span>"#),
            Prim::Tick { x, y } => out += &format!(r#"<span class="tick" style="left:{x}px;top:{y}px"></span>"#),
            Prim::Link { x1, y1, x2, y2, label } => {
                svg += &format!(r#"<line x1="{x1}" y1="{y1}" x2="{x2}" y2="{y2}"/><text text-anchor="middle" x="{}" y="{}">{}</text>"#, (x1 + x2) / 2.0, (y1 + y2) / 2.0 - 4.0, esc(label));
            }
        }
    }
    if svg.is_empty() { out } else { format!(r#"<svg aria-hidden="true">{svg}</svg>{out}"#) }
}

/// The same display list drawn natively (dialog previews), `w`×`h` points.
pub fn rune(prims: &[Prim], w: f32, h: f32, fg: Color, muted: Color, border: Color, card: Color) -> El {
    let font = TextStyle::new(8.5, 500.0);
    let c = |s: &str| hex(s).unwrap_or(muted);
    let mut root = el().size(w, h).clip_soft(0.0);
    let mut lines = Vector::new();
    let mut has_lines = false;
    for p in prims {
        let child = match p {
            Prim::Label { x, y, w, text: t } => {
                let e = text(t.clone()).font(font).color(muted).truncate().absolute(*x, *y);
                if let Some(w) = w { e.w(w.max(1.0)) } else { e }
            }
            Prim::Item { x, y, w, text: t, color, dashed } => {
                let mut e = el().absolute(*x, *y).w(*w).h(16.0).px(4.0).radius(4.0).fill(card).border(1.0, border).center_y()
                    .child(el().absolute(0.0, 0.0).w(3.0).h(16.0).fill(c(color)))
                    .child(text(t.clone()).font(font).color(fg).truncate());
                if *dashed {
                    e = e.border_dash(3.0, 2.0);
                }
                e
            }
            Prim::Cell { x, y, w, h } => el().absolute(*x, *y).size(*w, *h).fill(card).border(1.0, border),
            Prim::Dot { x, y, color } => el().absolute(x - 5.0, y - 5.0).size(10.0, 10.0).radius(5.0).fill(c(color)),
            Prim::Band { y } => el().absolute(0.0, *y).w(w).h(1.0).fill(border),
            Prim::Lane { y, h: lh, color } => el().absolute(4.0, *y).w(w - 8.0).h(*lh).radius(6.0).fill(c(color).alpha(0.1)).border(1.0, border),
            Prim::Axis { y } => el().absolute(8.0, *y).w(w - 16.0).h(1.0).fill(muted),
            Prim::Tick { x, y } => el().absolute(*x, *y).w(1.0).h(5.0).fill(muted),
            Prim::Link { x1, y1, x2, y2, .. } => {
                lines = lines.line([*x1, *y1], [*x2, *y2], 1.2, LineCap::Round, muted);
                has_lines = true;
                continue;
            }
        };
        root = root.child(child);
    }
    if has_lines {
        root = root.child(canvas(lines).absolute(0.0, 0.0).size(w, h).inert());
    }
    root
}

/// The card document for the chosen layout, provenance under the scene (the reference's `layoutCardHtml`).
pub fn card_html(title: &str, l: Layout, a: &Shape, b: &Shape, r: &Relation) -> String {
    let (w, h, k) = (1120.0, 520.0, 2.6);
    let scene_html = html(&scene(l, a, b, Some(r), w, h, k));
    let row = |label: &str, value: String| format!("<div class=\"row\"><dt>{}</dt><dd>{value}</dd></div>", esc(label));
    let src = |s: &Shape| format!("<code>{}</code> <span class=\"tag\">{}</span>", esc(&s.source), esc(access_label(&s.access)));
    let via = r.via.as_ref().map(|v| format!(" über {}", esc(v))).unwrap_or_default();
    let evidence = r.evidence.as_ref().map(|e| row("Beleg", esc(e))).unwrap_or_default();
    format!(r#"<!doctype html><html lang="de"><head><meta charset="utf-8"><title>{t}</title>
<link rel="stylesheet" href="https://fonts.googleapis.com/css2?family=Google+Sans+Flex:wght@400;500;600;700&display=swap">
<style>
:root{{--background:#fff;--foreground:#18181b;--muted-foreground:#71717a;--border:#e4e4e7;--card:#fff;--ring:#a1a1aa;--muted:#f4f4f5}}
@media (prefers-color-scheme:dark){{:root{{--background:#0a0a0a;--foreground:#fafafa;--muted-foreground:#a1a1aa;--border:#27272a;--card:#141414;--ring:#52525b;--muted:#18181b}}}}
*{{box-sizing:border-box;margin:0}}
body{{background:var(--background);color:var(--foreground);font:15px/1.5 "Google Sans Flex",system-ui,sans-serif;padding:40px}}
.k{{font-size:12px;font-weight:600;letter-spacing:.08em;text-transform:uppercase;color:var(--muted-foreground)}}
h1{{font-size:30px;font-weight:650;margin:6px 0 20px;overflow-wrap:anywhere}}
.stage{{width:{w}px;max-width:100%;height:{h}px;border:1px solid var(--border);border-radius:14px;background:var(--muted);--lfm-fs:15px}}
dl{{display:grid;gap:8px;margin-top:22px;max-width:{w}px}}
.row{{display:grid;grid-template-columns:150px 1fr;gap:12px}}
dt{{color:var(--muted-foreground)}}
dd{{overflow-wrap:anywhere}}
code{{font:13px ui-monospace,SFMono-Regular,Menlo,monospace}}
.tag{{display:inline-block;margin-left:6px;padding:0 8px;border-radius:999px;background:var(--muted);border:1px solid var(--border);font-size:12px}}
{css}
</style></head><body>
<div class="k">{name}</div>
<h1>{t}</h1>
<div class="stage lfm" role="img" aria-label="{aria}">{scene_html}</div>
<dl>
{ra}
{rb}
{rr}
{evidence}
</dl>
</body></html>"#,
        t = esc(title),
        css = SCENE_CSS,
        name = esc(l.name()),
        aria = esc(&format!("{}: {} und {} über {}", l.name(), a.name, b.name, r.predicate)),
        ra = row(&a.name, src(a)),
        rb = row(&b.name, src(b)),
        rr = row("Beziehung", format!("<code>{}</code>{via} <span class=\"tag\">{}</span>", esc(&r.predicate), esc(access_label(&r.access)))),
    )
}
