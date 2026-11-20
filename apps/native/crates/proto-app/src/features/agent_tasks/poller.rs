//! One shared poll of `GET /api/agent-tasks` for the header pill, its panel and the inspector: every 4 s
//! while the panel is open, else every 8 s. An explicit state machine (`rune::machine` style): the fetch
//! runs on a `rune::Latest` worker, timers are `Tx::wake_at` deadlines, and the answer lands in `poll`.

use rune::{Latest, Tx, Waker};

use super::data::{self, AgentTasksData};

const POLL_OPEN: f64 = 4.0;
const POLL_CLOSED: f64 = 8.0;

/// Where the poll is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Phase {
    /// Not started (before `setup`), or asked to reload now.
    Due,
    /// A fetch is on the worker.
    Fetching,
    /// Next fetch at `due` (seconds on the poll clock).
    Waiting { due: f64 },
}

#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// The poll clock ticked (every `poll`).
    Tick { now: f64, panel_open: bool },
    /// A fetch finished.
    Answer { now: f64, panel_open: bool },
    /// Reload as soon as possible (panel opened, a note was sent).
    Reload,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Effect {
    Fetch,
    WakeAt(f64),
}

pub fn step(phase: &mut Phase, event: Event) -> Option<Effect> {
    match (*phase, event) {
        (Phase::Fetching, Event::Answer { now, panel_open }) => {
            let due = now + if panel_open { POLL_OPEN } else { POLL_CLOSED };
            *phase = Phase::Waiting { due };
            Some(Effect::WakeAt(due))
        }
        (Phase::Due, Event::Tick { .. }) => {
            *phase = Phase::Fetching;
            Some(Effect::Fetch)
        }
        (Phase::Waiting { due }, Event::Tick { now, .. }) if now >= due => {
            *phase = Phase::Fetching;
            Some(Effect::Fetch)
        }
        // Opening the panel shortens the wait to the open interval.
        (Phase::Waiting { due }, Event::Tick { now, panel_open: true }) if due - now > POLL_OPEN => {
            *phase = Phase::Waiting { due: now + POLL_OPEN };
            Some(Effect::WakeAt(now + POLL_OPEN))
        }
        (Phase::Waiting { .. }, Event::Reload) => {
            *phase = Phase::Due;
            None
        }
        _ => None,
    }
}

/// The poller and what it last got.
pub struct Poller {
    latest: Latest<Result<AgentTasksData, String>>,
    pub phase: Phase,
    pub data: Option<AgentTasksData>,
    /// The last fetch failed (the server isn't up, or answered an error).
    pub failed: bool,
    started: bool,
}

impl Default for Poller {
    fn default() -> Self {
        Poller { latest: Latest::default(), phase: Phase::Due, data: None, failed: false, started: false }
    }
}

impl Poller {
    pub fn setup(&mut self, waker: Waker) {
        self.latest.set_waker(waker);
        self.started = true;
    }

    /// Ask for a fetch on the next poll.
    pub fn reload(&mut self, tx: &mut Tx) {
        step(&mut self.phase, Event::Reload);
        tx.wake_after(std::time::Duration::ZERO);
    }

    /// Feed the clock and any answer in; true when what the pill shows changed.
    pub fn poll(&mut self, now: f64, panel_open: bool, tx: &mut Tx) -> bool {
        if !self.started {
            return false;
        }
        let mut changed = false;
        if let Some(answer) = self.latest.take() {
            match answer {
                Ok(d) => {
                    changed = self.data.as_ref() != Some(&d) || self.failed;
                    self.data = Some(d);
                    self.failed = false;
                }
                Err(_) => {
                    changed = !self.failed;
                    self.failed = true;
                }
            }
            let effect = step(&mut self.phase, Event::Answer { now, panel_open });
            self.run(effect, tx);
        }
        let effect = step(&mut self.phase, Event::Tick { now, panel_open });
        self.run(effect, tx);
        changed
    }

    fn run(&mut self, effect: Option<Effect>, tx: &mut Tx) {
        match effect {
            Some(Effect::Fetch) => {
                let addr = crate::features::http::server_addr();
                self.latest.request(addr, |a| data::fetch(&a));
            }
            Some(Effect::WakeAt(due)) => tx.wake_at(due),
            None => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fetches_then_waits_by_panel_state() {
        let mut p = Phase::Due;
        assert_eq!(step(&mut p, Event::Tick { now: 0.0, panel_open: false }), Some(Effect::Fetch));
        // Ticks while fetching do nothing.
        assert_eq!(step(&mut p, Event::Tick { now: 1.0, panel_open: false }), None);
        assert_eq!(step(&mut p, Event::Answer { now: 1.0, panel_open: false }), Some(Effect::WakeAt(9.0)));
        assert_eq!(step(&mut p, Event::Tick { now: 2.0, panel_open: false }), None);
        // Opening the panel pulls the next fetch in.
        assert_eq!(step(&mut p, Event::Tick { now: 2.0, panel_open: true }), Some(Effect::WakeAt(6.0)));
        assert_eq!(step(&mut p, Event::Tick { now: 6.0, panel_open: true }), Some(Effect::Fetch));
        assert_eq!(step(&mut p, Event::Answer { now: 6.5, panel_open: true }), Some(Effect::WakeAt(10.5)));
        assert_eq!(step(&mut p, Event::Reload), None);
        assert_eq!(p, Phase::Due);
    }
}
