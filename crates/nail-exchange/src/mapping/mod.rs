//! The mapping between Exchange and Nail concepts (plan M3c WP93).
//!
//! Pure functions, no I/O: the core (WP95) calls them between the engines
//! (`ExchangeEngine`, which speak the contract's Exchange shapes) and the
//! store / UI (Nail's model).
//!
//! | Module | Exchange | Nail |
//! | --- | --- | --- |
//! | [`categories`] | master categories, item categories | tags with colours |
//! | [`focus`] | `inferenceClassification` | `MessageSummary.exchange.inference`, `focus:` |
//! | [`rules`] | inbox rules | `rules.kdl` rules placed on the server where expressible |
//! | [`oof`] | automatic replies | the generic vacation settings |
//! | [`events`] | events, patterns, exceptions | events, `RRULE`s, overrides |
//! | [`tasks`] | To Do tasks, checklists / EWS tasks | tasks, subtasks |
//! | [`contacts`] | contacts, distribution lists | contacts, groups |
//! | [`rooms`] | rooms, room lists, availability | the free/busy picker's rooms |
//!
//! Lossy spots are reported as notes ([`nail_api::localized::LocalizedMessage`]
//! with `exchange-*` ids) rather than dropped silently.

pub mod categories;
pub mod contacts;
pub mod events;
pub mod focus;
pub mod oof;
pub mod rooms;
pub mod rules;
pub mod tasks;
pub mod text;
pub mod time;
pub mod windows_zones;

use nail_api::exchange::rules::Importance;

/// A stable `UID` for an Exchange item that has none (To Do tasks,
/// contacts): derived from its remote id so every sync gives the same one.
pub fn derived_uid(kind: &str, remote_id: &str) -> String {
    let hash = nail_ical::ical::sha256_hex(remote_id.as_bytes());
    format!("{kind}-{}@exchange.nail", &hash[..32])
}

/// iCalendar `PRIORITY` of an Exchange importance (high = 1, normal = 5,
/// low = 9).
pub fn priority_of(importance: Option<Importance>) -> Option<u8> {
    importance.map(|i| match i {
        Importance::High => 1,
        Importance::Normal => 5,
        Importance::Low => 9,
    })
}

/// The importance of an iCalendar `PRIORITY` (1–4 high, 5 normal, 6–9
/// low, undefined → none).
pub fn importance_of(priority: Option<u8>) -> Option<Importance> {
    match priority? {
        0 => None,
        1..=4 => Some(Importance::High),
        5 => Some(Importance::Normal),
        _ => Some(Importance::Low),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uids_are_stable_and_distinct() {
        let a = derived_uid("task", "AAkALgAA");
        assert_eq!(a, derived_uid("task", "AAkALgAA"));
        assert_ne!(a, derived_uid("task", "AAkALgAB"));
        assert!(a.starts_with("task-") && a.ends_with("@exchange.nail"));
    }

    #[test]
    fn priorities() {
        for importance in [Importance::High, Importance::Normal, Importance::Low] {
            assert_eq!(
                importance_of(priority_of(Some(importance))),
                Some(importance)
            );
        }
        assert_eq!(importance_of(Some(3)), Some(Importance::High));
        assert_eq!(importance_of(Some(7)), Some(Importance::Low));
        assert_eq!(importance_of(Some(0)), None);
        assert_eq!(importance_of(None), None);
        assert_eq!(priority_of(None), None);
    }
}
