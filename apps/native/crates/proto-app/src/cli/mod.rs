//! `prototypie notes|queue|cards|status|skill|help`: the app binary as a command for people and coding agents.
//! It talks to the running app's server (never starts the app, opens no window, takes no lock, opens no store)
//! with a small blocking HTTP client. Spec: spec-public/05 §2 (AN-CLI-2xx).

mod args;
mod cards;
mod help;
mod http;
mod notes;
mod out;
mod queue;
mod skill;
mod status;
#[cfg(test)]
mod tests;

use std::io::Read;
use std::time::Duration;

use serde_json::Value;

use args::Line;
use http::{Gone, Response, Server};
use out::Fail;

/// The CLI's version line.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

const GROUPS: &[&str] = &["notes", "queue", "cards", "status", "skill"];

/// Whether these arguments are for the CLI rather than the app (AN-CLI-201).
pub fn is_cli(first: Option<&str>) -> bool {
    let Some(a) = first else { return false };
    GROUPS.contains(&a)
        || matches!(a, "help" | "--help" | "-h" | "--version" | "-V" | "--json")
        || ["--server", "--as"].iter().any(|o| a == *o || a.starts_with(&format!("{o}=")))
}

/// First thing in `main`: run the CLI and return its exit code when the arguments are for it.
pub fn run() -> Option<i32> {
    let args: Vec<String> = std::env::args_os().skip(1).map(|a| a.to_string_lossy().into_owned()).collect();
    if !is_cli(args.first().map(String::as_str)) {
        return None;
    }
    Some(main_with(&args))
}

/// Options each command takes (besides the global ones).
fn allowed(group: &str, command: &str) -> Option<&'static [&'static str]> {
    Some(match (group, command) {
        ("notes", "list") => &["status", "all", "location", "prefix", "collection", "tag", "limit"],
        ("notes", "watch") => &["after", "timeout", "batch", "location", "prefix", "collection", "tag", "ack", "follow", "replies"],
        ("notes", "show") => &[],
        ("notes", "ack") => &["message", "force"],
        ("notes", "reply") => &["file", "role"],
        ("notes", "resolve") => &["summary", "summary-file"],
        ("notes", "dismiss") => &["reason", "reason-file"],
        ("notes", "collections") => &["location", "state"],
        ("queue", "list") | ("queue", "claim") => &[],
        ("queue", "show") => &["base-html"],
        ("queue", "deliver") => &["title", "html", "rationale", "viewport", "viewport-height"],
        ("cards", "list") => &["space"],
        ("cards", "create") => &[
            "title", "html", "space", "near", "rationale", "kind", "parent", "component", "viewport", "viewport-height", "from",
            "new-exploration", "parent-space", "seed",
        ],
        ("status", "") => &[],
        ("skill", "install") => &["dir", "force", "no-link"],
        ("skill", "show") => &[],
        _ => return None,
    })
}

/// Run one command line; the exit code.
pub fn main_with(argv: &[String]) -> i32 {
    let json = argv.iter().take_while(|a| *a != "--").any(|a| a == "--json");
    match dispatch(argv) {
        Ok(code) => code,
        Err(f) => {
            f.report(json);
            f.exit
        }
    }
}

fn dispatch(argv: &[String]) -> Result<i32, Fail> {
    let line = args::parse(argv)?;
    let words: Vec<&str> = line.words.iter().map(String::as_str).collect();
    if line.has("version") && words.is_empty() {
        out::println(&format!("prototypie {VERSION}"));
        return Ok(0);
    }
    match words.first().copied() {
        None => {
            out::print(help::MAIN);
            return Ok(0);
        }
        Some("help") => return help::show(&words[1..]),
        _ => {}
    }
    let group = words[0];
    if !GROUPS.contains(&group) {
        return Err(Fail::usage(format!("Unknown command \"{group}\". Run \"prototypie help\" for the commands.")));
    }
    let command = if group == "status" { "" } else { words.get(1).copied().unwrap_or("") };
    if command.is_empty() && group != "status" {
        out::print(help::group(group).unwrap_or(help::MAIN));
        return Ok(0);
    }
    let Some(allowed) = allowed(group, command) else {
        if line.has("help") {
            out::print(help::group(group).unwrap_or(help::MAIN));
            return Ok(0);
        }
        return Err(Fail::usage(format!("Unknown command \"{group} {command}\". Run \"prototypie help {group}\" for the commands.")));
    };
    if line.has("help") {
        return help::show(&[group, command]);
    }
    let full = if command.is_empty() { group.to_string() } else { format!("{group} {command}") };
    for (name, _) in &line.opts {
        if !args::GLOBAL.contains(&name.as_str()) && !allowed.contains(&name.as_str()) {
            return Err(Fail::usage(format!(
                "Unknown option \"{}\" for \"prototypie {full}\". Run \"prototypie {full} --help\".",
                args::shown(name)
            )));
        }
    }
    let first_arg = if group == "status" { 1 } else { 2 };
    let ctx = Ctx::new(line.clone(), full, first_arg, group == "skill")?;
    match (group, command) {
        ("notes", c) => notes::run(&ctx, c),
        ("queue", c) => queue::run(&ctx, c),
        ("cards", c) => cards::run(&ctx, c),
        ("status", _) => status::run(&ctx),
        ("skill", c) => skill::run(&ctx, c),
        _ => unreachable!("checked above"),
    }
}

