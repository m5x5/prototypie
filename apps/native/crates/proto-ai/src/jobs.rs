//! Detached generation jobs (reference `src/lib/server/jobs.ts`; adapted from proto-server's first version).
//! A job keeps running when its client goes away; its events are recorded so any number of subscribers can replay
//! them from an offset and follow live. Every job ends with exactly one `Done`.

use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures_util::Stream;
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

use crate::AiError;
use crate::types::VariationEvent;

pub type JobId = String;

/// Finished jobs stay replayable this long.
pub const TTL: Duration = Duration::from_secs(60 * 60);

struct Job {
    events: Vec<VariationEvent>,
    done: bool,
    cancel: CancellationToken,
    created: Instant,
    /// Bumped on every new event and when the job ends.
    tick: watch::Sender<u64>,
}

/// Records events into one job. Cheap to clone; emitting after the job ended is ignored.
#[derive(Clone)]
pub struct Emitter {
    id: JobId,
    jobs: Jobs,
}

impl Emitter {
    pub fn send(&self, event: VariationEvent) {
        self.jobs.push(&self.id, event, false);
    }

    pub fn id(&self) -> &str {
        &self.id
    }
}

#[derive(Clone, Default)]
pub struct Jobs {
    inner: Arc<Mutex<HashMap<JobId, Job>>>,
    ttl: Option<Duration>,
}

impl Jobs {
    pub fn new() -> Self {
        Self::default()
    }

    /// For tests: a shorter replay window.
    pub fn with_ttl(ttl: Duration) -> Self {
        Self { inner: Arc::default(), ttl: Some(ttl) }
    }

    fn sweep(&self, map: &mut HashMap<JobId, Job>) {
        let ttl = self.ttl.unwrap_or(TTL);
        map.retain(|_, j| !(j.done && j.created.elapsed() > ttl));
    }

    /// Start `run` as a detached job on the current tokio runtime and return its id at once.
    /// An `Err` from `run` is recorded as `Error { index: None }`; `Done` always comes last.
    pub fn start<F, Fut>(&self, run: F) -> JobId
    where
        F: FnOnce(Emitter, CancellationToken, JobId) -> Fut,
        Fut: Future<Output = Result<(), AiError>> + Send + 'static,
    {
        let id = crate::nanoid(10);
        let cancel = CancellationToken::new();
        {
            let mut map = self.inner.lock().unwrap();
            self.sweep(&mut map);
            let (tick, _) = watch::channel(0);
            map.insert(id.clone(), Job { events: Vec::new(), done: false, cancel: cancel.clone(), created: Instant::now(), tick });
        }
        let fut = run(Emitter { id: id.clone(), jobs: self.clone() }, cancel, id.clone());
        let jobs = self.clone();
        let job_id = id.clone();
        tokio::spawn(async move {
            if let Err(err) = fut.await {
                jobs.push(&job_id, VariationEvent::Error { index: None, message: err.to_string() }, false);
            }
            jobs.push(&job_id, VariationEvent::Done, true);
        });
        id
    }

    fn push(&self, id: &str, event: VariationEvent, finish: bool) {
        let mut map = self.inner.lock().unwrap();
        let Some(job) = map.get_mut(id) else { return };
        if job.done {
            return;
        }
        job.events.push(event);
        job.done |= finish;
        job.tick.send_modify(|n| *n += 1);
    }

    /// Whether the job is still known (running, or finished within the replay window).
    pub fn exists(&self, id: &str) -> bool {
        let mut map = self.inner.lock().unwrap();
        self.sweep(&mut map);
        map.contains_key(id)
    }

    /// Trip the job's cancellation token (the job still ends with `Done`).
    pub fn cancel(&self, id: &str) {
        if let Some(j) = self.inner.lock().unwrap().get(id) {
            j.cancel.cancel();
        }
    }

    /// Events from `from` and whether the job is done; None for an unknown job.
    pub fn events(&self, id: &str, from: usize) -> Option<(Vec<VariationEvent>, bool)> {
        let map = self.inner.lock().unwrap();
        let j = map.get(id)?;
        Some((j.events.get(from..).map(<[VariationEvent]>::to_vec).unwrap_or_default(), j.done))
    }

