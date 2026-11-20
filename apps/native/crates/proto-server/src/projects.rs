//! The user's projects from a Turtle register, a Solid pod and a projects folder (reference `src/lib/server/projects.ts`): a tiny Turtle reader
//! for schema:WebSite subjects, local folder matching, and the README's first prose paragraph. Read-only.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Serialize;

/// Where the app finds its project sources. Every key is optional and nothing is assumed: the environment variable
/// named next to a key wins, then `config.json` in the data directory (`$PROTOTYPIE_DATA`, else
/// `~/Library/Application Support/Prototypie`; `$PROTOTYPIE_CONFIG` names another file).
///
/// ```json
/// { "projectsTtl": "~/projects.ttl", "projectsRoot": "~/code", "podProjects": "https://pod.example/me/projects/index.ttl" }
/// ```
#[derive(Default, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct Settings {
    /// `PROTOTYPIE_PROJECTS_TTL`: a Turtle file of schema:WebSite projects (name, repository, keywords).
    projects_ttl: Option<String>,
    /// `PROTOTYPIE_PROJECTS_ROOT`: a folder whose sub-folders are projects; those the register doesn't name are listed too.
    projects_root: Option<String>,
    /// `PROTOTYPIE_POD_PROJECTS`: a Solid pod's project register, read like the Turtle file (an empty value turns it off).
    pod_projects: Option<String>,
}

fn data_dir() -> PathBuf {
    std::env::var("PROTOTYPIE_DATA")
        .map(PathBuf::from)
        .unwrap_or_else(|_| dirs_home().join("Library/Application Support/Prototypie"))
}

fn settings() -> Settings {
    // Unit tests use their own files, never the user's configuration.
    if cfg!(test) {
        return Settings::default();
    }
    let path = std::env::var("PROTOTYPIE_CONFIG").map(PathBuf::from).unwrap_or_else(|_| data_dir().join("config.json"));
    std::fs::read_to_string(path).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

/// `~` and `~/…` at the start of a configured path.
fn expand(path: &str) -> PathBuf {
    match path.strip_prefix("~/") {
        Some(rest) => dirs_home().join(rest),
        None if path == "~" => dirs_home(),
        None => PathBuf::from(path),
    }
}

fn nonempty(v: String) -> Option<String> {
    (!v.trim().is_empty()).then_some(v)
}

/// The project register (Turtle). Without any setting it is `projects.ttl` in the data directory, which usually
/// doesn't exist: the "New project" dialog then offers to start a project from a name or a folder path.
pub fn projects_ttl() -> PathBuf {
    std::env::var("PROTOTYPIE_PROJECTS_TTL")
        .ok()
        .and_then(nonempty)
        .or_else(|| settings().projects_ttl.and_then(nonempty))
        .map(|p| expand(&p))
        .unwrap_or_else(|| data_dir().join("projects.ttl"))
}

/// The folder whose sub-folders are projects, when one is configured.
fn projects_root() -> Option<PathBuf> {
    std::env::var("PROTOTYPIE_PROJECTS_ROOT")
        .ok()
        .and_then(nonempty)
        .or_else(|| settings().projects_root.and_then(nonempty))
        .map(|p| expand(&p))
}

/// Where a repository's folder is looked for by its name: the configured root, else next to the register.
fn near(ttl: &Path) -> PathBuf {
    projects_root().unwrap_or_else(|| ttl.parent().unwrap_or(Path::new("/")).to_path_buf())
}

fn dirs_home() -> PathBuf {
    std::env::var("HOME").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("/"))
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct PodRecord {
    pub doc: String,
    pub subject: String,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repo: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub local_path: Option<String>,
    pub keywords: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pod: Option<PodRecord>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub readme: Option<String>,
    /// Where the entry comes from: "private-projects.ttl", "Folder" (a directory next to it) or "Solid pod".
    pub source: String,
}

#[derive(Debug, PartialEq)]
enum Tok {
    Iri(String),
    Pname(String),
    Lit(String),
    A,
    Punct(char),
}

impl Tok {
    fn text(&self) -> &str {
        match self {
            Tok::Iri(s) | Tok::Pname(s) | Tok::Lit(s) => s,
            Tok::A => "a",
            Tok::Punct(';') => ";",
            Tok::Punct(',') => ",",
            Tok::Punct(_) => ".",
        }
    }
}

fn word_char(c: Option<char>) -> bool {
    c.is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '-')
}