/// Everything a command needs: the server, output mode, the agent's name, its arguments.
pub struct Ctx {
    pub server: Server,
    pub json: bool,
    /// `--as`, else `PROTOTYPIE_AGENT`, else `agent`.
    pub name: String,
    /// The name was set on purpose (not the default `agent`): agent texts get signed with it.
    pub signs: bool,
    /// The name from `--as` (repeated in the `Next:` command).
    pub as_option: Option<String>,
    pub line: Line,
    /// `group command`, for messages.
    pub full: String,
    /// Index of the first argument after the command in `line.words`.
    first_arg: usize,
}

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

impl Ctx {
    fn new(line: Line, full: String, first_arg: usize, local_only: bool) -> Result<Ctx, Fail> {
        let input = line.value("server").map(str::to_string).or_else(|| env("PROTOTYPIE_SERVER")).unwrap_or_else(|| http::DEFAULT_SERVER.into());
        let server = match Server::parse(&input) {
            Ok(s) => s,
            // `skill` works without the app: a bad address only matters to commands that use it.
            Err(_) if local_only => Server::parse(http::DEFAULT_SERVER).expect("default"),
            Err(f) => return Err(f),
        };
        let as_option = line.value("as").map(str::to_string);
        let given = as_option.clone().or_else(|| env("PROTOTYPIE_AGENT"));
        if let Some(n) = &given
            && !(1..=100).contains(&n.trim().chars().count())
        {
            return Err(Fail::usage("--as must be a name of 1 to 100 characters."));
        }
        let name = given.as_deref().map(str::trim).unwrap_or("agent").to_string();
        Ok(Ctx { signs: given.is_some() && name != "agent", json: line.has("json"), server, name, as_option, line, full, first_arg })
    }

    /// Arguments after the command.
    pub fn args(&self) -> &[String] {
        &self.line.words[self.first_arg.min(self.line.words.len())..]
    }

    pub fn needs(&self, what: &str) -> Fail {
        Fail::usage(format!("\"prototypie {}\" needs {what}. Run \"prototypie {} --help\".", self.full, self.full))
    }

    /// `<Name>: ` before an agent's text, unless it already starts with it (AN-CLI-217).
    pub fn sign(&self, text: &str) -> String {
        if !self.signs || text.starts_with(&format!("{}:", self.name)) {
            text.to_string()
        } else {
            format!("{}: {text}", self.name)
        }
    }

    /// One request; transport failures become the exit-3 messages.
    pub fn send(&self, method: &str, path: &str, body: Option<&Value>, limit: Duration) -> Result<Response, Gone> {
        let bytes = body.map(|b| serde_json::to_vec(b).unwrap_or_default());
        http::request(&self.server, method, path, bytes.as_deref(), limit)
    }

    pub fn call(&self, method: &str, path: &str, body: Option<&Value>, limit: Duration) -> Result<(u16, Value), Fail> {
        let r = self.send(method, path, body, limit).map_err(|g| http::unreachable(&self.server, &g))?;
        Ok((r.status, r.json()))
    }

    /// A request to the notes service: also "no notes service" (a 404 that isn't the service's) and "quitting".
    pub fn notes(&self, method: &str, path: &str, body: Option<&Value>) -> Result<(u16, Value), Fail> {
        let (status, v) = self.call(method, path, body, http::NORMAL)?;
        check_notes_answer(&self.server, status, &v)?;
        Ok((status, v))
    }

