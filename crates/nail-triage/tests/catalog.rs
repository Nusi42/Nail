//! Every Fluent id the crate produces exists in its English catalog: the
//! triage texts in `locales/en/core-triage.ftl`, the reminder scheduler's
//! in the PIM texts (`locales/en/core-pim.ftl`); the German files exist
//! without definitions (per-key English fallback).
#![cfg(test)]

use std::path::PathBuf;

use nail_api::localized::is_message_id;
use nail_triage::reminders::ReminderError;
use nail_triage::snooze::SnoozeError;

fn catalog(locale: &str, file: &str) -> std::io::Result<String> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../locales")
        .join(locale)
        .join(file);
    std::fs::read_to_string(path)
}

fn defined(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|l| l.split_once(" =").map(|(id, _)| id.to_owned()))
        .filter(|id| is_message_id(id))
        .collect()
}

/// Checks that the errors' ids are among `ids`, every one defined in the
/// English `file` and none in the German one; returns the English ids.
fn check(file: &str, ids: &[&str], errors: &[&str]) -> Vec<String> {
    let en = defined(&catalog("en", file).unwrap());
    for id in ids {
        assert!(is_message_id(id), "{id}");
        assert!(
            en.iter().any(|d| d == id),
            "{id} missing in locales/en/{file}"
        );
    }
    for id in errors {
        assert!(ids.contains(id), "{id}");
    }
    assert_eq!(defined(&catalog("de", file).unwrap()).len(), 0);
    en
}

#[test]
fn produced_ids_exist_in_english() {
    check(
        "core-triage.ftl",
        nail_triage::FLUENT_IDS,
        &[
            SnoozeError::<()>::NotInFuture.message_id(),
            SnoozeError::<()>::TooFar.message_id(),
            SnoozeError::Store(()).message_id(),
        ],
    );
}

#[test]
fn reminder_ids_exist_in_english() {
    let en = check(
        "core-pim.ftl",
        nail_triage::reminders::FLUENT_IDS,
        &[
            ReminderError::<()>::NotInFuture.message_id(),
            ReminderError::<()>::TooFar.message_id(),
            ReminderError::Store(()).message_id(),
        ],
    );
    // The Collected book's name (nail-store) lives there too.
    assert!(en.iter().any(|d| d == "pim-address-book-collected"));
}