fn tokenize(src: &str) -> Vec<Tok> {
    let cs: Vec<char> = src.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < cs.len() {
        let c = cs[i];
        if c.is_whitespace() {
            i += 1;
        } else if c == '#' {
            while i < cs.len() && cs[i] != '\n' {
                i += 1;
            }
        } else if c == '<' {
            let end = cs[i..].iter().position(|&x| x == '>').map_or(cs.len(), |p| i + p);
            out.push(Tok::Iri(cs[i + 1..end].iter().collect()));
            i = end + 1;
        } else if c == '"' {
            let mut v = String::new();
            i += 1;
            while i < cs.len() && cs[i] != '"' {
                if cs[i] == '\\' && i + 1 < cs.len() {
                    v.push(match cs[i + 1] {
                        'n' => '\n',
                        't' => '\t',
                        e => e,
                    });
                    i += 2;
                } else {
                    v.push(cs[i]);
                    i += 1;
                }
            }
            i += 1;
            if cs.get(i) == Some(&'@') {
                while i < cs.len() && (word_char(cs.get(i).copied()) || cs[i] == '@') {
                    i += 1;
                }
            } else if cs.get(i) == Some(&'^') && cs.get(i + 1) == Some(&'^') {
                i += 2;
                while i < cs.len() && !(cs[i].is_whitespace() || ";,.".contains(cs[i])) {
                    i += 1;
                }
            }
            out.push(Tok::Lit(v));
        } else if c == ';' || c == ',' {
            out.push(Tok::Punct(c));
            i += 1;
        } else if c == '.' && !word_char(cs.get(i + 1).copied()) {
            out.push(Tok::Punct('.'));
            i += 1;
        } else {
            let mut w = String::new();
            while i < cs.len()
                && !(cs[i].is_whitespace() || ";,<\"".contains(cs[i]))
                && !(cs[i] == '.' && !word_char(cs.get(i + 1).copied()))
            {
                w.push(cs[i]);
                i += 1;
            }
            out.push(if w == "a" { Tok::A } else { Tok::Pname(w) });
        }
    }
    out
}

/// Subjects (in document order) with predicate → values. `@prefix` lines are skipped.
pub fn parse_turtle(src: &str) -> Vec<(String, BTreeMap<String, Vec<String>>)> {
    let t = tokenize(src);
    let mut subjects: Vec<(String, BTreeMap<String, Vec<String>>)> = Vec::new();
    let mut i = 0;
    while i < t.len() {
        if matches!(&t[i], Tok::Pname(w) if w.starts_with('@')) {
            while i < t.len() && t[i] != Tok::Punct('.') {
                i += 1;
            }
            i += 1;
            continue;
        }
        let subject = t[i].text().to_string();
        i += 1;
        let idx = match subjects.iter().position(|s| s.0 == subject) {
            Some(x) => x,
            None => {
                subjects.push((subject, BTreeMap::new()));
                subjects.len() - 1
            }
        };
        while i < t.len() && t[i] != Tok::Punct('.') {
            let pred = if t[i] == Tok::A { "rdf:type".to_string() } else { t[i].text().to_string() };
            i += 1;
            let mut values = subjects[idx].1.remove(&pred).unwrap_or_default();
            while i < t.len() && t[i] != Tok::Punct(';') && t[i] != Tok::Punct('.') {
                if t[i] != Tok::Punct(',') {
                    values.push(t[i].text().to_string());
                }
                i += 1;
            }
            subjects[idx].1.insert(pred, values);
            if t.get(i) == Some(&Tok::Punct(';')) {
                i += 1;
            }
        }
        i += 1;
    }
    subjects
}

