//! `prototypie queue …`: design jobs from the agent queue (spec-public/05 AN-CLI-220…224).

use serde_json::{Map, Value, json};

use super::args::{self, JobRef};
use super::http::{self, enc};
use super::out::{self, Fail};
use super::{Ctx, HTML_MAX, read_input, refused};

pub fn run(ctx: &Ctx, command: &str) -> Result<i32, Fail> {
    match command {
        "list" => list(ctx),
        "show" => show(ctx),
        "claim" => claim(ctx),
        "deliver" => deliver(ctx),
        _ => unreachable!("checked by the dispatcher"),
    }
}

fn job_ref(ctx: &Ctx) -> Result<JobRef, Fail> {
    match ctx.args() {
        [r] => JobRef::parse(r),
        [] => Err(ctx.needs("a queue item (JOB#INDEX)")),
        _ => Err(Fail::usage(format!("\"prototypie {}\" takes one queue item. Run \"prototypie {} --help\".", ctx.full, ctx.full))),
    }
}

fn not_waiting(r: &JobRef) -> Fail {
    Fail::not_found(format!("No queue item {} is waiting (done, cancelled or expired).", r.shown()))
}

fn items(ctx: &Ctx) -> Result<Value, Fail> {
    let (code, v) = ctx.call("GET", "/api/agent-queue", None, http::NORMAL)?;
    if code != 200 {
        return Err(refused(code, &v));
    }
    Ok(v)
}

fn list(ctx: &Ctx) -> Result<i32, Fail> {
    let v = items(ctx)?;
    ctx.show(&v, || list_text(&v));
    Ok(0)
}

fn s<'a>(v: &'a Value, k: &str) -> &'a str {
    v[k].as_str().unwrap_or("")
}

