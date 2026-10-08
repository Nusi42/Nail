//! A small thread-safe map whose entries may expire: the unlock caches of
//! the OpenPGP and S/MIME keyrings (secrets in memory until a deadline or
//! the end of the process), one definition for both.

use std::collections::HashMap;
use std::fmt;
use std::sync::{Mutex, MutexGuard, PoisonError};

/// Values by key, each with an optional deadline of type `T` (an `Instant`,
/// Unix seconds, …); an entry is gone once `now` reaches its deadline.
pub struct ExpiringMap<V, T> {
    entries: Mutex<HashMap<String, (V, Option<T>)>>,
}

impl<V, T> Default for ExpiringMap<V, T> {
    fn default() -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
        }
    }
}

/// Shows the number of entries only (the values are often secrets).
impl<V, T> fmt::Debug for ExpiringMap<V, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ExpiringMap")
            .field("entries", &self.len())
            .finish()
    }
}

impl<V: Clone, T: PartialOrd> ExpiringMap<V, T> {
    /// The value of `key` unless it expired at `now` (an expired entry is
    /// removed).
    pub fn get(&self, key: &str, now: &T) -> Option<V> {
        let mut entries = self.lock();
        match entries.get(key) {
            Some((_, Some(until))) if until <= now => {
                entries.remove(key);
                None
            }
            Some((value, _)) => Some(value.clone()),
            None => None,
        }
    }
}

impl<V, T> ExpiringMap<V, T> {
    /// Keeps `value` for `key` until `until` (`None` = no deadline).
    pub fn insert(&self, key: &str, value: V, until: Option<T>) {
        self.lock().insert(key.to_owned(), (value, until));
    }

    /// Forgets `key`.
    pub fn remove(&self, key: &str) {
        self.lock().remove(key);
    }

    /// Forgets every entry.
    pub fn clear(&self) {
        self.lock().clear();
    }

    /// The number of entries (expired ones included until they are read).
    pub fn len(&self) -> usize {
        self.lock().len()
    }

    /// Whether there are no entries.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The entries, also after a panic of another holder of the lock.
    fn lock(&self) -> MutexGuard<'_, HashMap<String, (V, Option<T>)>> {
        self.entries.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entries_expire_at_their_deadline() {
        let map = ExpiringMap::<String, i64>::default();
        assert!(map.is_empty());
        map.insert("a", "one".to_owned(), Some(10));
        map.insert("b", "two".to_owned(), None);
        assert_eq!(map.get("a", &9).as_deref(), Some("one"));
        assert_eq!(map.get("a", &10), None, "gone at the deadline");
        assert_eq!(map.len(), 1, "the expired entry was removed");
        assert_eq!(map.get("b", &i64::MAX).as_deref(), Some("two"));
        map.remove("b");
        assert_eq!(map.get("b", &0), None);
        map.insert("c", "three".to_owned(), None);
        map.clear();
        assert!(map.is_empty());
        assert_eq!(format!("{map:?}"), "ExpiringMap { entries: 0 }");
    }
}
