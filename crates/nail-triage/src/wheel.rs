//! A hierarchical timer wheel with one-second ticks.
//!
//! Six levels of 64 slots cover 64⁶ s (about 2,000 years) around the
//! current time, so every Unix-seconds deadline has a slot. An entry sits
//! on the level of the highest base-64 digit in which its deadline differs
//! from the wheel's time (the scheme of the Linux and Tokio wheels): all
//! entries of a lower level are due before any entry of a higher one, so
//! the next deadline is found by looking at one slot per level. When time
//! reaches a slot of a higher level its entries cascade to lower levels.
//!
//! The wheel has no clock: [`TimerWheel::advance`] is told the time.
//! Time never runs backwards in the wheel (an earlier time is ignored),
//! and deadlines at or before the wheel's time are due at the next
//! [`TimerWheel::advance`].

use std::collections::HashMap;
use std::hash::Hash;

const LEVELS: usize = 6;
const SLOT_BITS: u32 = 6;
const SLOTS: usize = 1 << SLOT_BITS;
const SLOT_MASK: u64 = (SLOTS as u64) - 1;
/// The wheel covers one aligned block of 64⁶ s; deadlines in a later block
/// wait in an overflow list until time reaches their block.
const BLOCK_MASK: u64 = (1u64 << SHIFTS[LEVELS]) - 1;
/// `SHIFTS[level]` is the bit offset of a level's base-64 digit
/// (`SLOT_BITS * level`); `SHIFTS[LEVELS]` is the width of a block.
const SHIFTS: [u32; LEVELS + 1] = {
    let mut shifts = [0; LEVELS + 1];
    let (mut level, mut shift) = (0, 0);
    while level <= LEVELS {
        shifts[level] = shift;
        shift += SLOT_BITS;
        level += 1;
    }
    shifts
};

#[derive(Debug, Clone)]
struct Entry<K> {
    key: K,
    deadline: i64,
    generation: u64,
}

/// A timer wheel of keys with deadlines (Unix seconds); one deadline per
/// key.
#[derive(Debug, Clone)]
pub struct TimerWheel<K> {
    elapsed: u64,
    levels: Vec<Vec<Vec<Entry<K>>>>,
    occupied: [u64; LEVELS],
    /// The authoritative deadline and generation per key; slot entries
    /// with another generation are stale (removed or rescheduled).
    index: HashMap<K, (i64, u64)>,
    /// Entries already due when inserted.
    ready: Vec<Entry<K>>,
    /// Entries beyond the current block (all later than the wheel's).
    overflow: Vec<Entry<K>>,
    next_generation: u64,
}

impl<K: Clone + Eq + Hash + Ord> TimerWheel<K> {
    /// An empty wheel standing at `now` (Unix seconds).
    pub fn new(now: i64) -> Self {
        Self {
            elapsed: clamp_time(now),
            levels: (0..LEVELS)
                .map(|_| (0..SLOTS).map(|_| Vec::new()).collect())
                .collect(),
            occupied: [0; LEVELS],
            index: HashMap::new(),
            ready: Vec::new(),
            overflow: Vec::new(),
            next_generation: 0,
        }
    }

    /// The wheel's time.
    pub fn now(&self) -> i64 {
        i64::try_from(self.elapsed).unwrap_or(i64::MAX)
    }

    /// Number of scheduled keys.
    pub fn len(&self) -> usize {
        self.index.len()
    }

    /// Whether nothing is scheduled.
    pub fn is_empty(&self) -> bool {
        self.index.is_empty()
    }

    /// The deadline of a key.
    pub fn deadline(&self, key: &K) -> Option<i64> {
        self.index.get(key).map(|(d, _)| *d)
    }

    /// Schedules (or reschedules) a key.
    pub fn insert(&mut self, key: K, deadline: i64) {
        let generation = self.next_generation;
        self.next_generation += 1;
        self.index.insert(key.clone(), (deadline, generation));
        self.place(Entry {
            key,
            deadline,
            generation,
        });
    }

    /// Unschedules a key. Returns its deadline.
    pub fn remove(&mut self, key: &K) -> Option<i64> {
        // Slot entries become stale and are dropped when reached.
        self.index.remove(key).map(|(d, _)| d)
    }

    fn is_live(&self, e: &Entry<K>) -> bool {
        self.index.get(&e.key) == Some(&(e.deadline, e.generation))
    }

    fn place(&mut self, entry: Entry<K>) {
        let when = clamp_time(entry.deadline);
        if when <= self.elapsed {
            self.ready.push(entry);
            return;
        }
        if when > self.elapsed | BLOCK_MASK {
            self.overflow.push(entry);
            return;
        }
        let level = level_for(self.elapsed, when);
        let slot = ((when >> SHIFTS[level]) & SLOT_MASK) as usize;
        self.levels[level][slot].push(entry);
        self.occupied[level] |= 1 << slot;
    }

