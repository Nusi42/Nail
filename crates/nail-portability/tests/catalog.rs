//! Every Fluent id this crate emits exists in
//! `locales/en/core-portability.ftl`, and the German catalog is the same
//! file (texts later).

use std::path::Path;

use nail_portability::PortabilityError;
use nail_portability::backup::restore::{NOTE_ACCOUNTS_NEED_PASSWORDS, NOTE_PLUGINS_REVIEW};
use nail_portability::import::DELETED_IN_SOURCE_ID;

fn catalog(lang: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../locales")
        .join(lang)
        .join("core-portability.ftl");
    std::fs::read_to_string(path).unwrap()
}

fn defines(text: &str, id: &str) -> bool {
    text.lines().any(|line| {
        line.strip_prefix(id)
            .is_some_and(|rest| rest.trim_start().starts_with('='))
    })
}

#[test]
fn every_emitted_id_has_an_english_text() {
    let english = catalog("en");
    let mut ids: Vec<String> = PortabilityError::examples()
        .iter()
        .map(|e| e.localized().id)
        .collect();
    ids.extend(
        [
            DELETED_IN_SOURCE_ID,
            NOTE_ACCOUNTS_NEED_PASSWORDS,
            NOTE_PLUGINS_REVIEW,
        ]
        .map(str::to_owned),
    );
    assert_eq!(ids.len(), 14);
    for id in ids {
        assert!(
            defines(&english, &id),
            "{id} is missing in locales/en/core-portability.ftl"
        );
    }
    // The plural note takes its count.
    assert!(english.contains("restore-note-accounts-need-passwords = { $count ->"));
}

#[test]
fn the_german_catalog_exists() {
    let german = catalog("de");
    assert!(
        !defines(&german, "portability-error-io"),
        "German texts come later"
    );
}
