//! Work queue for the "claude" agent (reference `src/lib/server/agent-queue.ts`, file + waiter part; adapted from
//! proto-server's first version). A job writes one item per direction to `<data>/agent-queue/<jobId>-<index>.json`
//! and waits until a Claude Code session POSTs the finished design. Store-dependent parts (item context,
//! placeholder fill, the deleted-card check) live in proto-server; the notes service comes in as a `Notifier`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::SystemTime;

use base64::Engine;
use futures_util::future::BoxFuture;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

use crate::AiError;
use crate::types::{Annotation, BaseDesign, ComponentRef, Direction, GuidelinesRef, PodContextEntry, Scope, Variation};

/// Largest import screenshot accepted, in bytes after decoding.
pub const MAX_SCREENSHOT: usize = 8 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum QueueStatus {
    #[default]
    Pending,
    Done,
    Cancelled,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum QueueMode {
    Import,
    Edit,
}

/// One queued direction, stored exactly as the reference stores it (camelCase, absent fields omitted).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueItem {
    pub job_id: String,
    pub index: usize,
    #[serde(default)]
    pub instruction: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub focus: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<Scope>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub annotations: Option<Vec<Annotation>>,
    pub direction: Direction,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_html: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bases: Option<Vec<BaseDesign>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pod_context: Option<Vec<PodContextEntry>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub components: Option<Vec<ComponentRef>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub guidelines: Option<GuidelinesRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<QueueMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_design_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub element: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub import_source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub import_screenshot: Option<String>,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub status: QueueStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub annotation_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub placeholder_id: Option<String>,
    /// Who works on it (a worker or session name), set by `claim`; re-claiming overwrites.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claimed_at: Option<String>,
    /// Where the result lands; computed by the server when listing, never stored.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<Value>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoteStatus {
    Resolved,
    Dismissed,
}

/// The notes service bridge (implemented by proto-server; must not block for long: 3 s timeouts there).
pub trait Notifier: Send + Sync {
    /// Post a note; the note id, or None when the notes service isn't reachable.
    fn announce(&self, comment: String, element: String, element_path: String) -> BoxFuture<'static, Option<String>>;
    fn set_status(&self, annotation_id: String, status: NoteStatus) -> BoxFuture<'static, ()>;
}

/// A `Notifier` that does nothing (tests, or no notes service).
pub struct NoNotifier;

impl Notifier for NoNotifier {
    fn announce(&self, _: String, _: String, _: String) -> BoxFuture<'static, Option<String>> {
        Box::pin(async { None })
    }
    fn set_status(&self, _: String, _: NoteStatus) -> BoxFuture<'static, ()> {
        Box::pin(async {})
    }
}

type Waiter = oneshot::Sender<Result<Variation, String>>;

pub fn key(job_id: &str, index: usize) -> String {
    format!("{job_id}-{index}")
}

#[derive(Clone)]
pub struct AgentQueue {
    dir: PathBuf,
    notifier: Arc<dyn Notifier>,
    waiting: Arc<Mutex<HashMap<String, Waiter>>>,
}

