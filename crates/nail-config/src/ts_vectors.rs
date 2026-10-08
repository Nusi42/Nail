//! Test vectors for the UI's port of `nail-config` rules (wave Q2, see
//! `nail_api::ts_vectors`): theme ids from names (`theme_edit::id_from_name`,
//! the UI's `themeIdFromName`). Regenerate with `NAIL_UPDATE_SNAPSHOTS=1
//! cargo test -p nail-config ts_vectors`.

use nail_api::ts_vectors::Vectors;
use serde_json::json;

#[test]
fn theme_id_vectors() {
    let mut v = Vectors::default();
    let long = "Long name ".repeat(10);
    for name in [
        "My Ocean!",
        "Café",
        "2024",
        "",
        "   ",
        "!!!",
        "Ünïcödé Thème",
        "a--b__c",
        " trailing - ",
        "ﬁne ligature",
        "日本語",
        long.as_str(),
    ] {
        v.case(
            "themeIdFromName",
            json!([name]),
            crate::theme_edit::id_from_name(name),
        );
    }
    v.check("theme-id");
}
