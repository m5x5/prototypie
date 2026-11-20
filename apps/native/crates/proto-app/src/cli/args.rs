//! Command-line parsing without a dependency: groups, commands, options anywhere, `--` ends options, durations
//! and queue item references (spec-public/05 AN-CLI-203, -211, -220).

use super::out::Fail;

/// Options that take a value (in every command that knows them).
const VALUED: &[&str] = &[
    "server", "as", "status", "location", "prefix", "collection", "tag", "limit", "after", "timeout", "batch", "message",
    "file", "role", "summary", "summary-file", "reason", "reason-file", "state", "title", "html", "rationale", "viewport",
    "viewport-height", "space", "near", "kind", "parent", "from", "new-exploration", "parent-space", "seed", "dir",
];

/// Options every command takes.
pub const GLOBAL: &[&str] = &["server", "as", "json", "help", "version"];

/// One parsed command line.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Line {
    /// Words that are not options, in order (group, command, arguments).
    pub words: Vec<String>,
    /// `(name, value)` in order; flags have no value.
    pub opts: Vec<(String, Option<String>)>,
}

impl Line {
    pub fn has(&self, name: &str) -> bool {
        self.opts.iter().any(|(n, _)| n == name)
    }

    /// The last value given for `name`.
    pub fn value(&self, name: &str) -> Option<&str> {
        self.opts.iter().rev().find(|(n, _)| n == name).and_then(|(_, v)| v.as_deref())
    }

    pub fn values(&self, name: &str) -> Vec<&str> {
        self.opts.iter().filter(|(n, _)| n == name).filter_map(|(_, v)| v.as_deref()).collect()
    }

    pub fn word(&self, i: usize) -> Option<&str> {
        self.words.get(i).map(String::as_str)
    }
}

/// Split `args` (without the program name) into words and options.
pub fn parse(args: &[String]) -> Result<Line, Fail> {
    let mut line = Line::default();
    let mut i = 0;
    let mut rest_are_words = false;
    while i < args.len() {
        let a = &args[i];
        i += 1;
        if rest_are_words {
            line.words.push(a.clone());
            continue;
        }
        if a == "--" {
            rest_are_words = true;
            continue;
        }
        if a == "-h" {
            line.opts.push(("help".into(), None));
            continue;
        }
        if a == "-V" {
            line.opts.push(("version".into(), None));
            continue;
        }
        if let Some(long) = a.strip_prefix("--") {
            let (name, inline) = match long.split_once('=') {
                Some((n, v)) => (n.to_string(), Some(v.to_string())),
                None => (long.to_string(), None),
            };
            if VALUED.contains(&name.as_str()) {
                let value = match inline {
                    Some(v) => v,
                    None if i < args.len() => {
                        i += 1;
                        args[i - 1].clone()
                    }
                    None => return Err(Fail::usage(format!("--{name} needs a value."))),
                };
                line.opts.push((name, Some(value)));
            } else {
                if inline.is_some() {
                    return Err(Fail::usage(format!("--{name} takes no value.")));
                }
                line.opts.push((name, None));
            }
            continue;
        }
        // Job ids may start with `-`; a reference (`-x1#0`, `-x1/0`) is a word, not an option.
        if a.len() > 1 && a.starts_with('-') && !a.contains(['#', '/']) {
            // Short options other than -h/-V do not exist; reported against the command later.
            line.opts.push((a.clone(), None));
            continue;
        }
        line.words.push(a.clone());
    }
    Ok(line)
}

/// The option as typed, for messages.
pub fn shown(name: &str) -> String {
    if name.starts_with('-') { name.to_string() } else { format!("--{name}") }
}

/// A duration `N`, `Ns`, `Nm` or `NmMs` in whole seconds.
pub fn duration(s: &str) -> Option<u64> {
    let digits = |x: &str| (!x.is_empty() && x.bytes().all(|b| b.is_ascii_digit())).then(|| x.parse::<u64>().ok()).flatten();
    if let Some(n) = digits(s) {
        return Some(n);
    }
    if let Some(sec) = s.strip_suffix('s') {
        if let Some((m, sec)) = sec.split_once('m') {
            return Some(digits(m)?.checked_mul(60)?.checked_add(digits(sec)?)?);
        }
        return digits(sec);
    }
    if let Some(m) = s.strip_suffix('m') {
        return digits(m)?.checked_mul(60);
    }
    None
}

/// Seconds as the CLI writes them: `90s`, `2m`.
pub fn show_duration(secs: u64) -> String {
    if secs >= 60 && secs % 60 == 0 { format!("{}m", secs / 60) } else { format!("{secs}s") }
}

/// `--timeout` (1–300 s) or `--batch` (0–60 s).
pub fn duration_opt(line: &Line, name: &str, min: u64, max: u64, default: u64) -> Result<u64, Fail> {
    let Some(v) = line.value(name) else { return Ok(default) };
    match duration(v) {
        Some(n) if (min..=max).contains(&n) => Ok(n),
        _ => Err(Fail::usage(format!(
            "--{name} must be a duration from {min}s to {max}s (for example {}).",
            if name == "batch" { "5s or 1m" } else { "90s or 2m" }
        ))),
    }
}

/// A whole number option in `min..=max`.
pub fn number_opt(line: &Line, name: &str, min: u64, max: u64) -> Result<Option<u64>, Fail> {
    let Some(v) = line.value(name) else { return Ok(None) };
    match v.parse::<u64>() {
        Ok(n) if (min..=max).contains(&n) && v.bytes().all(|b| b.is_ascii_digit()) => Ok(Some(n)),
        _ => Err(Fail::usage(format!("--{name} must be a whole number from {min} to {max}."))),
    }
}

/// A queue item: `JOB#INDEX` or `JOB/INDEX` (AN-CLI-220).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JobRef {
    pub job: String,
    pub index: u64,
}

impl JobRef {
    pub fn parse(s: &str) -> Result<JobRef, Fail> {
        let bad = || Fail::usage(format!("\"{s}\" is not a queue item (use JOB#INDEX, for example J1abc#0)."));
        let (job, index) = s.rsplit_once('#').or_else(|| s.rsplit_once('/')).ok_or_else(bad)?;
        let job_ok = (1..=64).contains(&job.len()) && job.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
        if !job_ok || index.is_empty() || !index.bytes().all(|b| b.is_ascii_digit()) {
            return Err(bad());
        }
        Ok(JobRef { job: job.to_string(), index: index.parse().map_err(|_| bad())? })
    }

    pub fn shown(&self) -> String {
        format!("{}#{}", self.job, self.index)
    }
}
