//! Every Fluent id the crate produces exists in the English catalog
//! (`locales/en/core-triage.ftl`), and the German file exists.
#![cfg(test)]

use std::path::PathBuf;

use nail_api::localized::is_message_id;
use nail_triage::FLUENT_IDS;
use nail_triage::snooze::SnoozeError;

fn catalog(locale: &str) -> std::io::Result<String> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../locales")
        .join(locale)
        .join("core-triage.ftl");
    std::fs::read_to_string(path)
}

fn defined(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|l| l.split_once(" =").map(|(id, _)| id.to_owned()))
        .filter(|id| is_message_id(id))
        .collect()
}

#[test]
fn produced_ids_exist_in_english() {
    let en = defined(&catalog("en").unwrap());
    for id in FLUENT_IDS {
        assert!(is_message_id(id), "{id}");
        assert!(
            en.iter().any(|d| d == id),
            "{id} missing in locales/en/core-triage.ftl"
        );
    }
    for e in [
        SnoozeError::<()>::NotInFuture,
        SnoozeError::TooFar,
        SnoozeError::Store(()),
    ] {
        assert!(FLUENT_IDS.contains(&e.message_id()), "{}", e.message_id());
    }
    // German stays empty for now (per-key English fallback).
    assert!(defined(&catalog("de").unwrap()).is_empty());
}
