//! Review (the reference's `spokes/review*.tsx`): the queue of ready, unreviewed designs, the keyboard-driven
//! review mode (star, discard, done, siblings of the same round) and the header's Review dock entry.
//! "Another round" waits for generation (proto-ai) in M3; everything else works on the store.

pub mod dock;
pub mod select;
pub mod view;

use std::collections::BTreeMap;
use std::sync::Arc;

use proto_core::{Change, Design, DesignKind, DesignStatus, RecordKind, Snapshot, VariationRequest, ops};
use rune::{TextEdit, Tx};

use crate::app::state::AppState;
use crate::features::{NoticeAction, nav, notify_with};

#[derive(Default)]
pub struct ReviewState {
    pub open: bool,
    /// The design on stage (None: the queue's head).
    pub current: Option<String>,
    /// Manual sibling picks for "Another round" (number → on), over what the feedback's leading clause picks.
    pub overrides: BTreeMap<usize, bool>,
    /// The round the overrides belong to (sibling ids joined); a new round resets them.
    pub round_key: String,
    pub feedback: TextEdit,
    /// Designs per target for "Another round" (1–5, default 3).
    pub round_count: u32,
    /// The header dock's panel is open.
    pub dock_open: bool,
    /// The live page over the stage (worker-5's controller, our own instance).
    pub live: crate::fullscreen::live::LiveCtl,
    /// What `sync_round` last looked at (snapshot version, design on stage): skip the scans when unchanged.
    synced: Option<(u64, Option<String>)>,
}

/// Ready designs not yet reviewed, oldest first.
pub fn queue_ids(s: &Snapshot) -> Vec<String> {
    let reviewed = s.reviewed_ids();
    let mut v: Vec<Arc<Design>> = s.designs().filter(|d| d.is_ready() && !reviewed.contains(&d.id)).collect();
    // Equal createdAt (one round lands in the same ms): left to right, like the round itself.
    v.sort_by(|a, b| a.created_at.total_cmp(&b.created_at).then(a.position.x.total_cmp(&b.position.x)).then(a.position.y.total_cmp(&b.position.y)));
    v.into_iter().map(|d| d.id.clone()).collect()
}

/// Designs still generating, oldest first.
pub fn generating(s: &Snapshot) -> Vec<Arc<Design>> {
    let mut v: Vec<Arc<Design>> = s.designs().filter(|d| d.status == DesignStatus::Generating).collect();
    v.sort_by(|a, b| a.created_at.total_cmp(&b.created_at));
    v
}

/// The designs of the round that produced `id`, in generation order (includes `id`): its run while one
/// exists, else designs made together from the same prompt (same space, parent, kind and instruction,
/// created within 10 s), left to right.
pub fn siblings_of(s: &Snapshot, id: &str) -> Vec<Arc<Design>> {
    let Some(d) = s.design(id) else { return Vec::new() };
    if let Some(run) = s.runs().find(|r| r.design_ids.iter().any(|x| x == id)) {
        return run.design_ids.iter().filter_map(|x| s.design(x)).collect();
    }
    let mut v: Vec<Arc<Design>> = s
        .designs_in(&d.space_id)
        .filter(|o| {
            o.parent_id == d.parent_id
                && o.kind == d.kind
                && o.instruction.as_deref().unwrap_or("") == d.instruction.as_deref().unwrap_or("")
                && (o.created_at - d.created_at).abs() < 10_000.0
        })
        .collect();
    v.sort_by(|a, b| a.position.x.total_cmp(&b.position.x).then(a.created_at.total_cmp(&b.created_at)));
    v
}

/// The lane (or run label) a design came from, for the queue rows.
pub fn lane_name(s: &Snapshot, id: &str) -> Option<String> {
    let run = s.runs().find(|r| r.design_ids.iter().any(|x| x == id))?;
    Some(s.lane(&run.id).map_or(run.label.clone(), |l| l.name.clone()))
}

/// The component a spoke explores: its own focus, else up through earlier rounds of the same spoke.
pub fn focus_of(s: &Snapshot, id: &str) -> Option<String> {
    let mut cur = s.design(id);
    for _ in 0..50 {
        let d = cur?;
        if let Some(f) = s.spoke_focus(&d.id) {
            return Some(f.to_string());
        }
        if d.kind != DesignKind::Spoke {
            return None;
        }
        cur = d.parent_id.as_deref().and_then(|p| s.design(p));
    }
    None
}

impl ReviewState {
    /// The design on stage: `current` while it exists, else the queue's head.
    pub fn effective(&self, s: &Snapshot, queue: &[String]) -> Option<String> {
        match &self.current {
            Some(id) if s.design(id).is_some() => Some(id.clone()),
            _ => queue.first().cloned(),
        }
    }

