//! Solid pod data catalog (reference `src/lib/server/pod-catalog.ts`): documented entries from
//! `<data>/pod-catalog.json`, entries marked `live` probed with an unauthenticated GET, cached 5 minutes.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{Map, Value, json};

use crate::util;

const TTL: Duration = Duration::from_secs(5 * 60);
const TIMEOUT: Duration = Duration::from_millis(2500);

#[derive(Clone, Default)]
pub struct PodCatalog {
    cache: Arc<Mutex<Option<(Instant, Value)>>>,
}

/// The web app's catalog (`$PROTOTYPIE_REFERENCE_DATA/pod-catalog.json`), copied in once when the data dir has none
/// (read-only source). None when the variable isn't set.
fn reference_file() -> Option<PathBuf> {
    std::env::var_os("PROTOTYPIE_REFERENCE_DATA").filter(|d| !d.is_empty()).map(|d| PathBuf::from(d).join("pod-catalog.json"))
}

async fn probe(http: &reqwest::Client, url: &str, accept: &str) -> Value {
    let checked_at = util::iso_now();
    let res = http.get(url).header("accept", accept).timeout(TIMEOUT).send().await;
    let Ok(res) = res else { return json!({ "status": "error", "checkedAt": checked_at }) };
    let status = res.status().as_u16();
    if !res.status().is_success() {
        return json!({ "status": status, "checkedAt": checked_at });
    }
    let text = if url.ends_with('/') { res.text().await.unwrap_or_default() } else { String::new() };
    let mut out = json!({ "status": status, "checkedAt": checked_at });
    if !text.is_empty() {
        out["contains"] = json!(contained_names(&text));
    }
    out
}

/// Best-effort names from an LDP container listing in Turtle (`ldp:contains <a>, <b> .`).
pub fn contained_names(turtle: &str) -> Vec<String> {
    let block = regex::Regex::new(r"ldp:contains\s+((?:<[^>]*>\s*,?\s*)+)").unwrap();
    let iri = regex::Regex::new(r"<([^>]+)>").unwrap();
    let mut names = std::collections::BTreeSet::new();
    for m in block.captures_iter(turtle) {
        for i in iri.captures_iter(&m[1]) {
            let s = &i[1];
            let parts: Vec<&str> = s.split('/').collect();
            let last = if s.ends_with('/') { format!("{}/", parts[parts.len().saturating_sub(2)]) } else { parts.last().unwrap_or(&"").to_string() };
            if !last.is_empty() && last != "/" {
                names.insert(percent_decode(&last));
            }
        }
    }
    names.into_iter().collect()
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16)
        {
            out.push(v);
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn access_after(documented: &Value, probe: &Value) -> Value {
    match probe["status"].as_u64() {
        Some(s) if (200..300).contains(&s) => json!("live"),
        Some(401 | 403) => json!("401"),
        _ => documented.clone(),
    }
}

async fn build(data_dir: &Path) -> anyhow::Result<Value> {
    let file = data_dir.join("pod-catalog.json");
    if !tokio::fs::try_exists(&file).await.unwrap_or(false) {
        if let Some(src) = reference_file()
            && tokio::fs::try_exists(&src).await.unwrap_or(false)
        {
            tokio::fs::create_dir_all(data_dir).await?;
            tokio::fs::copy(&src, &file).await?;
        }
    }
    let parsed: Value = serde_json::from_str(&tokio::fs::read_to_string(&file).await?)?;
    let pods = parsed["pods"].as_array().cloned().unwrap_or_default();
    let base_of = |pod_id: &str| -> Option<String> {
        let p = pods.iter().find(|p| p["id"] == pod_id)?;
        let env = std::env::var(format!("POD_BASE_{}", pod_id.to_uppercase())).ok().filter(|s| !s.is_empty());
        env.or_else(|| p["base"].as_str().map(str::to_string))
    };
    let http = reqwest::Client::new();
    let entries = futures_util::future::join_all(parsed["entries"].as_array().cloned().unwrap_or_default().into_iter().map(|e| {
        let http = http.clone();
        let base = e["pod"].as_str().and_then(base_of);
        async move {
            let mut o: Map<String, Value> = e.as_object().cloned().unwrap_or_default();
            let live = o.remove("live").is_some_and(|v| v == json!(true));
            let accept = o.remove("accept").and_then(|v| v.as_str().map(str::to_string)).unwrap_or_else(|| "text/turtle".into());
            let (true, Some(base)) = (live, base) else { return Value::Object(o) };
            let Some(url) = reqwest::Url::parse(&base).ok().and_then(|b| b.join(o.get("path").and_then(Value::as_str).unwrap_or("")).ok()) else {
                return Value::Object(o);
            };
            let result = probe(&http, url.as_str(), &accept).await;
            let documented = o.get("access").cloned().unwrap_or(Value::Null);
            o.insert("access".into(), access_after(&documented, &result));
            o.insert("probe".into(), result);
            Value::Object(o)
        }
    }))
    .await;
    let pods_out: Vec<Value> = pods
        .iter()
        .map(|p| {
            let mut o = json!({ "id": p["id"], "label": p["label"], "display": p["display"] });
            if let Some(n) = p.get("note").filter(|n| n.as_str().is_some_and(|s| !s.is_empty())) {
                o["note"] = n.clone();
            }
            o
        })
        .collect();
    Ok(json!({ "version": parsed["version"], "pods": pods_out, "entries": entries, "checkedAt": util::iso_now() }))
}

impl PodCatalog {
    /// The catalog with fresh-enough probes; `refresh` skips the cache.
    pub async fn get(&self, data_dir: &Path, refresh: bool) -> anyhow::Result<Value> {
        if !refresh
            && let Some((at, v)) = self.cache.lock().unwrap().as_ref()
            && at.elapsed() < TTL
        {
            return Ok(v.clone());
        }
        let v = build(data_dir).await?;
        *self.cache.lock().unwrap() = Some((Instant::now(), v.clone()));
        Ok(v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn container_names() {
        let t = "<> ldp:contains <https://p/x/a.ttl>, <https://p/x/sub%20dir/> .";
        assert_eq!(contained_names(t), ["a.ttl", "sub dir/"]);
    }

    #[test]
    fn access_from_probe() {
        assert_eq!(access_after(&json!("ref"), &json!({ "status": 200 })), json!("live"));
        assert_eq!(access_after(&json!("live"), &json!({ "status": 403 })), json!("401"));
        assert_eq!(access_after(&json!("ref"), &json!({ "status": "error" })), json!("ref"));
        assert_eq!(access_after(&json!("ref"), &json!({ "status": 404 })), json!("ref"));
    }
}
