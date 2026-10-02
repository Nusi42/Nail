//! Widgets of M4b WP114 (the Windows desktop app's own settings; contract
//! ADR 0087). Rendered by `ui/src/features/system/`.
//!
//! Slot created by WP110 as future widgets (any options accepted); WP114
//! gives them typed options. The first-run screen and the "Send to"
//! composer notice are dialogs / mounted parts, not widgets.

use super::{WidgetSpec, future_widget};

/// Widgets of this feature.
pub(crate) const WIDGETS: &[WidgetSpec] = &[
    // Version, installer, channel, check now, release notes, download /
    // restart to install, the policy-disabled state ("About" + updates).
    future_widget("update-settings", false),
    // Close to tray, start hidden, start with Windows, the `nail plugin
    // dev` endpoint.
    future_widget("desktop-settings", false),
    // Default apps (mailto / webcal / .eml / .ics / .vcf, Simple MAPI) and
    // the button to Settings → Default apps.
    future_widget("default-apps", false),
];

#[cfg(test)]
mod tests {
    #[test]
    fn system_widgets_are_registered_and_parse() {
        for spec in super::WIDGETS {
            assert!(crate::kdl_util::is_kebab(spec.kind), "{}", spec.kind);
            assert!(crate::widgets::find(spec.kind).is_some());
        }
        let text = "layout \"x\" extends=\"default\" {
    window \"main\" {
        stack {
            update-settings
            desktop-settings
            default-apps
        }
    }
}
";
        crate::parse_layout(text, "layout.kdl").unwrap();
    }
}
