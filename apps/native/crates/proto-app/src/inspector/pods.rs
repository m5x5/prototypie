//! Projects' Solid pod records for the inspector's pod links: `GET /api/projects` on proto-server
//! (its `private-projects.ttl` gives `pod {doc, subject}` per project folder), fetched once on a
//! worker, then looked up by local folder (case-insensitive, as the reference groups folders).

use std::collections::HashMap;

use rune::Latest;
use serde::Deserialize;

use super::nested::PodRecord;
use crate::app::state::AppState;
use crate::features::http;

#[derive(Deserialize)]
struct ProjectsReply {
    #[serde(default)]
    projects: Vec<Project>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Project {
    local_path: Option<String>,
    pod: Option<Pod>,
}

#[derive(Deserialize)]
struct Pod {
    doc: String,
    subject: String,
}

/// Where the records are.
#[derive(Default)]
enum Load {
    #[default]
    Idle,
    Pending,
    /// Records by lower-cased local folder (trailing `/` dropped).
    Ready(HashMap<String, PodRecord>),
    /// The server was unreachable; retried on the next space change.
    Failed,
}

#[derive(Default)]
pub struct PodsState {
    load: Load,
    req: Latest<Option<HashMap<String, PodRecord>>>,
    /// The space the last failed load happened in (retry once the user moves on).
    failed_in: Option<String>,
}

fn norm(path: &str) -> String {
    path.trim_end_matches('/').to_lowercase()
}

fn fetch(addr: String) -> Option<HashMap<String, PodRecord>> {
    let body = http::request(&addr, "GET", "/api/projects", None).ok()?;
    let reply: ProjectsReply = serde_json::from_slice(&body).ok()?;
    Some(index(reply))
}

fn index(reply: ProjectsReply) -> HashMap<String, PodRecord> {
    reply.projects.into_iter()
        .filter_map(|p| Some((norm(&p.local_path?), p.pod.map(|d| PodRecord { doc: d.doc, subject: d.subject })?)))
        .collect()
}

/// Start the load once (and again after a failure, once the space changed); take the result.
/// True when the records arrived.
pub fn poll(s: &mut AppState) -> bool {
    let st = &mut s.inspector.pods;
    if let Some(result) = st.req.take() {
        st.load = match result {
            Some(map) => Load::Ready(map),
            None => {
                st.failed_in = Some(s.route.space.clone());
                Load::Failed
            }
        };
        return true;
    }
    let retry = matches!(st.load, Load::Failed) && st.failed_in.as_deref() != Some(s.route.space.as_str());
    if matches!(st.load, Load::Idle) || retry {
        let Some(wake) = s.wake.clone() else { return false };
        st.req.set_waker(wake);
        st.req.request(http::server_addr(), fetch);
        st.load = Load::Pending;
    }
    false
}

/// The pod record of the project in folder `path`, once loaded.
pub fn pod_for(s: &AppState, path: &str) -> Option<PodRecord> {
    match &s.inspector.pods.load {
        Load::Ready(map) => map.get(&norm(path)).cloned(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indexes_projects_with_a_folder_and_a_pod() {
        let reply: ProjectsReply = serde_json::from_str(r#"{"available": true, "file": "x", "projects": [
            {"id": "a", "name": "A", "keywords": [], "localPath": "/Users/me/Software/A/", "pod": {"doc": "https://pod/a.ttl", "subject": "https://pod/a.ttl#it"}},
            {"id": "b", "name": "B", "keywords": [], "localPath": "/Users/me/Software/B"},
            {"id": "c", "name": "C", "keywords": [], "pod": {"doc": "d", "subject": "s"}}
        ]}"#).unwrap();
        let map = index(reply);
        assert_eq!(map.len(), 1);
        assert_eq!(map.get(&norm("/users/me/software/a")).map(|p| p.doc.as_str()), Some("https://pod/a.ttl"));
    }
}