fn is_dir(p: &Path) -> bool {
    std::fs::metadata(p).is_ok_and(|m| m.is_dir())
}

fn pod_record(subject: &str, props: &BTreeMap<String, Vec<String>>, proj_ns: Option<&str>) -> Option<PodRecord> {
    let producer = props.get("schema:producer")?.first()?;
    let ns = proj_ns?;
    let local = subject.strip_prefix("proj:")?;
    let u = reqwest::Url::parse(producer).ok()?;
    if !matches!(u.scheme(), "http" | "https") || !u.path().ends_with("/clients/index.ttl") {
        return None;
    }
    let mut doc = u.join("../projects/index.ttl").ok()?;
    doc.set_fragment(None);
    Some(PodRecord { doc: doc.to_string(), subject: format!("{ns}{local}") })
}

/// The Solid pod's project register, read like the Turtle register. Off unless `PROTOTYPIE_POD_PROJECTS` or
/// `podProjects` in `config.json` names it (an empty environment value turns a configured one off).
fn pod_index_url() -> Option<String> {
    match std::env::var("PROTOTYPIE_POD_PROJECTS") {
        Ok(url) => nonempty(url),
        Err(_) => settings().pod_projects.and_then(nonempty),
    }
}

static POD_CACHE: std::sync::Mutex<Option<(std::time::Instant, Vec<Project>)>> = std::sync::Mutex::new(None);
const POD_TTL: std::time::Duration = std::time::Duration::from_secs(600);

/// The pod's projects, fetched at most every ten minutes (an unreachable pod gives an empty list, or the last one
/// we had, for a minute: it must never slow the dialog down).
pub async fn pod_projects() -> Vec<Project> {
    let Some(url) = pod_index_url() else { return Vec::new() };
    let cached = POD_CACHE.lock().ok().and_then(|c| c.clone());
    if let Some((at, list)) = &cached
        && at.elapsed() < POD_TTL
    {
        return list.clone();
    }
    let fetched = async {
        let client = reqwest::Client::builder().timeout(std::time::Duration::from_secs(5)).build().ok()?;
        let res = client.get(&url).header("accept", "text/turtle").send().await.ok()?.error_for_status().ok()?;
        let src = res.text().await.ok()?;
        Some(parse_projects(&src, Path::new("/nonexistent"), "Solid pod"))
    }
    .await;
    let list = match (fetched, cached) {
        (Some(list), _) => list,
        (None, Some((_, old))) => old,
        (None, None) => Vec::new(),
    };
    // A failed fetch keeps what it has and retries in a minute.
    let at = std::time::Instant::now() - if list.is_empty() { POD_TTL - std::time::Duration::from_secs(60) } else { std::time::Duration::ZERO };
    if let Ok(mut c) = POD_CACHE.lock() {
        *c = Some((at, list.clone()));
    }
    list
}

/// All schema:WebSite projects, sorted by name; None when the file can't be read. Blocking (small file):
/// call from `spawn_blocking`.
pub fn list(ttl: &Path) -> Option<Vec<Project>> {
    let src = std::fs::read_to_string(ttl).ok()?;
    Some(parse_projects(&src, &near(ttl), "private-projects.ttl"))
}

