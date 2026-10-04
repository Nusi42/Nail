//! Widgets of M2b WP43 (remote UI and privacy UI, ADRs 0036 and 0039):
//! remote-UI settings, users and credentials, devices and sessions, the
//! security audit view, the reader's "load remote content" bar and the
//! privacy allow-lists. Rendered by `ui/src/features/remote/`.

use super::WidgetSpec;
#[expect(
    unused_imports,
    reason = "the table's helpers stay imported for the package that adds widgets (ADR 0022)"
)]
use super::{OptionType, future_widget, widget};

/// Widgets of this feature (none takes options).
///
/// `remote-settings`: Settings → Remote access (enable, bind address,
/// TLS, allowed networks, session limits, status and certificate);
/// `remote-users`: users and their credentials (password, TOTP,
/// passkeys); `remote-devices`: devices, sessions and pending pairing
/// codes; `remote-audit`: the security log; `remote-content`: the
/// reader bar for the selected message; `remote-content-allows`:
/// Settings → Privacy (remote-content allow-lists).
pub(crate) const WIDGETS: &[WidgetSpec] = &[
    widget("remote-settings", &[], &[], false),
    widget("remote-users", &[], &[], false),
    widget("remote-devices", &[], &[], false),
    widget("remote-audit", &[], &[], false),
    widget("remote-content", &[], &[], false),
    widget("remote-content-allows", &[], &[], false),
];

#[cfg(test)]
mod tests {
    #[test]
    fn remote_widgets_are_unique_and_kebab_case() {
        for spec in super::WIDGETS {
            assert!(crate::kdl_util::is_kebab(spec.kind), "{}", spec.kind);
            assert!(spec.kind.starts_with("remote-"), "{}", spec.kind);
            assert!(crate::widgets::find(spec.kind).is_some());
        }
    }

    #[test]
    fn remote_widgets_are_registered() {
        let kinds = super::WIDGETS
            .iter()
            .map(|w| ["            ", w.kind, "\n"].concat())
            .collect::<String>();
        let text = format!(
            "layout \"x\" extends=\"default\" {{\n    window \"main\" {{\n        stack {{\n{kinds}        }}\n    }}\n}}\n"
        );
        crate::parse_layout(&text, "layout.kdl").unwrap();
        let bad = text.replace("remote-audit", "remote-audit bogus=#true");
        assert!(crate::parse_layout(&bad, "layout.kdl").is_err());
    }
}
