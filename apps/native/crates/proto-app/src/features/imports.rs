//! Imports (the reference's `features/imports`): script-built bundles of designs (`GET /api/bundles`, listed in
//! the ⋯ menu) imported into a new project or exploration, and "+ → Import from app…": a one-card agent job
//! that recreates an element of the project's app from its source (Claude agent only).

use std::collections::HashMap;

use proto_core::{Change, DesignKind, RecordKind, VariationRequest, ops};
use rune::{Color, El, Latest, TextEdit, TextStyle, Tx, Waker, col, el, row, text, text_input};
use serde::Deserialize;
use serde_json::json;

use crate::app::state::{AppState, ROOT_SPACE};
use crate::app::style::{self, Palette};
use crate::features::launch::{self, Launch, NODE_W};
use crate::features::{http, nav, notify};

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct BundleInfo {
    pub file: String,
    pub name: String,
    pub count: usize,
}

#[derive(Clone, Debug, Deserialize)]
struct BundleDesign {
    key: Option<String>,
    #[serde(rename = "parentKey")]
    parent_key: Option<String>,
    #[serde(default)]
    group: String,
    title: String,
    #[serde(default)]
    rationale: String,
    html: String,
    viewport: Option<f64>,
    source: Option<String>,
    instruction: Option<String>,
    kind: Option<String>,
    starred: Option<bool>,
    position: Option<proto_core::Point>,
}

#[derive(Clone, Debug, Deserialize)]
struct BundleFile {
    name: String,
    designs: Vec<BundleDesign>,
}

/// The "Import from app" dialog while open.
pub struct FromApp {
    pub element: TextEdit,
    pub source: TextEdit,
    /// The designer typed into Source: the project prefill must not overwrite it.
    pub source_touched: bool,
    /// The optional screenshot: (file name, data: URL).
    pub screenshot: Option<(String, String)>,
}

/// Largest screenshot accepted (the reference's 8 MB).
const MAX_SHOT: usize = 8 * 1024 * 1024;

/// Standard base64 (no line breaks) for data: URLs.
pub fn base64(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for c in bytes.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if c.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if c.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}

fn mime_for(name: &str) -> Option<&'static str> {
    let ext = name.rsplit('.').next()?.to_ascii_lowercase();
    Some(match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        _ => return None,
    })
}

/// "Screenshot (optional)": a file picker; the file is read on a worker, never on the UI thread.
pub fn pick_screenshot(_s: &mut AppState, tx: &mut Tx) {
    let opts = rune::FileDialogOptions { title: "Screenshot".into(), ..Default::default() }.filter("Images", &["png", "jpg", "jpeg", "webp", "gif"]);
    tx.open_file(opts, |s: &mut AppState, result, _tx| match result {
        Ok(Some(path)) => {
            s.features.imports.shot_req.request(path, |path: std::path::PathBuf| {
                let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                let mime = mime_for(&name).ok_or("Use a PNG, JPEG, WebP or GIF screenshot.")?;
                let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
                if bytes.len() > MAX_SHOT {
                    return Err("Screenshot is larger than 8 MB.".into());
                }
                Ok((name, format!("data:{mime};base64,{}", base64(&bytes))))
            });
        }
        Ok(None) => {}
        Err(e) => notify(s, "Couldn't open the file dialog", &e.to_string()),
    });
}

/// An image pasted into the dialog becomes the screenshot.
pub fn paste_screenshot(s: &mut AppState, content: &rune::ClipboardContent) -> bool {
    let Some(img) = content.image() else { return false };
    let png = img.png();
    if png.len() > MAX_SHOT {
        notify(s, "Screenshot is larger than 8 MB.", "");
        return true;
    }
    if let Some(f) = &mut s.features.imports.from_app {
        f.screenshot = Some(("Pasted image.png".into(), format!("data:image/png;base64,{}", base64(png))));
    }
    true
}

#[derive(Default)]
pub struct ImportsState {
    /// None = loading (after `on_more_open`), Some = the list.
    pub bundles: Option<Vec<BundleInfo>>,
    pub busy: bool,
    pub from_app: Option<FromApp>,
    list_req: Latest<Vec<BundleInfo>>,
    file_req: Latest<Result<BundleFile, String>>,
    projects_req: Latest<Option<String>>,
    shot_req: Latest<Result<(String, String), String>>,
}