    /// Print the server's answer (`--json`) or the human text.
    pub fn show(&self, v: &Value, human: impl FnOnce() -> String) {
        if self.json { out::print_json(v) } else { out::print(&human()) }
    }
}

/// 404 without the service's `code` → no notes service; 503 → the app is quitting.
fn check_notes_answer(server: &Server, status: u16, v: &Value) -> Result<(), Fail> {
    if status == 404 && v["code"].as_str().is_none() {
        return Err(http::no_notes_service(server));
    }
    if status == 503 {
        return Err(Fail::new(3, "shutting_down", "Prototypie is quitting."));
    }
    Ok(())
}

/// The message of a refusal: the first validation issue (exit 2), or "The server refused the request" (exit 1).
pub fn refused(status: u16, v: &Value) -> Fail {
    let issues = v["issues"].as_array().or_else(|| v["error"].as_array());
    if status == 400
        && let Some(i) = issues.and_then(|l| l.first())
    {
        let path: Vec<String> = i["path"].as_array().map(|p| p.iter().map(|x| x.as_str().map(str::to_string).unwrap_or_else(|| x.to_string())).collect()).unwrap_or_default();
        let msg = i["message"].as_str().unwrap_or("invalid");
        return Fail::usage(if path.is_empty() { msg.to_string() } else { format!("{}: {msg}", path.join(".")) });
    }
    let error = v["error"].as_str().unwrap_or("no reason given");
    let code = v["code"].as_str().unwrap_or("refused");
    Fail::new(1, code, format!("The server refused the request ({status}): {}.", error.trim_end_matches('.')))
}

/// The server's sentence, ending with one period.
pub fn sentence(v: &Value) -> String {
    format!("{}.", v["error"].as_str().unwrap_or("Not found").trim_end_matches('.'))
}

pub const HTML_MAX: u64 = 32 * 1024 * 1024;
pub const TEXT_MAX: u64 = 80_000;

/// Read a named file or (`-`) stdin: regular files only, at most `max` bytes, UTF-8 (AN-CLI-233).
pub fn read_input(path: &str, max: u64, too_big: &str) -> Result<String, Fail> {
    let cant = |why: String| Fail::usage(format!("Can't read {path}: {why}."));
    let bytes = if path == "-" {
        let mut buf = Vec::new();
        std::io::stdin().lock().take(max + 1).read_to_end(&mut buf).map_err(|e| cant(e.to_string()))?;
        buf
    } else {
        let meta = std::fs::metadata(path).map_err(|e| cant(e.to_string()))?;
        if meta.is_dir() {
            return Err(cant("is a directory".into()));
        }
        if !meta.is_file() {
            return Err(cant("not a regular file".into()));
        }
        if meta.len() > max {
            return Err(Fail::usage(too_big));
        }
        let mut buf = Vec::new();
        std::fs::File::open(path).and_then(|f| f.take(max + 1).read_to_end(&mut buf)).map_err(|e| cant(e.to_string()))?;
        buf
    };
    if bytes.len() as u64 > max {
        return Err(Fail::usage(too_big));
    }
    String::from_utf8(bytes).map_err(|_| cant("not UTF-8 text".into()))
}

pub const TEXT_TOO_LONG: &str = "The text is longer than 20000 characters.";

/// An agent text from `--NAME TEXT` or `--NAME-file FILE|-`; `None` when neither is given.
pub fn text_option(ctx: &Ctx, name: &str) -> Result<Option<String>, Fail> {
    let file_opt = format!("{name}-file");
    let text = match (ctx.line.value(name), ctx.line.value(&file_opt)) {
        (Some(_), Some(_)) => return Err(Fail::usage(format!("Pass --{name} or --{file_opt}, not both."))),
        (Some(t), None) => t.to_string(),
        (None, Some(f)) => read_input(f, TEXT_MAX, TEXT_TOO_LONG)?,
        (None, None) => return Ok(None),
    };
    checked_text(ctx, &text).map(Some)
}

/// Not empty, signed, at most 20000 characters.
pub fn checked_text(ctx: &Ctx, text: &str) -> Result<String, Fail> {
    if text.trim().is_empty() {
        return Err(Fail::usage("The text is empty."));
    }
    let signed = ctx.sign(text);
    if signed.chars().count() > 20_000 {
        return Err(Fail::usage(TEXT_TOO_LONG));
    }
    Ok(signed)
}