impl AgentQueue {
    /// Items live in `data_dir/agent-queue`.
    pub fn new(data_dir: &Path, notifier: Arc<dyn Notifier>) -> Self {
        Self { dir: data_dir.join("agent-queue"), notifier, waiting: Arc::default() }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn file(&self, job_id: &str, index: usize) -> PathBuf {
        self.dir.join(format!("{}.json", key(job_id, index)))
    }

    async fn save(&self, item: &QueueItem) -> std::io::Result<()> {
        tokio::fs::create_dir_all(&self.dir).await?;
        let stored = QueueItem { context: None, ..item.clone() };
        let json = serde_json::to_string_pretty(&stored).map_err(std::io::Error::other)?;
        tokio::fs::write(self.file(&item.job_id, item.index), json).await
    }

    pub async fn load(&self, job_id: &str, index: usize) -> Option<QueueItem> {
        let txt = tokio::fs::read_to_string(self.file(job_id, index)).await.ok()?;
        serde_json::from_str(&txt).ok()
    }

    /// pending → `status`, and close its note in the notes service (done → resolved, else dismissed).
    async fn set_status(&self, job_id: &str, index: usize, status: QueueStatus) {
        let Some(item) = self.load(job_id, index).await else { return };
        if item.status != QueueStatus::Pending {
            return;
        }
        let annotation = item.annotation_id.clone();
        let _ = self.save(&QueueItem { status, ..item }).await;
        if let Some(id) = annotation {
            let note = if status == QueueStatus::Done { NoteStatus::Resolved } else { NoteStatus::Dismissed };
            self.notifier.set_status(id, note).await;
        }
    }

    pub fn is_waiting(&self, job_id: &str, index: usize) -> bool {
        self.waiting.lock().unwrap().contains_key(&key(job_id, index))
    }

    /// `(jobId, index)` of every item that has a live waiter.
    pub fn waiting(&self) -> Vec<(String, usize)> {
        self.waiting
            .lock()
            .unwrap()
            .keys()
            .filter_map(|k| {
                let (job, idx) = k.rsplit_once('-')?;
                Some((job.to_string(), idx.parse().ok()?))
            })
            .collect()
    }

    /// Queue one direction and wait for its result. `createdAt`, `status` and `annotationId` are set here.
    /// Rejects with `Cancelled` when `cancel` trips and with `Other(reason)` after `cancel_item`; the item is then
    /// marked cancelled and its note dismissed.
    pub async fn enqueue_and_wait(&self, mut item: QueueItem, cancel: CancellationToken) -> Result<Variation, AiError> {
        item.created_at = crate::iso_now();
        item.status = QueueStatus::Pending;
        item.annotation_id = None;
        item.context = None;
        self.save(&item).await.map_err(|e| AiError::Other(format!("Couldn't save the queue item: {e}")))?;
        let k = key(&item.job_id, item.index);
        let (tx, rx) = oneshot::channel();
        self.waiting.lock().unwrap().insert(k.clone(), tx);
        if cancel.is_cancelled() {
            self.stop(&k, &item).await;
            return Err(AiError::Cancelled);
        }
        let comment = announce_comment(&item);
        let announce = self.notifier.announce(comment, format!("agent-queue item {k}"), format!("data/agent-queue/{k}.json"));
        tokio::select! {
            id = announce => {
                if let Some(id) = id
                    && let Some(now) = self.load(&item.job_id, item.index).await
                {
                    let _ = self.save(&QueueItem { annotation_id: Some(id), ..now }).await;
                }
            }
            _ = cancel.cancelled() => {
                self.stop(&k, &item).await;
                return Err(AiError::Cancelled);
            }
        }
        tokio::select! {
            r = rx => match r {
                Ok(Ok(v)) => Ok(v),
                Ok(Err(reason)) => Err(AiError::Other(reason)),
                Err(_) => Err(AiError::Cancelled),
            },
            _ = cancel.cancelled() => {
                self.stop(&k, &item).await;
                Err(AiError::Cancelled)
            }
        }
    }

    async fn stop(&self, k: &str, item: &QueueItem) {
        self.waiting.lock().unwrap().remove(k);
        self.set_status(&item.job_id, item.index, QueueStatus::Cancelled).await;
    }

    /// Give up on a waiting item (e.g. "Card deleted"): its job gets `reason` as the direction's error and the
    /// item is cancelled. False when nobody waited for it.
    pub async fn cancel_item(&self, job_id: &str, index: usize, reason: &str) -> bool {
        let tx = self.waiting.lock().unwrap().remove(&key(job_id, index));
        let Some(tx) = tx else { return false };
        let _ = tx.send(Err(reason.to_string()));
        self.set_status(job_id, index, QueueStatus::Cancelled).await;
        true
    }

    /// Deliver a finished design: resolves the waiter, marks the item done, resolves its note.
    /// False when nobody waits for it (done, cancelled, expired, or another process) → 404.
    pub async fn submit(&self, job_id: &str, index: usize, v: Variation) -> bool {
        let tx = self.waiting.lock().unwrap().remove(&key(job_id, index));
        let Some(tx) = tx else { return false };
        let _ = tx.send(Ok(v));
        self.set_status(job_id, index, QueueStatus::Done).await;
        true
    }

    /// Remember the placeholder card id on disk (once), so a delivery still finds its card after the run is gone.
    pub async fn remember_placeholder(&self, job_id: &str, index: usize, placeholder_id: &str) {
        if let Some(item) = self.load(job_id, index).await
            && item.placeholder_id.is_none()
        {
            let _ = self.save(&QueueItem { placeholder_id: Some(placeholder_id.to_string()), ..item }).await;
        }
    }

    /// Record who works on an item (stored in its file, so it survives restarts). `None`: no such item.
    pub async fn claim(&self, job_id: &str, index: usize, agent: &str) -> Option<QueueItem> {
        let item = self.load(job_id, index).await?;
        let item = QueueItem { agent: Some(agent.to_string()), claimed_at: Some(crate::iso_now()), ..item };
        self.save(&item).await.ok()?;
        Some(item)
    }

    /// Every item on disk in any status, with the file's mtime (unreadable files skipped).
    pub async fn read_all(&self) -> Vec<(QueueItem, SystemTime)> {
        let mut out = Vec::new();
        let Ok(mut rd) = tokio::fs::read_dir(&self.dir).await else { return out };
        while let Ok(Some(e)) = rd.next_entry().await {
            let path = e.path();
            if path.extension().is_none_or(|x| x != "json") {
                continue;
            }
            let (Ok(txt), Ok(meta)) = (tokio::fs::read_to_string(&path).await, e.metadata().await) else { continue };
            if let Ok(item) = serde_json::from_str::<QueueItem>(&txt) {
                out.push((item, meta.modified().unwrap_or(SystemTime::UNIX_EPOCH)));
            }
        }
        out
    }

    /// Pending items with a live waiter in this process, oldest first (without context).
    pub async fn list_pending(&self) -> Vec<QueueItem> {
        let mut items: Vec<QueueItem> = self
            .read_all()
            .await
            .into_iter()
            .map(|(i, _)| i)
            .filter(|i| i.status == QueueStatus::Pending && self.is_waiting(&i.job_id, i.index))
            .collect();
        items.sort_by(|a, b| a.created_at.cmp(&b.created_at));
        items
    }

    /// Save an import's screenshot (`data:image/<png|jpeg|jpg|webp|gif>;base64,…`, ≤ 8 MiB) next to the items.
    pub async fn save_screenshot(&self, job_id: &str, data_url: &str) -> Option<PathBuf> {
        static RE: LazyLock<Regex> =
            LazyLock::new(|| Regex::new(r"^data:image/(png|jpe?g|webp|gif);base64,([A-Za-z0-9+/=]+)$").unwrap());
        let m = RE.captures(data_url)?;
        let bytes = base64::engine::general_purpose::STANDARD.decode(&m[2]).ok()?;
        if bytes.is_empty() || bytes.len() > MAX_SCREENSHOT {
            return None;
        }
        let ext = if &m[1] == "jpeg" { "jpg" } else { &m[1] };
        tokio::fs::create_dir_all(&self.dir).await.ok()?;
        let file = self.dir.join(format!("{job_id}-screenshot.{ext}"));
        tokio::fs::write(&file, bytes).await.ok()?;
        Some(std::path::absolute(&file).unwrap_or(file))
    }
}

/// The note text (notes service) that mirrors an item (same text and suffixes as the reference).
pub fn announce_comment(item: &QueueItem) -> String {
    let mut c = format!(
        "[prototypie job {}#{}] {}",
        item.job_id,
        item.index,
        if item.instruction.is_empty() { &item.direction.brief } else { &item.instruction }
    );
    if let Some(list) = item.bases.as_ref().filter(|l| !l.is_empty()) {
        let titles: Vec<&str> = list.iter().map(|b| b.title.as_str()).collect();
        c.push_str(&format!(" · COMBINE {} designs (item.bases): {}", list.len(), titles.join(" + ")));
    }
    if let Some(list) = item.pod_context.as_ref().filter(|l| !l.is_empty()) {
        let labels: Vec<&str> = list.iter().map(|e| e.label.as_str()).collect();
        c.push_str(&format!(" · POD DATA (item.podContext): {}", labels.join(", ")));
    }
    if let Some(list) = item.components.as_ref().filter(|l| !l.is_empty()) {
        let titles: Vec<&str> = list.iter().map(|e| e.title.as_str()).collect();
        c.push_str(&format!(" · REUSE COMPONENTS AS-IS (item.components): {}", titles.join(", ")));
    }
    if let Some(g) = &item.guidelines {
        c.push_str(&format!(" · FOLLOW DESIGN GUIDELINES (item.guidelines): {}", g.title));
    }
    match item.mode {
        Some(QueueMode::Import) => c.push_str(&format!(
            " · IMPORT (item.mode): recreate \"{}\" faithfully from {}{}; read the source read-only, invent nothing",
            item.element.as_deref().unwrap_or(""),
            item.import_source.as_deref().unwrap_or(""),
            item.import_screenshot.as_ref().map(|s| format!(" + screenshot {s}")).unwrap_or_default()
        )),
        Some(QueueMode::Edit) => c.push_str(&format!(
            " · EDIT IN PLACE (item.mode) card {}: return item.baseHtml with ONLY this change, everything else byte-identical; the POST overwrites that card",
            item.target_design_id.as_deref().unwrap_or("")
        )),
        None => {}
    }
    c.push_str(&format!(
        " · context + skills: prototypie queue show {}#{} (item.context), prototypie queue deliver, prototypie cards create (skill prototypie)",
        item.job_id, item.index
    ));
    c.push_str(" · rules: only the discussed component, one variant, no explanatory text, short space-scoped title (no project name, no \"Komponente\")");
    c
}

static RE_VIEWPORT_META: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?i)<meta\b[^>]*\bname\s*=\s*["']?viewport["']?[^>]*>"#).unwrap());
static RE_WIDTH: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\bwidth\s*=\s*(\d+)").unwrap());
static RE_HEIGHT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\bheight\s*=\s*(\d+)").unwrap());

fn meta_number(html: &str, re: &Regex) -> Option<u32> {
    let meta = RE_VIEWPORT_META.find(html)?;
    let n: u32 = re.captures(meta.as_str())?[1].parse().ok()?;
    (200..=4000).contains(&n).then_some(n)
}

/// A fixed layout width declared in the html (`<meta name="viewport" content="width=1240">`), if in 200..=4000.
pub fn infer_viewport(html: &str) -> Option<u32> {
    meta_number(html, &RE_WIDTH)
}

/// A fixed screen height (`content="width=640,height=960"`), if in 200..=4000.
pub fn infer_viewport_height(html: &str) -> Option<u32> {
    meta_number(html, &RE_HEIGHT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex as StdMutex;

    #[derive(Default)]
    struct Recorder {
        announced: StdMutex<Vec<(String, String, String)>>,
        statuses: StdMutex<Vec<(String, NoteStatus)>>,
    }

    impl Notifier for Recorder {
        fn announce(&self, comment: String, element: String, path: String) -> BoxFuture<'static, Option<String>> {
            self.announced.lock().unwrap().push((comment, element, path));
            Box::pin(async { Some("note1".to_string()) })
        }
        fn set_status(&self, id: String, status: NoteStatus) -> BoxFuture<'static, ()> {
            self.statuses.lock().unwrap().push((id, status));
            Box::pin(async {})
        }
    }

    fn item(job: &str, index: usize) -> QueueItem {
        QueueItem {
            job_id: job.into(),
            index,
            instruction: "make it pop".into(),
            direction: Direction { title: "Direction 1".into(), brief: "b".into() },
            ..Default::default()
        }
    }

    fn variation() -> Variation {
        Variation { title: "T".into(), rationale: "R".into(), html: "<p>".into(), viewport: None, viewport_height: None }
    }

    async fn wait_until_waiting(q: &AgentQueue, job: &str, i: usize) {
        for _ in 0..200 {
            if q.is_waiting(job, i) && q.load(job, i).await.is_some_and(|it| it.annotation_id.is_some()) {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        panic!("item never queued");
    }

    #[tokio::test]
    async fn enqueue_submit_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let rec = Arc::new(Recorder::default());
        let q = AgentQueue::new(dir.path(), rec.clone());
        let q2 = q.clone();
        let waiter = tokio::spawn(async move { q2.enqueue_and_wait(item("J1", 0), CancellationToken::new()).await });
        wait_until_waiting(&q, "J1", 0).await;

        let pending = q.list_pending().await;
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].status, QueueStatus::Pending);
        assert_eq!(pending[0].annotation_id.as_deref(), Some("note1"));
        let (comment, element, path) = rec.announced.lock().unwrap()[0].clone();
        assert!(comment.starts_with("[prototypie job J1#0] make it pop · context + skills:"));
        assert!(comment.contains("prototypie queue show J1#0 (item.context)") && comment.contains("prototypie cards create (skill prototypie)"), "{comment}");
        assert!(!comment.contains("scripts/"), "{comment}");
        assert_eq!(element, "agent-queue item J1-0");
        assert_eq!(path, "data/agent-queue/J1-0.json");

        // File format: camelCase, absent fields omitted.
        let raw: Value = serde_json::from_str(&std::fs::read_to_string(dir.path().join("agent-queue/J1-0.json")).unwrap()).unwrap();
        assert_eq!(raw["jobId"], "J1");
        assert_eq!(raw["status"], "pending");
        assert!(raw.get("focus").is_none() && raw.get("context").is_none());

        assert!(q.submit("J1", 0, variation()).await);
        assert_eq!(waiter.await.unwrap().unwrap(), variation());
        assert!(!q.submit("J1", 0, variation()).await, "second delivery finds nobody waiting");
        assert_eq!(q.load("J1", 0).await.unwrap().status, QueueStatus::Done);
        assert_eq!(rec.statuses.lock().unwrap().as_slice(), &[("note1".to_string(), NoteStatus::Resolved)]);
        assert!(q.list_pending().await.is_empty());
        let all = q.read_all().await;
        assert_eq!(all.len(), 1);
    }

    #[tokio::test]
    async fn cancel_token_and_cancel_item() {
        let dir = tempfile::tempdir().unwrap();
        let rec = Arc::new(Recorder::default());
        let q = AgentQueue::new(dir.path(), rec.clone());

        let token = CancellationToken::new();
        let (q2, t2) = (q.clone(), token.clone());
        let a = tokio::spawn(async move { q2.enqueue_and_wait(item("J2", 0), t2).await });
        wait_until_waiting(&q, "J2", 0).await;
        token.cancel();
        assert!(matches!(a.await.unwrap(), Err(AiError::Cancelled)));
        assert_eq!(q.load("J2", 0).await.unwrap().status, QueueStatus::Cancelled);
        assert!(!q.is_waiting("J2", 0));

        let q3 = q.clone();
        let b = tokio::spawn(async move { q3.enqueue_and_wait(item("J2", 1), CancellationToken::new()).await });
        wait_until_waiting(&q, "J2", 1).await;
        assert_eq!(q.waiting(), vec![("J2".to_string(), 1)]);
        assert!(q.cancel_item("J2", 1, "Card deleted").await);
        match b.await.unwrap() {
            Err(AiError::Other(m)) => assert_eq!(m, "Card deleted"),
            other => panic!("unexpected {other:?}"),
        }
        assert!(!q.cancel_item("J2", 1, "again").await);
        let statuses = rec.statuses.lock().unwrap().clone();
        assert_eq!(statuses.iter().filter(|(_, s)| *s == NoteStatus::Dismissed).count(), 2);
    }

    #[tokio::test]
    async fn reads_reference_item_and_remembers_placeholder() {
        let dir = tempfile::tempdir().unwrap();
        let q = AgentQueue::new(dir.path(), Arc::new(NoNotifier));
        std::fs::create_dir_all(q.dir()).unwrap();
        let reference = r#"{
  "jobId": "EN8YHP4hPi", "index": 0, "instruction": "x", "scope": "component",
  "annotations": [{"element": "nav", "comment": "bigger"}],
  "direction": {"title": "Direction 1", "brief": "x"},
  "guidelines": {"cardId": "c1", "title": "Rules", "text": "t"},
  "mode": "edit", "targetDesignId": "d1",
  "createdAt": "2026-11-18T20:00:00.000Z", "status": "cancelled", "annotationId": "a1"
}"#;
        std::fs::write(q.dir().join("EN8YHP4hPi-0.json"), reference).unwrap();
        let it = q.load("EN8YHP4hPi", 0).await.unwrap();
        assert_eq!(it.mode, Some(QueueMode::Edit));
        assert_eq!(it.status, QueueStatus::Cancelled);
        let c = announce_comment(&it);
        assert!(c.contains(" · FOLLOW DESIGN GUIDELINES (item.guidelines): Rules · EDIT IN PLACE (item.mode) card d1: return"));
        q.remember_placeholder("EN8YHP4hPi", 0, "p1").await;
        q.remember_placeholder("EN8YHP4hPi", 0, "p2").await;
        assert_eq!(q.load("EN8YHP4hPi", 0).await.unwrap().placeholder_id.as_deref(), Some("p1"));
    }

    #[tokio::test]
    async fn screenshots_are_validated() {
        let dir = tempfile::tempdir().unwrap();
        let q = AgentQueue::new(dir.path(), Arc::new(NoNotifier));
        let path = q.save_screenshot("J3", "data:image/jpeg;base64,/9j/4AAQ").await.unwrap();
        assert!(path.is_absolute() && path.ends_with("agent-queue/J3-screenshot.jpg"));
        assert_eq!(std::fs::read(&path).unwrap(), vec![0xff, 0xd8, 0xff, 0xe0, 0x00, 0x10]);
        assert!(q.save_screenshot("J3", "data:image/bmp;base64,AAAA").await.is_none());
        assert!(q.save_screenshot("J3", "data:image/png;base64,").await.is_none());
    }

    #[test]
    fn viewport_meta_is_parsed() {
        let html = r#"<head><meta charset="utf-8"><meta name="viewport" content="width=640, height=960"></head>"#;
        assert_eq!(infer_viewport(html), Some(640));
        assert_eq!(infer_viewport_height(html), Some(960));
        assert_eq!(infer_viewport(r#"<meta name=viewport content="width=device-width">"#), None);
        assert_eq!(infer_viewport(r#"<meta name="viewport" content="width=5000">"#), None);
        assert_eq!(infer_viewport("<p>no meta</p>"), None);
    }
}