    /// Sibling numbers picked for the next round: overrides over the feedback's leading clause.
    pub fn selected(&self, siblings: usize) -> Vec<usize> {
        let parsed = select::parse_selection(&self.feedback.text, siblings);
        (1..=siblings).filter(|n| self.overrides.get(n).copied().unwrap_or(parsed.contains(n))).collect()
    }
}

pub fn open(s: &mut AppState, start: Option<String>) {
    let r = &mut s.features.review;
    r.feedback.focused = false;
    r.open = true;
    r.dock_open = false;
    r.current = start;
    if r.round_count == 0 {
        r.round_count = 3;
    }
}

/// Close; the live page is hidden in the same turn (it draws above everything).
pub fn close(s: &mut AppState) {
    s.features.review.open = false;
    s.features.review.current = None;
    s.features.review.live.hide();
}

/// Drive the stage's live page: the design on stage while review is open and it's ready. Its Esc closes,
/// ←/→ move through the round (as on the stage). Returns whether the view must be rebuilt.
pub fn poll_live(s: &mut AppState, clock: f64, tx: &mut Tx) -> bool {
    use crate::fullscreen::live::KeyAsk;
    let open = if s.features.review.open {
        let queue = queue_ids(s.snap());
        s.features.review.effective(s.snap(), &queue).filter(|id| s.snap().design(id).is_some_and(|d| d.is_ready()))
    } else {
        None
    };
    let snap = s.store().snapshot();
    let asks = s.features.review.live.poll(&snap, open.as_deref(), clock, tx);
    let changed = !asks.is_empty();
    for a in asks {
        match a {
            KeyAsk::Close => close(s),
            KeyAsk::Step(d) => move_in_strip(s, d as isize),
        }
    }
    changed
}

/// The queue item to show once `id` leaves the queue: the next one, else the previous one.
fn next_after(queue: &[String], id: &str) -> Option<String> {
    match queue.iter().position(|x| x == id) {
        Some(i) => queue.get(i + 1).or_else(|| i.checked_sub(1).and_then(|j| queue.get(j))).cloned(),
        None => queue.iter().find(|x| *x != id).cloned(),
    }
}

fn on_stage(s: &AppState) -> (Vec<String>, Option<String>) {
    let queue = queue_ids(s.snap());
    let cur = s.features.review.effective(s.snap(), &queue);
    (queue, cur)
}

/// J/K: next / previous in the queue.
pub fn go(s: &mut AppState, delta: isize) {
    let (queue, cur) = on_stage(s);
    if queue.is_empty() {
        return;
    }
    let next = match cur.as_ref().and_then(|c| queue.iter().position(|x| x == c)) {
        None => if delta > 0 { queue[0].clone() } else { queue[queue.len() - 1].clone() },
        Some(i) => queue[(i as isize + delta).clamp(0, queue.len() as isize - 1) as usize].clone(),
    };
    s.features.review.current = Some(next);
}

/// ←/→: through the siblings of this round (wraps).
pub fn move_in_strip(s: &mut AppState, delta: isize) {
    let (_, Some(cur)) = on_stage(s) else { return };
    let sib: Vec<String> = siblings_of(s.snap(), &cur).iter().map(|d| d.id.clone()).collect();
    let Some(i) = sib.iter().position(|x| *x == cur) else { return };
    let n = sib.len() as isize;
    s.features.review.current = Some(sib[((i as isize + delta).rem_euclid(n)) as usize].clone());
}

/// Done (↵): reviewed, move on.
pub fn done(s: &mut AppState) {
    let (queue, Some(cur)) = on_stage(s) else { return };
    let next = next_after(&queue, &cur);
    nav::apply(s, ops::set_reviewed(&[cur.as_str()], true), "review done");
    s.features.review.current = next;
}

/// Discard (X): delete the design; the notice offers Undo (puts the snapshot back).
pub fn discard(s: &mut AppState) {
    let (queue, Some(cur)) = on_stage(s) else { return };
    let Some(d) = s.snap().design(&cur) else { return };
    let next = next_after(&queue, &cur);
    if nav::apply(s, vec![Change::delete(RecordKind::Designs.path(&cur))], "review discard") {
        s.features.review.current = next;
        notify_with(s, &format!("Discarded \"{}\"", d.title), "", Some(NoticeAction::Restore(Box::new(Design::clone(&d)))));
    }
}

/// Star (S).
pub fn toggle_star(s: &mut AppState) {
    let (_, Some(cur)) = on_stage(s) else { return };
    let on = !s.snap().design(&cur).is_some_and(|d| d.is_starred());
    if let Some(c) = ops::set_starred(s.snap(), &cur, on) {
        nav::apply(s, vec![c], "review star");
    }
}