    /// Replay the job's events from `from`, then follow live; the stream ends after `Done`.
    /// None for an unknown or expired job. Does not include the `Job { id }` line of the HTTP stream.
    pub fn subscribe(&self, id: &str, from: usize) -> Option<impl Stream<Item = VariationEvent> + Send + 'static> {
        let rx = self.inner.lock().unwrap().get(id)?.tick.subscribe();
        let state = Follow { jobs: self.clone(), id: id.to_string(), cursor: from, rx, pending: Vec::new(), finished: false };
        Some(futures_util::stream::unfold(state, |mut s| async move {
            loop {
                if let Some(e) = s.pending.pop() {
                    return Some((e, s));
                }
                if s.finished {
                    return None;
                }
                let (events, done) = s.jobs.events(&s.id, s.cursor)?;
                if !events.is_empty() {
                    s.cursor += events.len();
                    s.finished = done;
                    s.pending = events.into_iter().rev().collect();
                    continue;
                }
                if done {
                    return None;
                }
                s.rx.borrow_and_update();
                // Re-check after marking the tick seen, so an event pushed in between isn't missed.
                if s.jobs.events(&s.id, s.cursor).is_some_and(|(e, d)| !e.is_empty() || d) {
                    continue;
                }
                if s.rx.changed().await.is_err() {
                    return None;
                }
            }
        }))
    }
}

struct Follow {
    jobs: Jobs,
    id: JobId,
    cursor: usize,
    rx: watch::Receiver<u64>,
    /// Fetched but not yet yielded, reversed (pop = next).
    pending: Vec<VariationEvent>,
    finished: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Direction;
    use futures_util::StreamExt;

    fn plan() -> VariationEvent {
        VariationEvent::Plan { directions: vec![Direction { title: "a".into(), brief: "b".into() }] }
    }

    #[tokio::test]
    async fn live_subscriber_sees_events_then_done() {
        let jobs = Jobs::new();
        let (go_tx, go_rx) = tokio::sync::oneshot::channel::<()>();
        let id = jobs.start(|emit, _cancel, _id| async move {
            emit.send(plan());
            let _ = go_rx.await;
            emit.send(VariationEvent::Error { index: Some(0), message: "x".into() });
            Ok(())
        });
        let mut s = Box::pin(jobs.subscribe(&id, 0).unwrap());
        assert_eq!(s.next().await, Some(plan()));
        go_tx.send(()).unwrap();
        assert_eq!(s.next().await, Some(VariationEvent::Error { index: Some(0), message: "x".into() }));
        assert_eq!(s.next().await, Some(VariationEvent::Done));
        assert_eq!(s.next().await, None);
    }

    #[tokio::test]
    async fn late_subscriber_gets_full_replay_and_offsets_work() {
        let jobs = Jobs::new();
        let id = jobs.start(|emit, _c, _id| async move {
            emit.send(plan());
            Err(AiError::Llm("boom".into()))
        });
        // Wait until done.
        let all: Vec<_> = jobs.subscribe(&id, 0).unwrap().collect().await;
        assert_eq!(all, vec![plan(), VariationEvent::Error { index: None, message: "boom".into() }, VariationEvent::Done]);
        let again: Vec<_> = jobs.subscribe(&id, 0).unwrap().collect().await;
        assert_eq!(again, all);
        let tail: Vec<_> = jobs.subscribe(&id, 2).unwrap().collect().await;
        assert_eq!(tail, vec![VariationEvent::Done]);
        let past_end: Vec<_> = jobs.subscribe(&id, 10).unwrap().collect().await;
        assert!(past_end.is_empty());
        assert!(jobs.subscribe("nope", 0).is_none());
    }

    #[tokio::test]
    async fn cancel_trips_token_and_job_still_ends() {
        let jobs = Jobs::new();
        let id = jobs.start(|emit, cancel, _id| async move {
            cancel.cancelled().await;
            emit.send(VariationEvent::Error { index: Some(0), message: "Cancelled".into() });
            Ok(())
        });
        assert!(jobs.exists(&id));
        jobs.cancel(&id);
        let all: Vec<_> = jobs.subscribe(&id, 0).unwrap().collect().await;
        assert_eq!(all.last(), Some(&VariationEvent::Done));
        assert_eq!(all.len(), 2);
    }

    #[tokio::test]
    async fn finished_jobs_expire_after_ttl() {
        let jobs = Jobs::with_ttl(Duration::from_millis(20));
        let id = jobs.start(|_e, _c, _id| async { Ok(()) });
        let _: Vec<_> = jobs.subscribe(&id, 0).unwrap().collect().await;
        assert!(jobs.exists(&id));
        tokio::time::sleep(Duration::from_millis(40)).await;
        assert!(!jobs.exists(&id));
    }
}
