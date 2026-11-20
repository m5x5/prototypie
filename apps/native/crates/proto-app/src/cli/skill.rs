//! `prototypie skill install|show`: the agent skill embedded in the binary (spec-public/05 AN-CLI-228, -301,
//! -505). One source: `skills/prototypie/SKILL.md` at the workspace root.

use std::path::{Path, PathBuf};

use serde_json::json;

use super::Ctx;
use super::out::{self, Fail};

/// The skill text, exactly as in the repository.
pub const SKILL: &str = include_str!("../../../../skills/prototypie/SKILL.md");

pub fn run(ctx: &Ctx, command: &str) -> Result<i32, Fail> {
    match command {
        "show" => {
            out::print(SKILL);
            Ok(0)
        }
        "install" => install(ctx),
        _ => unreachable!("checked by the dispatcher"),
    }
}

fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."))
}

/// Where the skill goes without `--dir`: `~/.agents/skills`, and `~/.claude/skills` / `~/.codex/skills` when
/// those folders exist.
pub fn default_targets() -> Vec<PathBuf> {
    let h = home();
    let mut out = vec![h.join(".agents/skills")];
    for tool in [".claude", ".codex"] {
        if h.join(tool).is_dir() {
            out.push(h.join(tool).join("skills"));
        }
    }
    out
}

/// `~/…` for paths under the home folder.
pub fn tilde(p: &Path) -> String {
    match p.strip_prefix(home()) {
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => p.display().to_string(),
    }
}

/// Whether a SKILL.md is ours: its front matter says `name: prototypie`.
fn is_ours(text: &str) -> bool {
    let mut lines = text.lines();
    if lines.next().map(str::trim) != Some("---") {
        return false;
    }
    lines.take_while(|l| l.trim() != "---").any(|l| l.trim() == "name: prototypie")
}

/// `installed`, `outdated` or `missing`, and the copies found (for `status`).
pub fn state() -> (&'static str, Vec<PathBuf>) {
    let found: Vec<PathBuf> = default_targets().into_iter().map(|t| t.join("prototypie/SKILL.md")).filter(|p| p.is_file()).collect();
    if found.is_empty() {
        return ("missing", found);
    }
    let same = found.iter().all(|p| std::fs::read_to_string(p).is_ok_and(|t| t == SKILL));
    (if same { "installed" } else { "outdated" }, found)
}

fn is_link(p: &Path) -> bool {
    std::fs::symlink_metadata(p).is_ok_and(|m| m.file_type().is_symlink())
}

/// Write `<target>/prototypie/SKILL.md` atomically; `Err(reason)` when it must not be written.
fn write_one(target: &Path, force: bool) -> Result<PathBuf, String> {
    let folder = target.join("prototypie");
    let file = folder.join("SKILL.md");
    if is_link(&folder) {
        return Err(format!("{} is a symbolic link", folder.display()));
    }
    if is_link(&file) {
        return Err(format!("{} is a symbolic link", file.display()));
    }
    if file.exists() {
        let text = std::fs::read_to_string(&file).unwrap_or_default();
        if !force && !is_ours(&text) {
            return Err("another SKILL.md is there (its name is not prototypie; --force replaces it)".into());
        }
    }
    std::fs::create_dir_all(&folder).map_err(|e| format!("can't create {}: {e}", folder.display()))?;
    let tmp = folder.join(format!(".SKILL.md.tmp-{}", std::process::id()));
    let written = std::fs::write(&tmp, SKILL).and_then(|_| std::fs::rename(&tmp, &file));
    if let Err(e) = written {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("can't write {}: {e}", file.display()));
    }
    Ok(file)
}

/// The folders of `PATH`.
fn path_dirs() -> Vec<PathBuf> {
    std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default()
}

fn on_path(name: &str) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path_dirs().iter().any(|d| std::fs::metadata(d.join(name)).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0))
}

fn install(ctx: &Ctx) -> Result<i32, Fail> {
    let dirs = ctx.line.values("dir");
    let targets: Vec<PathBuf> = if dirs.is_empty() { default_targets() } else { dirs.iter().map(PathBuf::from).collect() };
    let force = ctx.line.has("force");
    let mut installed = Vec::new();
    let mut skipped = Vec::new();
    for t in &targets {
        match write_one(t, force) {
            Ok(p) => installed.push(p),
            Err(reason) => skipped.push((t.join("prototypie/SKILL.md"), reason)),
        }
    }
    // Make the command reachable: a link in ~/.local/bin when `prototypie` is not on PATH.
    let exe = std::env::current_exe().ok().and_then(|e| e.canonicalize().ok());
    let bin = home().join(".local/bin");
    let link = bin.join("prototypie");
    let mut linked = None;
    if !ctx.line.has("no-link")
        && !on_path("prototypie")
        && let Some(exe) = &exe
        && std::fs::symlink_metadata(&link).is_err()
        && std::fs::create_dir_all(&bin).is_ok()
        && std::os::unix::fs::symlink(exe, &link).is_ok()
    {
        linked = Some(link.clone());
    }
    let code = if installed.is_empty() { 1 } else { 0 };
    if ctx.json {
        out::print_json(&json!({
            "installed": installed.iter().map(|p| p.display().to_string()).collect::<Vec<_>>(),
            "skipped": skipped.iter().map(|(p, r)| json!({ "path": p.display().to_string(), "reason": r })).collect::<Vec<_>>(),
            "linked": linked.as_ref().map(|l| l.display().to_string()),
        }));
        return Ok(code);
    }
    for (p, reason) in &skipped {
        Fail::new(1, "skipped", format!("Skipped {}: {reason}.", p.display())).report(false);
    }
    if installed.is_empty() {
        return Ok(1);
    }
    let mut text = String::from("Installed the Prototypie skill:\n");
    for p in &installed {
        text.push_str(&format!("  {}\n", p.display()));
    }
    if let (Some(l), Some(exe)) = (&linked, &exe) {
        text.push_str(&format!("Linked {} -> {}\n", l.display(), exe.display()));
        if !path_dirs().iter().any(|d| d == &bin) {
            text.push_str(&format!("Note: ~/.local/bin is not on PATH; call {} directly or add it.\n", exe.display()));
        }
    }
    text.push_str(&format!("Read the skill now: {}\n", installed[0].display()));
    out::print(&text);
    Ok(code)
}