    /// Start time of the next occupied slot of a level (slots before the
    /// current digit cannot be occupied: see the module docs).
    fn next_slot(&self, level: usize) -> Option<(u64, usize)> {
        let shift = SHIFTS[level];
        let current = ((self.elapsed >> shift) & SLOT_MASK) as usize;
        let mask = self.occupied[level] & (u64::MAX << current);
        if mask == 0 {
            return None;
        }
        let slot = mask.trailing_zeros() as usize;
        let block = SHIFTS[level + 1];
        let base = if block >= 64 {
            0
        } else {
            (self.elapsed >> block) << block
        };
        Some((base + ((slot as u64) << shift), slot))
    }

    /// The earliest deadline (exact), `None` when empty. The core sleeps
    /// until then.
    pub fn next_deadline(&self) -> Option<i64> {
        let mut best: Option<i64> = self
            .ready
            .iter()
            .filter(|e| self.is_live(e))
            .map(|e| e.deadline)
            .min();
        for (level, &shift) in SHIFTS.iter().enumerate().take(LEVELS) {
            let current = ((self.elapsed >> shift) & SLOT_MASK) as usize;
            let mut mask = self.occupied[level] & (u64::MAX << current);
            // The first slot with a live entry holds the level's minimum.
            while mask != 0 {
                let slot = mask.trailing_zeros() as usize;
                mask &= mask - 1;
                if let Some(min) = self.levels[level][slot]
                    .iter()
                    .filter(|e| self.is_live(e))
                    .map(|e| e.deadline)
                    .min()
                {
                    best = Some(best.map_or(min, |b| b.min(min)));
                    return best;
                }
            }
        }
        let overflow = self
            .overflow
            .iter()
            .filter(|e| self.is_live(e))
            .map(|e| e.deadline)
            .min();
        best.or(overflow)
    }

    /// Moves the wheel to `now` and returns the keys that became due,
    /// earliest first (ties by key), with their deadlines.
    pub fn advance(&mut self, now: i64) -> Vec<(K, i64)> {
        let now = clamp_time(now).max(self.elapsed);
        let mut due: Vec<Entry<K>> = std::mem::take(&mut self.ready);
        loop {
            // The earliest slot start over all levels.
            let next = (0..LEVELS)
                .filter_map(|l| self.next_slot(l).map(|(start, slot)| (start, l, slot)))
                .min();
            let Some((start, level, slot)) = next.filter(|(start, ..)| *start <= now) else {
                // Every slot up to `now` is processed; when `now` lies in a
                // later block, move there and bring its overflow entries in.
                let block_end = self.elapsed | BLOCK_MASK;
                if block_end < now && !self.overflow.is_empty() {
                    self.elapsed = block_end + 1;
                    for entry in std::mem::take(&mut self.overflow) {
                        if self.is_live(&entry) {
                            self.place(entry);
                        }
                    }
                    due.append(&mut self.ready);
                    continue;
                }
                break;
            };
            self.elapsed = self.elapsed.max(start);
            self.occupied[level] &= !(1 << slot);
            let entries = std::mem::take(&mut self.levels[level][slot]);
            for entry in entries {
                if clamp_time(entry.deadline) <= now {
                    due.push(entry);
                } else {
                    self.place(entry);
                }
            }
            // Entries placed while cascading can land in `ready` only when
            // due at `elapsed` ≤ `now`.
            due.append(&mut self.ready);
        }
        self.elapsed = now;
        let mut out: Vec<(K, i64)> = Vec::new();
        for entry in due {
            if self.is_live(&entry) {
                self.index.remove(&entry.key);
                out.push((entry.key, entry.deadline));
            }
        }
        out.sort_by(|a, b| (a.1, &a.0).cmp(&(b.1, &b.0)));
        out
    }
}

fn clamp_time(t: i64) -> u64 {
    u64::try_from(t).unwrap_or(0)
}

