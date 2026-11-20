//! Where in the code a Rune key is set (spec 01 §9): a scan of the app crate's `.rs` files for `.key("…")`,
//! `.key(format!("…"))` and the Rune setter, built once per process on its own thread.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

/// One place a key is set.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    /// Workspace-relative file (`crates/proto-app/src/features/review/dock.rs`).
    pub file: String,
    /// 1-based.
    pub line: usize,
    pub kind: Kind,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    Exact(String),
    /// A format string with placeholders: the fixed text before the first `{` and after the last `}`.
    Pattern { prefix: String, suffix: String },
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Index {
    pub entries: Vec<Entry>,
}

/// The Rune setter's name (`El::set_key` in `vendor/rune-patched/crates/rune/src/el.rs`).
const SETTER: &str = "set_key(";

/// The string literal (or format string) a key call's argument starts with, from just after its `(`.
fn literal_arg(rest: &str) -> Option<&str> {
    let mut r = rest.trim_start_matches(' ');
    if let Some(after) = r.strip_prefix("format!(") {
        r = after.trim_start_matches(' ');
    }
    let body = r.strip_prefix('"')?;
    let end = body.find('"')?;
    Some(&body[..end])
}

fn kind_of(lit: &str) -> Kind {
    match (lit.find('{'), lit.rfind('}')) {
        (Some(open), Some(close)) => Kind::Pattern { prefix: lit[..open].to_string(), suffix: lit[close + 1..].to_string() },
        _ => Kind::Exact(lit.to_string()),
    }
}

/// The key entries of one file's text.
pub fn scan_text(file: &str, text: &str, out: &mut Vec<Entry>) {
    for (i, line) in text.lines().enumerate() {
        // Every call site in the line, in position order: `.key(` and anything ending in the setter's name.
        let mut hits: Vec<usize> = Vec::new();
        for pat in [".key(", SETTER] {
            let mut from = 0;
            while let Some(at) = line[from..].find(pat) {
                hits.push(from + at + pat.len());
                from += at + pat.len();
            }
        }
        hits.sort_unstable();
        for after in hits {
            if let Some(lit) = literal_arg(&line[after..]) {
                out.push(Entry { file: file.to_string(), line: i + 1, kind: kind_of(lit) });
            }
        }
    }
}

/// Test files: a stem ending in `_tests` (a file just named `tests.rs` counts as source).
fn is_test_file(p: &Path) -> bool {
    p.file_stem().and_then(|s| s.to_str()).is_some_and(|s| s.ends_with("_tests"))
}

fn walk(dir: &Path, root: &Path, out: &mut Vec<Entry>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut paths: Vec<PathBuf> = rd.filter_map(|e| e.ok().map(|e| e.path())).collect();
    paths.sort();
    for p in paths {
        if p.is_dir() {
            walk(&p, root, out);
        } else if p.extension().is_some_and(|e| e == "rs") && !is_test_file(&p) {
            let Ok(text) = std::fs::read_to_string(&p) else { continue };
            let rel = p.strip_prefix(root).unwrap_or(&p).to_string_lossy().replace('\\', "/");
            scan_text(&rel, &text, out);
        }
    }
}

/// Scan `src_dir`, storing paths relative to `root`. Missing sources give an empty index.
pub fn build(src_dir: &Path, root: &Path) -> Index {
    let mut entries = Vec::new();
    walk(src_dir, root, &mut entries);
    Index { entries }
}

/// Module path of a file (AN-UI-113): after the first `/src/` (up to a further one), no `.rs`, no `/mod`.
pub fn module_of(file: &str) -> String {
    let mut s = match file.find("/src/") {
        Some(at) => &file[at + 5..],
        None => file,
    };
    if let Some(at) = s.find("/src/") {
        s = &s[..at];
    }
    let s = s.strip_suffix(".rs").unwrap_or(s);
    let s = s.strip_suffix("/mod").unwrap_or(s);
    s.replace('/', "::")
}

impl Index {
    /// `(file:line, module)` for `key` (AN-UI-112).
    pub fn lookup(&self, key: &str) -> Option<(String, String)> {
        let found = self.entries.iter().find(|e| matches!(&e.kind, Kind::Exact(t) if t == key)).or_else(|| {
            let mut best: Option<(&Entry, usize)> = None;
            for e in &self.entries {
                let Kind::Pattern { prefix, suffix } = &e.kind else { continue };
                let fixed = prefix.len() + suffix.len();
                if fixed >= 2 && key.len() >= fixed && key.starts_with(prefix.as_str()) && key.ends_with(suffix.as_str())
                    && best.is_none_or(|(_, n)| fixed >= n)
                {
                    best = Some((e, fixed));
                }
            }
            best.map(|(e, _)| e)
        })?;
        Some((format!("{}:{}", found.file, found.line), module_of(&found.file)))
    }
}

// ---- the process-wide index ----

fn slot() -> &'static Mutex<Option<Arc<Index>>> {
    static SLOT: OnceLock<Mutex<Option<Arc<Index>>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}

/// Build the index in the background, once per process; `wake` runs when it's ready.
pub fn start(wake: Option<rune::Waker>) {
    static STARTED: OnceLock<()> = OnceLock::new();
    if STARTED.set(()).is_err() {
        return;
    }
    let crate_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let root = crate_dir.parent().and_then(Path::parent).map(Path::to_path_buf).unwrap_or_else(|| crate_dir.clone());
    let src = crate_dir.join("src");
    let _ = std::thread::Builder::new().name("annotate-source-index".into()).spawn(move || {
        let index = build(&src, &root);
        if let Ok(mut s) = slot().lock() {
            *s = Some(Arc::new(index));
        }
        if let Some(w) = wake {
            w();
        }
    });
}

/// The index once built (None until then).
pub fn ready() -> Option<Arc<Index>> {
    slot().lock().ok().and_then(|s| s.clone())
}

/// Look up a key in the built index; nothing before it's ready.
pub fn find(key: &str) -> Option<(String, String)> {
    ready()?.lookup(key)
}