/// 1–9: toggle a sibling for the next round.
pub fn toggle_num(s: &mut AppState, n: usize) {
    let (_, Some(cur)) = on_stage(s) else { return };
    let count = siblings_of(s.snap(), &cur).len();
    if n == 0 || n > count {
        return;
    }
    let r = &mut s.features.review;
    let parsed = select::parse_selection(&r.feedback.text, count);
    let now = r.overrides.get(&n).copied().unwrap_or(parsed.contains(&n));
    r.overrides.insert(n, !now);
}

/// "Mark all reviewed", with Undo.
pub fn mark_all(s: &mut AppState) {
    let queue = queue_ids(s.snap());
    if queue.is_empty() {
        return;
    }
    let refs: Vec<&str> = queue.iter().map(String::as_str).collect();
    if nav::apply(s, ops::set_reviewed(&refs, true), "review all") {
        notify_with(s, &format!("Marked {} as reviewed", queue.len()), "", Some(NoticeAction::Unreview(queue)));
    }
}

/// "Show on canvas": go to the design's space with it selected and close.
pub fn show_on_canvas(s: &mut AppState) {
    let (_, Some(cur)) = on_stage(s) else { return };
    let Some(d) = s.snap().design(&cur) else { return };
    close(s);
    nav::enter_space(s, &d.space_id, Some(&d.id));
}

/// Keys while review is open (none while typing in the feedback). True when handled.
pub fn on_key(s: &mut AppState, ev: &rune::KeyEvent, _tx: &mut Tx) -> bool {
    use rune::Key;
    if !s.features.review.open {
        return false;
    }
    if s.features.review.feedback.focused && !(ev.mods.cmd && ev.key == rune::Key::Enter) {
        return false;
    }
    let m = ev.mods;
    if m.cmd && ev.key == rune::Key::Enter {
        another_round(s);
        return true;
    }
    if m.cmd || m.ctrl || m.alt {
        return false;
    }
    match &ev.key {
        Key::Down => go(s, 1),
        Key::Up => go(s, -1),
        Key::Right => move_in_strip(s, 1),
        Key::Left => move_in_strip(s, -1),
        Key::Enter => done(s),
        Key::Escape => close(s),
        Key::Text(t) => match t.as_str() {
            "j" | "J" => go(s, 1),
            "k" | "K" => go(s, -1),
            "s" | "S" => toggle_star(s),
            "x" | "X" => discard(s),
            "f" | "/" => s.features.review.feedback.focused = true,
            d if d.len() == 1 && d.as_bytes()[0].is_ascii_digit() && d != "0" => toggle_num(s, (d.as_bytes()[0] - b'0') as usize),
            _ => return false,
        },
        _ => return false,
    }
    true
}