impl ImportsState {
    pub fn setup(&mut self, waker: Waker) {
        self.list_req.set_waker(waker.clone());
        self.file_req.set_waker(waker.clone());
        self.projects_req.set_waker(waker.clone());
        self.shot_req.set_waker(waker);
    }
}

/// The ⋯ menu opened: list the bundles again.
pub fn on_more_open(s: &mut AppState) {
    s.features.imports.bundles = None;
    s.features.imports.list_req.request(http::server_addr(), |a| {
        http::request(&a, "GET", "/api/bundles", None).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    });
}

pub fn import_bundle(s: &mut AppState, file: &str) {
    s.features.imports.busy = true;
    let (a, f) = (http::server_addr(), file.to_string());
    s.features.imports.file_req.request((a, f), |(a, f)| {
        let b = http::request(&a, "GET", &f, None).map_err(|e| format!("Bundle missing ({e})."))?;
        serde_json::from_slice::<BundleFile>(&b).map_err(|e| e.to_string())
    });
}

/// Create the bundle's space (a project on the main canvas, else an exploration of the current space) and its
/// designs: positions and parent links from the bundle when present, else one row per group.
fn place_bundle(s: &mut AppState, data: BundleFile) {
    let here = s.route.space.clone();
    let created = if here == ROOT_SPACE { ops::create_project(s.snap(), &data.name, None).0 } else { ops::create_exploration(s.snap(), &here, Some(&data.name), None) };
    let space = created.space_id.clone();
    let mut changes = created.changes;
    let mut groups: Vec<&str> = Vec::new();
    for d in &data.designs {
        if !groups.contains(&d.group.as_str()) {
            groups.push(&d.group);
        }
    }
    let mut id_by_key: HashMap<String, String> = HashMap::new();
    let mut rows: Vec<(String, serde_json::Value, Option<String>)> = Vec::new();
    let now = ops::now_ms();
    for (row_i, g) in groups.iter().enumerate() {
        for (col_i, d) in data.designs.iter().filter(|d| d.group == *g).enumerate() {
            let id = ops::new_id(10);
            if let Some(k) = &d.key {
                id_by_key.insert(k.clone(), id.clone());
            }
            let pos = d.position.unwrap_or(proto_core::Point { x: col_i as f64 * (NODE_W + 160.0), y: row_i as f64 * 620.0 });
            let instruction = d.instruction.clone().or_else(|| d.source.as_ref().map(|src| format!("Imported from {src}")));
            let mut v = json!({"id": id, "spaceId": space, "kind": d.kind.clone().unwrap_or_else(|| "page".into()), "title": d.title,
                "rationale": d.rationale, "html": d.html, "status": "ready", "position": {"x": pos.x, "y": pos.y}, "createdAt": now});
            if let Some(vp) = d.viewport { v["viewport"] = json!(vp); }
            if let Some(st) = d.starred { v["starred"] = json!(st); }
            if let Some(i) = instruction { v["instruction"] = json!(i); }
            rows.push((id, v, d.parent_key.clone()));
        }
    }
    for (id, mut v, parent_key) in rows {
        if let Some(pid) = parent_key.and_then(|k| id_by_key.get(&k).cloned()) {
            v["parentId"] = json!(pid);
        }
        changes.push(Change::set(RecordKind::Designs.path(&id), v));
    }
    let n = data.designs.len();
    if nav::apply(s, changes, "import bundle") {
        nav::enter_space(s, &space, None);
        notify(s, &format!("Imported {n} designs from {}", data.name), "");
    }
}

/* ---------------------------------------------------------------- import from app */

/// The top-level space above the active one (the project), or None on the main canvas.
fn project_name(s: &AppState) -> Option<String> {
    s.snap().project_of(&s.route.space).map(|p| p.name.clone())
}