fn level_for(elapsed: u64, when: u64) -> usize {
    let masked = (elapsed ^ when) | SLOT_MASK;
    let significant = 63 - masked.leading_zeros() as usize;
    (significant / SLOT_BITS as usize).min(LEVELS - 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use proptest::prelude::*;
    use std::collections::BTreeMap;

    #[test]
    fn fires_in_order_and_reports_next_deadline() {
        let mut w = TimerWheel::new(1_000);
        assert!(w.is_empty());
        assert_eq!(w.next_deadline(), None);
        w.insert("c", 1_000 + 86_400 * 30);
        w.insert("a", 1_005);
        w.insert("b", 1_070);
        assert_eq!(w.len(), 3);
        assert_eq!(w.next_deadline(), Some(1_005));
        assert!(w.advance(1_004).is_empty());
        assert_eq!(w.advance(1_005), [("a", 1_005)]);
        assert_eq!(w.next_deadline(), Some(1_070));
        assert_eq!(w.advance(5_000), [("b", 1_070)]);
        assert_eq!(w.next_deadline(), Some(1_000 + 86_400 * 30));
        assert_eq!(w.advance(1_000 + 86_400 * 30 - 1), []);
        assert_eq!(w.advance(i64::MAX / 2), [("c", 1_000 + 86_400 * 30)]);
        assert!(w.is_empty());
    }

    #[test]
    fn past_deadlines_fire_at_the_next_advance() {
        let mut w = TimerWheel::new(500);
        w.insert(1u32, 100);
        w.insert(2u32, -5);
        w.insert(3u32, 500);
        assert_eq!(w.next_deadline(), Some(-5));
        assert_eq!(w.advance(500), [(2, -5), (1, 100), (3, 500)]);
    }

    #[test]
    fn remove_and_reschedule() {
        let mut w = TimerWheel::new(0);
        w.insert("x", 100);
        w.insert("y", 200);
        assert_eq!(w.remove(&"x"), Some(100));
        assert_eq!(w.remove(&"x"), None);
        w.insert("y", 50);
        assert_eq!(w.deadline(&"y"), Some(50));
        assert_eq!(w.next_deadline(), Some(50));
        // Re-inserting with the same deadline does not fire twice.
        w.insert("y", 50);
        assert_eq!(w.advance(1_000), [("y", 50)]);
        assert!(w.advance(10_000).is_empty());
    }

    #[test]
    fn time_does_not_run_backwards() {
        let mut w = TimerWheel::new(1_000);
        w.insert("a", 1_010);
        assert!(w.advance(10).is_empty());
        assert_eq!(w.now(), 1_000);
        assert_eq!(w.advance(1_010), [("a", 1_010)]);
    }

    #[test]
    fn far_future_deadlines_are_kept() {
        let mut w = TimerWheel::new(0);
        let far = i64::try_from(BLOCK_MASK).unwrap() * 3;
        w.insert("far", far);
        assert_eq!(w.advance(far - 1), []);
        assert_eq!(w.deadline(&"far"), Some(far));
        assert_eq!(w.advance(far), [("far", far)]);
    }

    #[test]
    fn deadlines_across_a_block_boundary() {
        let edge = i64::try_from(BLOCK_MASK + 1).unwrap();
        let mut w = TimerWheel::new(edge - 10);
        w.insert("before", edge - 3);
        w.insert("after", edge + 5);
        w.insert("later", edge * 2 + 7);
        assert_eq!(w.next_deadline(), Some(edge - 3));
        assert_eq!(w.advance(edge), [("before", edge - 3)]);
        assert_eq!(w.next_deadline(), Some(edge + 5));
        assert_eq!(w.advance(edge + 4), []);
        assert_eq!(w.advance(edge + 5), [("after", edge + 5)]);
        assert_eq!(w.next_deadline(), Some(edge * 2 + 7));
        assert_eq!(w.advance(edge * 2 + 6), []);
        assert_eq!(w.advance(edge * 3), [("later", edge * 2 + 7)]);
    }

    #[derive(Debug, Clone)]
    enum Step {
        Insert(u8, i64),
        Remove(u8),
        Advance(i64),
    }

    fn step() -> impl Strategy<Value = Step> {
        prop_oneof![
            (
                0u8..20,
                prop_oneof![0i64..200, 0i64..100_000, 0i64..50_000_000]
            )
                .prop_map(|(k, d)| Step::Insert(k, d)),
            (0u8..20).prop_map(Step::Remove),
            prop_oneof![0i64..100, 0i64..10_000, 0i64..5_000_000].prop_map(Step::Advance),
        ]
    }

    proptest! {
        /// The wheel behaves like a sorted map of deadlines.
        #[test]
        fn matches_a_naive_model(start in 0i64..2_000_000_000, steps in prop::collection::vec(step(), 1..120)) {
            let mut wheel = TimerWheel::new(start);
            let mut model: BTreeMap<u8, i64> = BTreeMap::new();
            let mut now = start;
            for s in steps {
                match s {
                    Step::Insert(k, d) => {
                        wheel.insert(k, now - 50 + d);
                        model.insert(k, now - 50 + d);
                    }
                    Step::Remove(k) => {
                        prop_assert_eq!(wheel.remove(&k), model.remove(&k));
                    }
                    Step::Advance(by) => {
                        now += by;
                        let fired = wheel.advance(now);
                        let mut want: Vec<(u8, i64)> =
                            model.iter().filter(|(_, d)| **d <= now).map(|(k, d)| (*k, *d)).collect();
                        want.sort_by_key(|a| (a.1, a.0));
                        model.retain(|_, d| *d > now);
                        prop_assert_eq!(fired, want);
                    }
                }
                prop_assert_eq!(wheel.next_deadline(), model.values().min().copied());
                prop_assert_eq!(wheel.len(), model.len());
            }
        }
    }
}
