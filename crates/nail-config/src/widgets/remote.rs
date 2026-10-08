//! Widgets of M2b WP43 (remote UI and privacy UI, ADRs 0036 and 0039):
//! remote-UI settings, users and credentials, devices and sessions, the
//! security audit view, the reader's "load remote content" bar and the
//! privacy allow-lists. Rendered by `ui/src/features/remote/`.

use super::{WidgetSpec, widget};

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
    use crate::widgets::testing::{assert_prefixed, parses};

    #[test]
    fn remote_widgets_parse() {
        assert_prefixed(super::WIDGETS, &["remote-"]);
        let kinds: Vec<&str> = super::WIDGETS.iter().map(|w| w.kind).collect();
        assert!(parses(&kinds));
        assert!(!parses(&["remote-audit bogus=#true"]));
    }
}
