//! `prototypie cards …`: spaces and cards on the canvas (spec-public/05 AN-CLI-225, -226).

use serde_json::{Value, json};

use super::args;
use super::http;
use super::out::{self, Fail};
use super::queue::html_option;
use super::{Ctx, read_input, refused, sentence};

pub fn run(ctx: &Ctx, command: &str) -> Result<i32, Fail> {
    match command {
        "list" => list(ctx),
        "create" => create(ctx),
        _ => unreachable!("checked by the dispatcher"),
    }
}

fn list(ctx: &Ctx) -> Result<i32, Fail> {
    let (code, v) = ctx.call("GET", "/api/agent/canvas", None, http::NORMAL)?;
    if code != 200 {
        return Err(refused(code, &v));
    }
    match ctx.line.value("space") {
        None => ctx.show(&v, || spaces_text(&v)),
        Some(id) => {
            let space = v["spaces"].as_array().and_then(|l| l.iter().find(|s| s["id"].as_str() == Some(id))).cloned();
            let Some(space) = space else { return Err(Fail::not_found(format!("Unknown space {}.", out::safe_line(id)))) };
            ctx.show(&space, || cards_text(&space));
        }
    }
    Ok(0)
}

fn s<'a>(v: &'a Value, k: &str) -> &'a str {
    v[k].as_str().unwrap_or("")
}

/// `Main canvas › Shop › Checkout` from the parent chain.
fn path_of(spaces: &[Value], space: &Value) -> String {
    let mut names = vec![s(space, "name").to_string()];
    let mut cur = space.clone();
    for _ in 0..64 {
        let Some(parent) = cur["parentSpaceId"].as_str() else { break };
        let Some(p) = spaces.iter().find(|x| x["id"].as_str() == Some(parent)) else { break };
        names.push(s(p, "name").to_string());
        cur = p.clone();
    }
    names.reverse();
    names.join(" › ")
}

/// One line per space, the main canvas first.
pub fn spaces_text(v: &Value) -> String {
    let spaces = v["spaces"].as_array().cloned().unwrap_or_default();
    let mut out = String::new();
    for sp in &spaces {
        let n = sp["cards"].as_array().map_or(0, Vec::len);
        let kind = sp["kind"].as_str().filter(|k| !k.is_empty()).unwrap_or("-");
        out.push_str(&format!(
            "{}  {}  {}  {}\n",
            out::safe_line(s(sp, "id")),
            out::safe_line(kind),
            out::plural(n, "card", "cards"),
            out::safe_line(&path_of(&spaces, sp))
        ));
    }
    out
}

pub fn cards_text(space: &Value) -> String {
    let cards = space["cards"].as_array().cloned().unwrap_or_default();
    if cards.is_empty() {
        return format!("No cards in space {}.\n", out::safe_line(s(space, "id")));
    }
    cards
        .iter()
        .map(|c| format!("{}  {}  {}  {}\n", out::safe_line(s(c, "id")), out::safe_line(s(c, "status")), out::safe_line(s(c, "kind")), out::safe_line(s(c, "title"))))
        .collect()
}

/// The single card the options describe.
fn card_from_options(ctx: &Ctx) -> Result<Value, Fail> {
    let Some(title) = ctx.line.value("title") else { return Err(ctx.needs("--title TEXT")) };
    if !(1..=200).contains(&title.trim().chars().count()) {
        return Err(Fail::usage("--title must be 1 to 200 characters."));
    }
    let rationale = ctx.line.value("rationale");
    if rationale.is_some_and(|r| r.chars().count() > 4000) {
        return Err(Fail::usage("--rationale must be at most 4000 characters."));
    }
    let kind = match ctx.line.value("kind") {
        None => None,
        Some(k @ ("page" | "variation" | "spoke")) => Some(k),
        Some(_) => return Err(Fail::usage("--kind must be page, variation or spoke.")),
    };
    let viewport = args::number_opt(&ctx.line, "viewport", 200, 4000)?;
    let viewport_height = args::number_opt(&ctx.line, "viewport-height", 200, 4000)?;
    let html = html_option(ctx)?;
    let mut card = json!({ "title": title, "html": html });
    if let Some(r) = rationale {
        card["rationale"] = json!(r);
    }
    if let Some(k) = kind {
        card["kind"] = json!(k);
    }
    if let Some(p) = ctx.line.value("parent") {
        card["parentId"] = json!(p);
    }
    if ctx.line.has("component") {
        card["component"] = json!(true);
    }
    if let Some(v) = viewport {
        card["viewport"] = json!(v);
    }
    if let Some(v) = viewport_height {
        card["viewportHeight"] = json!(v);
    }
    Ok(card)
}