/// The schema:WebSite subjects of a Turtle document, sorted by name; `near` is where a repository's folder is
/// looked for by name, `source` what the entries say they come from.
pub fn parse_projects(src: &str, near: &Path, source: &str) -> Vec<Project> {
    let proj_ns = regex::Regex::new(r"@prefix\s+proj:\s*<([^>]*)>").unwrap().captures(src).map(|c| c[1].to_string());
    let mut out = Vec::new();
    for (subject, props) in parse_turtle(src) {
        if !props.get("rdf:type").is_some_and(|t| t.iter().any(|x| x == "schema:WebSite")) {
            continue;
        }
        let id = subject.rsplit([':', '#', '/']).next().unwrap_or("").to_string();
        if id.is_empty() || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b)) {
            continue;
        }
        let name = props.get("schema:name").and_then(|v| v.first()).cloned().unwrap_or_else(|| id.clone());
        let repo = props.get("schema:codeRepository").and_then(|v| v.first()).cloned();
        let local_path = repo.as_deref().and_then(|r| {
            let rp = Path::new(r);
            if rp.is_absolute() && is_dir(rp) {
                return Some(r.to_string());
            }
            let base = rp.file_name()?;
            let cand = near.join(base);
            is_dir(&cand).then(|| cand.to_string_lossy().into_owned())
        });
        let mut keywords: Vec<String> = Vec::new();
        for k in props.get("schema:keywords").into_iter().flatten() {
            if !keywords.contains(k) {
                keywords.push(k.clone());
            }
        }
        let pod = pod_record(&subject, &props, proj_ns.as_deref());
        out.push(Project { id, name, repo, local_path, keywords, pod, readme: None, source: source.to_string() });
    }
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()).then(a.name.cmp(&b.name)));
    out
}

/// The registry plus its additional sources, one row per project, sorted by name: the entries of the Turtle register
/// (None when it can't be read), the Solid pod's (`pod`, already parsed) and the sub-folders of the configured projects
/// root that none of them names (a project that isn't registered yet). Blocking.
pub fn list_all(ttl: &Path, pod: &[Project]) -> Option<Vec<Project>> {
    let mut out = list(ttl)?;
    let seen = |out: &[Project], p: &Project| {
        out.iter().any(|o| o.id.eq_ignore_ascii_case(&p.id) || o.name.eq_ignore_ascii_case(&p.name)
            || (p.local_path.is_some() && o.local_path == p.local_path))
    };
    for p in pod {
        if !seen(&out, p) {
            out.push(p.clone());
        }
    }
    // Folders that no register names: only from an explicitly configured root.
    let root = projects_root();
    for e in root.iter().flat_map(std::fs::read_dir).flatten().flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if name.starts_with(['.', '_']) || name == "node_modules" || !is_dir(&e.path()) {
            continue;
        }
        if name.is_empty() || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b)) {
            continue;
        }
        let folder = Project {
            id: name.clone(),
            name,
            repo: None,
            local_path: Some(e.path().to_string_lossy().into_owned()),
            keywords: Vec::new(),
            pod: None,
            readme: None,
            source: "Folder".into(),
        };
        if !seen(&out, &folder) {
            out.push(folder);
        }
    }
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()).then(a.name.cmp(&b.name)));
    Some(out)
}

/// The local folder a space name stands for: a project whose name or id matches, else a folder of that exact
/// name next to the TTL.
pub fn match_local_path(ttl: &Path, name: &str) -> Option<String> {
    let key = name.trim().to_lowercase();
    if key.is_empty() {
        return None;
    }
    if let Some(hit) = list(ttl)?.into_iter().find(|p| p.local_path.is_some() && (p.name.to_lowercase() == key || p.id.to_lowercase() == key)) {
        return hit.local_path;
    }
    let base = name.trim();
    if base.contains('/') || base.starts_with('.') {
        return None;
    }
    let near = near(ttl).join(base);
    is_dir(&near).then(|| near.to_string_lossy().into_owned())
}