/// Reset manual picks when the round on stage changes; true when it did.
pub fn sync_round(s: &mut AppState) -> bool {
    if !s.features.review.open {
        return false;
    }
    let seen = (s.snap().version, s.features.review.current.clone());
    if s.features.review.synced.as_ref() == Some(&seen) {
        return false;
    }
    s.features.review.synced = Some(seen);
    let (_, cur) = on_stage(s);
    let key = cur.map(|c| siblings_of(s.snap(), &c).iter().map(|d| d.id.as_str()).collect::<Vec<_>>().join(",")).unwrap_or_default();
    let r = &mut s.features.review;
    if r.round_key != key {
        r.round_key = key;
        r.overrides.clear();
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moves_on_to_next_else_previous() {
        let q: Vec<String> = ["a", "b", "c"].iter().map(|x| x.to_string()).collect();
        assert_eq!(next_after(&q, "b").as_deref(), Some("c"));
        assert_eq!(next_after(&q, "c").as_deref(), Some("b"));
        assert_eq!(next_after(&q, "z").as_deref(), Some("a"));
        assert_eq!(next_after(&q[..1], "a"), None);
    }
}

/// Open Annotate notes pinned on a design, as `request.annotations` wants them.
pub fn open_annotations(s: &Snapshot, id: &str) -> Option<serde_json::Value> {
    let notes = s.notes(id)?;
    let open: Vec<serde_json::Value> = notes
        .iter()
        .filter(|n| n.resolved != Some(true))
        .map(|n| {
            let element = match (n.element.is_empty(), n.element_path.is_empty()) {
                (false, false) => format!("{} (at `{}`)", n.element, n.element_path),
                (false, true) => n.element.clone(),
                _ => n.element_path.clone(),
            };
            let comment = match &n.selected_text {
                Some(t) if !t.is_empty() => format!("{} (re: \"{t}\")", n.comment),
                _ => n.comment.clone(),
            };
            serde_json::json!({ "element": element, "comment": comment })
        })
        .collect();
    (!open.is_empty()).then(|| serde_json::Value::Array(open))
}

/// "Another round" (⌘↵): new directions below each picked sibling (else the design on stage), steered by
/// the feedback minus its selection clause; then all of them count as reviewed and the stage moves on.
pub fn another_round(s: &mut AppState) {
    use crate::features::launch::{self, Launch, LaneInfo};
    let (queue, Some(cur)) = on_stage(s) else { return };
    let snap = s.store().snapshot();
    let sib = siblings_of(&snap, &cur);
    let picked: Vec<Arc<Design>> = s.features.review.selected(sib.len()).iter().filter_map(|n| sib.get(n - 1).cloned()).filter(|d| d.is_ready()).collect();
    let targets: Vec<Arc<Design>> = if picked.is_empty() { snap.design(&cur).filter(|d| d.is_ready()).into_iter().collect() } else { picked };
    if targets.is_empty() {
        return;
    }
    let stripped = select::strip_selection(&s.features.review.feedback.text);
    let instruction = if stripped.is_empty() { "Push this direction further: keep what works and explore fresh takes on it.".to_string() } else { stripped };
    let count = s.features.review.round_count.clamp(1, 5);
    let refs: Vec<&Design> = targets.iter().map(|d| d.as_ref()).collect();
    let origins = launch::below_origins(&snap, &refs, count);
    let lane = lane_name(&snap, &cur);
    let agent = match s.prefs.agent_choice {
        crate::app::prefs::AgentChoice::Claude => "claude",
        crate::app::prefs::AgentChoice::Codex => "codex",
    };
    for (t, origin) in targets.iter().zip(origins) {
        let num = sib.iter().position(|d| d.id == t.id).map_or(1, |i| i + 1);
        let prefix = lane.as_deref().map(|l| format!("{} ", l.trim_start_matches("Spoke · "))).unwrap_or_default();
        let name = format!("{prefix}#{num} → round");
        let focus = focus_of(&snap, &t.id);
        let mut request = VariationRequest {
            base_html: Some(t.html.clone()),
            base_title: Some(t.title.clone()),
            instruction: instruction.clone(),
            count,
            focus: focus.clone(),
            scope: (t.kind == DesignKind::Spoke).then(|| "component".to_string()),
            agent: Some(agent.into()),
            space_id: Some(t.space_id.clone()),
            ..Default::default()
        };
        if let Some(a) = open_annotations(&snap, &t.id) {
            request.extra.insert("annotations".into(), a);
        }
        launch::launch(s, Launch {
            space_id: t.space_id.clone(),
            parent_id: Some(t.id.clone()),
            kind: t.kind,
            origin: Some(origin),
            label: name.clone(),
            request,
            lane: Some(LaneInfo { lane_type: proto_core::LaneType::Round, name, focus, hub_id: snap.hub_of(&t.id).map(|h| h.id.clone()) }),
        });
    }
    let n = targets.len();
    crate::features::notify(
        s,
        &format!("Another round on {}", if n == 1 { "1 design".to_string() } else { format!("{n} designs") }),
        &format!("{} new directions will join the queue as they land.", n as u32 * count),
    );
    let next = next_after(&queue, &cur);
    let mut ids: Vec<&str> = vec![cur.as_str()];
    ids.extend(targets.iter().map(|d| d.id.as_str()));
    nav::apply(s, ops::set_reviewed(&ids, true), "review round");
    let r = &mut s.features.review;
    r.current = next;
    r.feedback = TextEdit::new("");
    r.overrides.clear();
}

#[cfg(test)]
mod round_tests {
    use crate::app::state::AppState;
    use crate::composer::Outgoing;

    #[test]
    fn another_round_queues_one_lane_per_pick_and_marks_reviewed() {
        let mut s = AppState::new();
        s.use_store(crate::features::scenes::fixture());
        super::open(&mut s, Some("zzv2".into()));
        s.features.review.feedback = rune::TextEdit::new("1 and 3, more contrast");
        super::another_round(&mut s);
        let gens: Vec<_> = s.composer.outbox.iter().filter_map(|o| match o {
            Outgoing::Generate { parent_id, request, origin, run_id, .. } => Some((parent_id.clone(), request.instruction.clone(), *origin, run_id.clone())),
            _ => None,
        }).collect();
        assert_eq!(gens.len(), 2);
        assert_eq!(gens[0].0.as_deref(), Some("zzv1"));
        assert_eq!(gens[1].0.as_deref(), Some("zzv3"));
        assert!(gens.iter().all(|g| g.1 == "more contrast" && g.2.is_some()));
        let snap = s.snap();
        let run = gens[0].3.as_deref().unwrap();
        assert_eq!(snap.lane(run).unwrap().name, "#1 → round");
        assert!(snap.is_reviewed("zzv1") && snap.is_reviewed("zzv2") && snap.is_reviewed("zzv3"));
        assert!(s.features.review.feedback.text.is_empty());
    }
}
