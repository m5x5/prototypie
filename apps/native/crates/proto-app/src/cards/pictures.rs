//! The card pictures the app holds, bounded (runtime health, M4): an LRU over entry count and
//! decoded bytes. proto-web keeps its own LRU, but every picture held here keeps its pixel buffer
//! alive however proto-web evicts, so without a bound this grows with every space visited.
//!
//! Eviction order: pictures of designs outside the active space first, then the active space's,
//! least recently used first. Pinned pictures (cards in view, open views) are never evicted: an
//! evicted visible card would ask again at once and push out another, round and round.

use std::collections::HashMap;

/// Entry cap (below proto-web's `SnapshotLimits` 400).
pub const MAX_ENTRIES: usize = 300;
/// Decoded-bytes cap (below proto-web's 384 MB).
pub const MAX_BYTES: usize = 256 << 20;

struct Slot<T> {
    value: T,
    bytes: usize,
    used: u64,
}

pub struct Pictures<T> {
    slots: HashMap<String, Slot<T>>,
    bytes: usize,
    tick: u64,
    max_entries: usize,
    max_bytes: usize,
}

impl<T> Default for Pictures<T> {
    fn default() -> Self {
        Pictures::new(MAX_ENTRIES, MAX_BYTES)
    }
}

impl<T> Pictures<T> {
    pub fn new(max_entries: usize, max_bytes: usize) -> Self {
        Pictures { slots: HashMap::new(), bytes: 0, tick: 0, max_entries, max_bytes }
    }

    pub fn get(&self, id: &str) -> Option<&T> {
        self.slots.get(id).map(|s| &s.value)
    }

    /// Marks `id` as just used (a card in view, a view showing it).
    pub fn touch(&mut self, id: &str) {
        self.tick += 1;
        if let Some(s) = self.slots.get_mut(id) {
            s.used = self.tick;
        }
    }

    /// Stores `value` (`bytes` decoded) as `id`'s picture, replacing the old one.
    pub fn insert(&mut self, id: &str, value: T, bytes: usize) {
        self.tick += 1;
        let slot = Slot { value, bytes, used: self.tick };
        if let Some(old) = self.slots.insert(id.to_string(), slot) {
            self.bytes -= old.bytes;
        }
        self.bytes += bytes;
    }

    pub fn remove(&mut self, id: &str) -> Option<T> {
        let s = self.slots.remove(id)?;
        self.bytes -= s.bytes;
        Some(s.value)
    }

    /// (decoded bytes, entries).
    pub fn usage(&self) -> (usize, usize) {
        (self.bytes, self.slots.len())
    }

    /// Evicts until both caps hold (or only pinned pictures are left): designs outside the
    /// active space first, then the active space's, least recently used first. Returns the
    /// evicted ids.
    pub fn evict(&mut self, active: impl Fn(&str) -> bool, pinned: impl Fn(&str) -> bool) -> Vec<String> {
        if self.slots.len() <= self.max_entries && self.bytes <= self.max_bytes {
            return Vec::new();
        }
        let mut order: Vec<(bool, u64, String)> = self
            .slots
            .iter()
            .filter(|(id, _)| !pinned(id))
            .map(|(id, s)| (active(id), s.used, id.clone()))
            .collect();
        // Other spaces (false) before the active space (true), then oldest first.
        order.sort_unstable();
        let mut out = Vec::new();
        for (_, _, id) in order {
            if self.slots.len() <= self.max_entries && self.bytes <= self.max_bytes {
                break;
            }
            self.remove(&id);
            out.push(id);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::Pictures;

    const MB: usize = 1 << 20;

    #[test]
    fn entry_cap_evicts_other_spaces_first_then_least_recently_used() {
        let mut p = Pictures::new(3, usize::MAX);
        p.insert("a1", (), MB);
        p.insert("o1", (), MB);
        p.insert("a2", (), MB);
        p.insert("o2", (), MB);
        p.insert("a3", (), MB);
        p.touch("a1");
        let active = |id: &str| id.starts_with('a');
        // 5 entries, cap 3: both other-space pictures go, though a1/a2 are older than o2.
        assert_eq!(p.evict(active, |_| false), vec!["o1".to_string(), "o2".to_string()]);
        assert_eq!(p.usage(), (3 * MB, 3));
        // Then within the active space, least recently used (a1 was touched last-but-one).
        p.insert("a4", (), MB);
        assert_eq!(p.evict(active, |_| false), vec!["a2".to_string()]);
        assert!(p.get("a1").is_some());
    }

    #[test]
    fn byte_cap_holds_with_replacements_counted_once() {
        let mut p = Pictures::new(usize::MAX, 10 * MB);
        p.insert("a", (), 4 * MB);
        p.insert("a", (), 6 * MB); // A sharper picture replaces the old one: 6, not 10.
        p.insert("b", (), 3 * MB);
        assert_eq!(p.usage(), (9 * MB, 2));
        assert!(p.evict(|_| true, |_| false).is_empty());
        p.insert("c", (), 4 * MB);
        assert_eq!(p.evict(|_| true, |_| false), vec!["a".to_string()]);
        assert_eq!(p.usage(), (7 * MB, 2));
    }

    #[test]
    fn pinned_pictures_stay_even_over_the_cap() {
        let mut p = Pictures::new(1, usize::MAX);
        p.insert("seen1", (), MB);
        p.insert("seen2", (), MB);
        p.insert("old", (), MB);
        let pinned = |id: &str| id.starts_with("seen");
        assert_eq!(p.evict(|_| true, pinned), vec!["old".to_string()]);
        assert_eq!(p.usage().1, 2);
        assert!(p.evict(|_| true, pinned).is_empty());
    }
}