/// "+ → Import from app…": the dialog, its Source prefilled from the project's folder.
pub fn open_from_app(s: &mut AppState, _tx: &mut Tx) {
    let here = s.route.space.clone();
    if !crate::features::nested::can_create_in(s, &here) {
        return;
    }
    let mut element = TextEdit::new("");
    element.focused = true;
    s.features.imports.from_app = Some(FromApp { element, source: TextEdit::new(""), source_touched: false, screenshot: None });
    let local = s.snap().local_path_of(&here);
    if let Some(lp) = local {
        if let Some(f) = &mut s.features.imports.from_app {
            f.source = TextEdit::new(lp);
        }
        return;
    }
    let Some(name) = project_name(s) else { return };
    s.features.imports.projects_req.request((http::server_addr(), name), |(a, name)| {
        let path = format!("/api/projects?match={}", crate::features::nested::project::url_escape(&name));
        http::request(&a, "GET", &path, None).ok().and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok()).and_then(|v| v["localPath"].as_str().map(str::to_string))
    });
}

pub fn close_from_app(s: &mut AppState) {
    s.features.imports.from_app = None;
}

fn claude_agent(s: &AppState) -> bool {
    s.prefs.agent_choice == crate::app::prefs::AgentChoice::Claude
}

pub fn submit_from_app(s: &mut AppState) {
    let Some(f) = s.features.imports.from_app.as_ref() else { return };
    let (element, source) = (f.element.text.trim().to_string(), f.source.text.trim().to_string());
    if element.is_empty() || source.is_empty() || !claude_agent(s) {
        return;
    }
    let space = s.route.space.clone();
    let label = format!("Import · {element}");
    let mut request = VariationRequest { instruction: String::new(), count: 1, focus: Some(element.clone()), scope: Some("component".into()), agent: Some("claude".into()), space_id: Some(space.clone()), ..Default::default() };
    let mut from = json!({"element": element, "source": source});
    if let Some((_, url)) = f.screenshot.as_ref() {
        from["screenshot"] = json!(url);
    }
    request.extra.insert("importFrom".into(), from);
    launch::launch(s, Launch {
        space_id: space,
        parent_id: None,
        kind: DesignKind::Variation,
        origin: None,
        label: label.clone(),
        request,
        lane: None,
    });
    notify(s, &format!("Importing {element}…"), "The agent recreates it from the app's source.");
    s.features.imports.from_app = None;
}

pub fn poll(s: &mut AppState) -> bool {
    let mut changed = false;
    if let Some(list) = s.features.imports.list_req.take() {
        s.features.imports.bundles = Some(list);
        changed = true;
    }
    if let Some(r) = s.features.imports.file_req.take() {
        s.features.imports.busy = false;
        match r {
            Ok(data) => place_bundle(s, data),
            Err(e) => notify(s, &e, ""),
        }
        changed = true;
    }
    if let Some(r) = s.features.imports.shot_req.take() {
        match r {
            Ok(shot) => {
                if let Some(f) = &mut s.features.imports.from_app {
                    f.screenshot = Some(shot);
                }
            }
            Err(e) => notify(s, &e, ""),
        }
        changed = true;
    }
    if let Some(lp) = s.features.imports.projects_req.take()
        && let (Some(f), Some(lp)) = (&mut s.features.imports.from_app, lp)
        && !f.source_touched
    {
        f.source = TextEdit::new(lp);
        changed = true;
    }
    changed
}

const TITLE: TextStyle = TextStyle::new(16.0, 600.0);
const BODY: TextStyle = TextStyle::new(13.0, 400.0);
const LABEL: TextStyle = TextStyle::new(12.5, 500.0);
const MONO: TextStyle = TextStyle::new(12.0, 400.0);

fn fa(s: &mut AppState) -> &mut FromApp {
    s.features.imports.from_app.as_mut().expect("dialog open")
}

