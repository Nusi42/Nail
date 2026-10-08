//! Widgets of M4b WP114 (the Windows desktop app's own settings; contract
//! ADR 0087, UI ADR 0091). Rendered by `ui/src/features/system/`.
//!
//! None takes `row` children; the options are listed in
//! `userDocs/en/desktop-app.md#layout-options`. The first-run screen and
//! "About Nail" are dialogs, and the "Send to" composer notice is mounted
//! inside the composer, so they need no widget of their own.

use super::OptionType::Bool;
use super::{WidgetSpec, widget};

/// Widgets of this feature.
pub(crate) const WIDGETS: &[WidgetSpec] = &[
    // Version, installer, channel, check now, release notes, download /
    // restart to install, the policy-disabled state. `show-installation`
    // (default #true): the version and installer line.
    widget(
        "update-settings",
        &[("show-installation", Bool)],
        &[],
        false,
    ),
    // Close to tray, start hidden, start with Windows, the `nail plugin
    // dev` endpoint. `show-dev-endpoint` (default #true).
    widget(
        "desktop-settings",
        &[("show-dev-endpoint", Bool)],
        &[],
        false,
    ),
    // Default apps (mailto / webcal / .eml / .ics / .vcf, Simple MAPI) and
    // the button to Settings → Default apps. `show-mapi` (default #true).
    widget("default-apps", &[("show-mapi", Bool)], &[], false),
];

#[cfg(test)]
mod tests {
    use crate::widgets::testing::parses;

    #[test]
    fn system_widgets_parse() {
        assert!(parses(&[
            "update-settings show-installation=#false",
            "desktop-settings show-dev-endpoint=#false",
            "default-apps show-mapi=#true",
        ]));
        for widget in ["update-settings", "desktop-settings", "default-apps"] {
            assert!(
                !parses(&[&format!("{widget} show-everything=#true")]),
                "{widget}"
            );
        }
    }
}
