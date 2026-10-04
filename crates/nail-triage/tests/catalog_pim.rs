//! Every Fluent id the reminder scheduler produces exists in the English
//! catalog of PIM texts (`locales/en/core-pim.ftl`), and the German file
//! exists without definitions.
#![cfg(test)]

use std::path::PathBuf;

use nail_api::localized::is_message_id;
use nail_triage::reminders::{FLUENT_IDS, ReminderError};

fn catalog(locale: &str) -> std::io::Result<String> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../locales")
        .join(locale)
        .join("core-pim.ftl");
    std::fs::read_to_string(path)
}

fn defined(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|l| l.split_once(" =").map(|(id, _)| id.to_owned()))
        .filter(|id| is_message_id(id))
        .collect()
}

#[test]
fn reminder_ids_exist_in_english() {
    let en = defined(&catalog("en").unwrap());
    for id in FLUENT_IDS {
        assert!(is_message_id(id), "{id}");
        assert!(
            en.iter().any(|d| d == id),
            "{id} missing in locales/en/core-pim.ftl"
        );
    }
    for e in [
        ReminderError::<()>::NotInFuture,
        ReminderError::TooFar,
        ReminderError::Store(()),
    ] {
        assert!(FLUENT_IDS.contains(&e.message_id()), "{}", e.message_id());
    }
    // The Collected book's name (nail-store) lives there too.
    assert!(en.iter().any(|d| d == "pim-address-book-collected"));
    assert!(defined(&catalog("de").unwrap()).is_empty());
}