pub fn from_app_view(s: &AppState, p: &Palette) -> Option<El> {
    let f = s.features.imports.from_app.as_ref()?;
    let claude = claude_agent(s);
    let ok = claude && !f.element.text.trim().is_empty() && !f.source.text.trim().is_empty();
    let fieldbox = |key: &'static str, label: &'static str, input: El| col().w_full().gap(6.0).child(text(label).font(LABEL).color(p.fg))
        .child(row().key(key).w_full().min_h(36.0).px(10.0).radius(8.0).border(1.0, p.border).center_y().child(input));
    let element = text_input(&f.element, BODY).color(p.fg).key("import-element").label("Element").placeholder("e.g. Toolbar", p.muted_fg).single_line().w_full()
        .on_edit(|s: &mut AppState| &mut fa(s).element)
        .on_input_paste(|s: &mut AppState, c, _| paste_screenshot(s, c));
    let source = text_input(&f.source, MONO).color(p.fg).key("import-source").label("Source").placeholder("Repo path or URL", p.muted_fg).single_line().w_full()
        .on_edit(|s: &mut AppState| &mut fa(s).source)
        .on_input_change(|s: &mut AppState, _| fa(s).source_touched = true);
    let mut card = col().key("import-dialog").role("dialog").label("Import from app").w(460.0).gap(14.0).padding(20.0).radius(16.0)
        .child(col().gap(4.0).child(text("Import from app").font(TITLE).color(p.fg))
            .child(text("Recreate an element that already exists in the app, as a card to improve on.").font(BODY).color(p.muted_fg).wrap()))
        .child(fieldbox("import-element-box", "Element", element))
        .child(fieldbox("import-source-box", "Source", source));
    let shot = match &f.screenshot {
        Some((name, _)) => row().key("import-shot").gap(6.0).h(28.0).px(8.0).radius(6.0).border(1.0, p.border).center_y()
            .child(text(name.clone()).font(MONO).color(p.fg).truncate())
            .child(row().key("import-shot-x").role("button").label("Remove screenshot").size(20.0, 20.0).center()
                .on_press(|s: &mut AppState| fa(s).screenshot = None).child(crate::features::icons::x(10.0, p.muted_fg))),
        None => row().key("import-shot-pick").role("button").label("Screenshot (optional)").h(28.0).px(10.0).radius(6.0).center_y()
            .border(1.0, p.border).hover(p.hover).on_press_tx(pick_screenshot)
            .child(text("Screenshot (optional) · or paste one").font(MONO).color(p.fg)),
    };
    card = card.child(shot);
    if !claude {
        card = card.child(text("Needs the Claude agent: switch the inspector's agent to Claude.").font(BODY).color(p.muted_fg).wrap());
    }
    let go = row().key("import-go").role("button").label("Import").disabled(!ok).h(32.0).px(14.0).radius(8.0).center().fill(p.fg).opacity(if ok { 1.0 } else { 0.5 })
        .child(text("Import").font(LABEL).color(p.canvas_bg));
    card = card.child(row().w_full().justify_end().gap(8.0)
        .child(row().key("import-cancel").role("button").label("Cancel").h(32.0).px(14.0).radius(8.0).center().border(1.0, p.border).hover(p.hover)
            .on_press(close_from_app).child(text("Cancel").font(LABEL).color(p.fg)))
        .child(if ok { go.press(Color::BLACK.alpha(0.15)).on_press(submit_from_app) } else { go }));
    Some(el().key("import-modal").pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)).center().fill(Color::BLACK.alpha(0.45))
        .modal(true, close_from_app).child(style::glass(card, p, 0.96)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_the_standard() {
        assert_eq!(base64(b"Man"), "TWFu");
        assert_eq!(base64(b"Ma"), "TWE=");
        assert_eq!(base64(b"M"), "TQ==");
        assert_eq!(mime_for("a.JPG"), Some("image/jpeg"));
        assert_eq!(mime_for("a.bmp"), None);
    }

    #[test]
    fn bundles_place_by_group_and_link_parents() {
        let mut s = AppState::new();
        s.use_store(crate::features::scenes::fixture());
        s.enter_space("zzproj", None);
        let data: BundleFile = serde_json::from_value(json!({"name": "Usage", "designs": [
            {"key": "a", "group": "g1", "title": "A", "html": "x"},
            {"key": "b", "parentKey": "a", "group": "g1", "title": "B", "html": "x", "source": "app"},
            {"group": "g2", "title": "C", "html": "x", "kind": "variation"}
        ]})).unwrap();
        place_bundle(&mut s, data);
        let space = s.route.space.clone();
        let snap = s.snap();
        assert_eq!(snap.space(&space).unwrap().name, "Usage");
        let mut ds: Vec<_> = snap.designs_in(&space).collect();
        ds.sort_by(|a, b| a.title.cmp(&b.title));
        assert_eq!((ds[1].position.x, ds[1].position.y), (NODE_W + 160.0, 0.0));
        assert_eq!(ds[1].parent_id.as_deref(), Some(ds[0].id.as_str()));
        assert_eq!(ds[1].instruction.as_deref(), Some("Imported from app"));
        assert_eq!((ds[2].position.y, ds[2].kind), (620.0, DesignKind::Variation));
    }
}