pub fn list_text(v: &Value) -> String {
    let items = v["items"].as_array().cloned().unwrap_or_default();
    if items.is_empty() {
        return "No queue items are waiting.\n".into();
    }
    let mut out = format!("{} waiting\n", out::plural(items.len(), "queue item", "queue items"));
    for it in &items {
        let r = format!("{}#{}", s(it, "jobId"), it["index"].as_u64().unwrap_or(0));
        let who = match it["agent"].as_str() {
            Some(a) => format!("claimed by {}", out::safe_line(a)),
            None => "unclaimed".into(),
        };
        out.push_str(&format!("\n{}  {who}  {}\n", out::safe_line(&r), out::time(s(it, "createdAt"))));
        let ctx = &it["context"];
        let path: Vec<&str> = ctx["spacePath"].as_array().map(|p| p.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
        let space = if path.is_empty() { s(ctx, "spaceName").to_string() } else { path.join(" › ") };
        let mut facts = Vec::new();
        if !space.is_empty() {
            facts.push(format!("space: {}", out::safe_line(&space)));
        }
        if let Some(m) = it["mode"].as_str() {
            facts.push(format!("mode: {}", out::safe_line(m)));
        }
        if let Some(sc) = it["scope"].as_str() {
            facts.push(format!("scope: {}", out::safe_line(sc)));
        }
        if !facts.is_empty() {
            out.push_str(&format!("  {}\n", facts.join("   ")));
        }
        let title = s(&it["direction"], "title");
        if !title.is_empty() {
            out.push_str(&format!("  direction: {}\n", out::safe_line(title)));
        }
        let instruction = if s(it, "instruction").is_empty() { s(&it["direction"], "brief") } else { s(it, "instruction") };
        out.push_str(&out::shortened(instruction, "  ", &format!("prototypie queue show {}", out::safe_line(&r))));
    }
    out
}

fn find(v: &Value, r: &JobRef) -> Option<Value> {
    v["items"].as_array()?.iter().find(|it| it["jobId"].as_str() == Some(r.job.as_str()) && it["index"].as_u64() == Some(r.index)).cloned()
}

/// Big fields as their size (AN-CLI-222).
fn sized(text: &str) -> String {
    format!("{} characters", text.chars().count())
}

pub fn show_text(item: &Value, r: &JobRef) -> String {
    let Some(o) = item.as_object() else { return String::new() };
    let width = o.keys().map(|k| k.chars().count()).max().unwrap_or(0) + 2;
    let mut out = String::new();
    for (k, v) in o {
        let value = match (k.as_str(), v) {
            ("baseHtml", Value::String(h)) => format!("{} (prototypie queue show {} --base-html)", sized(h), r.shown()),
            ("bases" | "components", Value::Array(list)) => {
                let shown: Vec<Value> = list
                    .iter()
                    .map(|x| {
                        let mut x = x.clone();
                        if let Some(h) = x.get("html").and_then(Value::as_str).map(sized) {
                            x["html"] = json!(h);
                        }
                        x
                    })
                    .collect();
                Value::Array(shown).to_string()
            }
            ("guidelines", Value::Object(g)) => {
                let mut g: Map<String, Value> = g.clone();
                if let Some(t) = g.get("text").and_then(Value::as_str).map(sized) {
                    g.insert("text".into(), json!(t));
                }
                Value::Object(g).to_string()
            }
            (_, Value::String(text)) => text.clone(),
            (_, other) => other.to_string(),
        };
        out.push_str(&format!("{:<width$}{}\n", k, out::safe_line(&value)));
    }
    out
}

fn show(ctx: &Ctx) -> Result<i32, Fail> {
    let r = job_ref(ctx)?;
    let v = items(ctx)?;
    let item = find(&v, &r).ok_or_else(|| not_waiting(&r))?;
    if ctx.line.has("base-html") {
        let Some(html) = item["baseHtml"].as_str() else {
            return Err(Fail::not_found(format!("Queue item {} has no base HTML.", r.shown())));
        };
        out::print(html);
        return Ok(0);
    }
    ctx.show(&item, || show_text(&item, &r));
    Ok(0)
}

fn claim(ctx: &Ctx) -> Result<i32, Fail> {
    let r = job_ref(ctx)?;
    let path = format!("/api/agent-queue/{}/{}/claim", enc(&r.job), r.index);
    let (code, v) = ctx.call("POST", &path, Some(&json!({ "agent": ctx.name })), http::NORMAL)?;
    match code {
        200 => {}
        404 => return Err(not_waiting(&r)),
        409 => return Err(Fail::conflict(format!("Queue item {} is no longer pending (done or cancelled).", r.shown()))),
        _ => return Err(refused(code, &v)),
    }
    ctx.show(&v, || format!("Claimed {} as {}\n", r.shown(), out::safe_line(&ctx.name)));
    Ok(0)
}

/// `--html FILE|-`, unchanged except the emptiness check.
pub fn html_option(ctx: &Ctx) -> Result<String, Fail> {
    let Some(path) = ctx.line.value("html") else {
        return Err(Fail::usage("Pass the HTML with --html FILE (or --html - to read stdin)."));
    };
    let html = read_input(path, HTML_MAX, "The HTML is larger than 32 MiB.")?;
    if html.trim().is_empty() {
        return Err(Fail::usage("The HTML is empty."));
    }
    Ok(html)
}

fn deliver(ctx: &Ctx) -> Result<i32, Fail> {
    let r = job_ref(ctx)?;
    let Some(title) = ctx.line.value("title").map(str::to_string) else { return Err(ctx.needs("--title TEXT")) };
    let viewport = args::number_opt(&ctx.line, "viewport", 200, 4000)?;
    let viewport_height = args::number_opt(&ctx.line, "viewport-height", 200, 4000)?;
    let html = html_option(ctx)?;
    let mut body = json!({ "title": title, "rationale": ctx.line.value("rationale").unwrap_or(""), "html": html });
    if let Some(v) = viewport {
        body["viewport"] = json!(v);
    }
    if let Some(v) = viewport_height {
        body["viewportHeight"] = json!(v);
    }
    let path = format!("/api/agent-queue/{}/{}", enc(&r.job), r.index);
    let (code, v) = ctx.call("POST", &path, Some(&body), http::LONG)?;
    match code {
        200 => {}
        404 => return Err(not_waiting(&r)),
        409 => {
            return Err(Fail::conflict(format!(
                "The card for {} was deleted in the app, so nothing was placed. Create a card with \"prototypie cards create\" only if the design is still wanted.",
                r.shown()
            )));
        }
        _ => return Err(refused(code, &v)),
    }
    ctx.show(&v, || format!("Delivered {}: \"{}\"\n", r.shown(), out::safe_line(&title)));
    Ok(0)
}