/// First prose paragraph of a README: no headings, badges, HTML, code or lists; at most ~400 characters.
pub fn first_paragraph(md: &str) -> Option<String> {
    let fence = regex::Regex::new(r"(?s)```.*?```").unwrap();
    let comment = regex::Regex::new(r"(?s)<!--.*?-->").unwrap();
    let skip = regex::Regex::new(r"^(#|!\[|\[!\[|<|[-*+] |\d+\. |>|\||---|===)").unwrap();
    let secret = regex::Regex::new(r"(?i)(token|secret|password|passwd|api[_-]?key|private[_-]?key|bearer|authorization)|[A-Za-z0-9_\-]{32,}|-----BEGIN").unwrap();
    let img = regex::Regex::new(r"!\[[^\]]*\]\([^)]*\)").unwrap();
    let link = regex::Regex::new(r"\[([^\]]+)\]\([^)]*\)").unwrap();
    let marks = regex::Regex::new(r"[*_`]+").unwrap();
    let ws = regex::Regex::new(r"\s+").unwrap();
    let blank = regex::Regex::new(r"\n\s*\n").unwrap();
    let cleaned = comment.replace_all(&fence.replace_all(md, ""), "").into_owned();
    for block in blank.split(&cleaned) {
        let lines: Vec<&str> = block.split('\n').map(str::trim).filter(|l| !l.is_empty() && !skip.is_match(l) && !secret.is_match(l)).collect();
        let text = lines.join(" ");
        let text = img.replace_all(&text, "");
        let text = link.replace_all(&text, "$1");
        let text = marks.replace_all(&text, "");
        let text = ws.replace_all(&text, " ").trim().to_string();
        if text.chars().count() >= 20 {
            if text.chars().count() > 400 {
                let cut: String = text.chars().take(397).collect();
                let cut = regex::Regex::new(r"\s+\S*$").unwrap().replace(&cut, "").into_owned();
                return Some(format!("{cut}…"));
            }
            return Some(text);
        }
    }
    None
}

/// One project with its README paragraph (blocking).
pub fn get(ttl: &Path, id: &str, pod: &[Project]) -> Option<Project> {
    let mut project = list_all(ttl, pod)?.into_iter().find(|p| p.id == id)?;
    let Some(dir) = project.local_path.clone() else { return Some(project) };
    let readme = std::fs::read_dir(&dir).ok().and_then(|rd| {
        rd.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).find(|n| {
            let l = n.to_lowercase();
            l == "readme" || l == "readme.md" || l == "readme.markdown" || l == "readme.txt"
        })
    });
    if let Some(name) = readme
        && let Ok(bytes) = std::fs::read(Path::new(&dir).join(name))
    {
        let txt = String::from_utf8_lossy(&bytes[..bytes.len().min(20_000)]).into_owned();
        project.readme = first_paragraph(&txt);
    }
    Some(project)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TTL: &str = r#"@prefix schema: <http://schema.org/> .
@prefix proj: <urn:example:project:> .
# comment
proj:abc a schema:WebSite ;
  schema:name "Alpha \"A\""@en ;
  schema:codeRepository "/nonexistent/alpha" ;
  schema:keywords "x", "y", "x" ;
  schema:producer <https://pod.example/u/clients/index.ttl#me> .
proj:b-2 a schema:WebSite ; schema:name "beta" .
proj:other a schema:Thing ; schema:name "no" .
"#;

    #[test]
    fn parses_websites() {
        let dir = tempfile::tempdir().unwrap();
        let ttl = dir.path().join("p.ttl");
        std::fs::write(&ttl, TTL).unwrap();
        std::fs::create_dir(dir.path().join("alpha")).unwrap();
        let list = list(&ttl).unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].id, "abc");
        assert_eq!(list[0].name, "Alpha \"A\"");
        assert_eq!(list[0].keywords, ["x", "y"]);
        assert_eq!(list[0].local_path.as_deref(), Some(dir.path().join("alpha").to_str().unwrap()));
        assert_eq!(list[0].pod, Some(PodRecord { doc: "https://pod.example/u/projects/index.ttl".into(), subject: "urn:example:project:abc".into() }));
        assert_eq!(list[1].id, "b-2");
        assert_eq!(match_local_path(&ttl, "ALPHA \"a\""), list[0].local_path);
        assert_eq!(match_local_path(&ttl, "../x"), None);
    }

    #[test]
    fn readme_paragraph() {
        let md = "# Title\n\n[![badge](x)](y)\n\n```\ncode\n```\n\nThis app helps [people](http://x) plan *their* week quickly.\n\nMore.";
        assert_eq!(first_paragraph(md).as_deref(), Some("This app helps people plan their week quickly."));
        assert_eq!(first_paragraph("api_key = 12345678901234567890\n"), None);
    }
}