fn ids(v: &Value, key: &str) -> Vec<String> {
    v[key].as_array().map(|l| l.iter().filter_map(|x| x.as_str().map(out::safe_line)).collect()).unwrap_or_default()
}

fn create(ctx: &Ctx) -> Result<i32, Fail> {
    if !ctx.args().is_empty() {
        return Err(Fail::usage("\"prototypie cards create\" takes no arguments. Run \"prototypie cards create --help\"."));
    }
    let from = match ctx.line.value("from") {
        Some(f) => {
            if ctx.line.has("title") || ctx.line.has("html") {
                return Err(Fail::usage("With --from, the cards come from the file; leave out --title and --html."));
            }
            let text = read_input(f, super::HTML_MAX, "The file is larger than 32 MiB.")?;
            let v: Value = serde_json::from_str(&text).map_err(|e| Fail::usage(format!("Can't read {f}: not a JSON object ({e}).")))?;
            if !v.is_object() {
                return Err(Fail::usage(format!("Can't read {f}: not a JSON object.")));
            }
            Some(v)
        }
        None => None,
    };
    if let Some(name) = ctx.line.value("new-exploration") {
        let mut body = json!({ "name": name });
        if let Some(p) = ctx.line.value("parent-space").or(ctx.line.value("space")) {
            body["parentSpaceId"] = json!(p);
        }
        if let Some(seed) = ctx.line.value("seed") {
            body["seedId"] = json!(seed);
        }
        body["cards"] = match &from {
            Some(f) => f["cards"].clone(),
            None if ctx.line.has("title") || ctx.line.has("html") => json!([card_from_options(ctx)?]),
            None => json!([]),
        };
        let v = post(ctx, "/api/agent/explorations", &body)?;
        let cards = ids(&v, "cardIds");
        ctx.show(&v, || {
            format!("Created exploration {} with {}{}\n", out::safe_line(s(&v, "spaceId")), out::plural(cards.len(), "card", "cards"), list_suffix(&cards))
        });
        return Ok(0);
    }
    let mut body = match from {
        Some(f) => f,
        None => {
            let space = (ctx.line.has("space") || ctx.line.has("near")).then_some(());
            if space.is_none() {
                return Err(Fail::usage("Pass --space ID or --near ID (prototypie cards list shows the spaces)."));
            }
            json!({ "cards": [card_from_options(ctx)?] })
        }
    };
    if body.get("spaceId").is_none_or(Value::is_null)
        && let Some(sp) = ctx.line.value("space")
    {
        body["spaceId"] = json!(sp);
    }
    if body.get("nearId").is_none_or(Value::is_null)
        && let Some(n) = ctx.line.value("near")
    {
        body["nearId"] = json!(n);
    }
    if body.get("spaceId").is_none_or(Value::is_null) && body.get("nearId").is_none_or(Value::is_null) {
        return Err(Fail::usage("Pass --space ID or --near ID (prototypie cards list shows the spaces)."));
    }
    let v = post(ctx, "/api/agent/cards", &body)?;
    let created = ids(&v, "ids");
    ctx.show(&v, || format!("Created {} in space {}{}\n", out::plural(created.len(), "card", "cards"), out::safe_line(s(&v, "spaceId")), list_suffix(&created)));
    Ok(0)
}

fn list_suffix(ids: &[String]) -> String {
    if ids.is_empty() { String::new() } else { format!(": {}", ids.join(", ")) }
}

fn post(ctx: &Ctx, path: &str, body: &Value) -> Result<Value, Fail> {
    let (code, v) = ctx.call("POST", path, Some(body), http::LONG)?;
    match code {
        200 | 201 => Ok(v),
        400 if v["issues"].is_array() || v["error"].is_array() => Err(refused(code, &v)),
        400 => Err(Fail::usage(sentence(&v))),
        404 => Err(Fail::not_found(sentence(&v))),
        _ => Err(refused(code, &v)),
    }
}
