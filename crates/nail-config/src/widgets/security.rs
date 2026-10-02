//! Widgets of M2a WP34 (security UI, ADRs 0029 and 0033): key and
//! certificate manager, per-identity crypto settings, composer sign /
//! encrypt toggles with recipient key status, the message security and
//! auth-results badges, reader warnings, encryption-at-rest settings and
//! the Autocrypt setup flow. Rendered by `ui/src/features/security/`.

use super::WidgetSpec;
#[allow(unused_imports)]
use super::{OptionType, widget};

/// Widgets of this feature (none takes options).
///
/// `security-keys`: key and certificate manager; `security-identities`:
/// per-identity crypto settings; `security-settings`: encryption at rest,
/// key lookup, decrypted indexing; `security-autocrypt`: setup message
/// send / import (the import dialog is a UI dialog, not a widget);
/// `security-message`: badges of the selected message;
/// `security-warnings`: link and attachment warnings of the selected
/// message; `security-composer`: sign / encrypt toggles with recipient
/// key status.
pub(crate) const WIDGETS: &[WidgetSpec] = &[
    widget("security-keys", &[], &[], false),
    widget("security-identities", &[], &[], false),
    widget("security-settings", &[], &[], false),
    widget("security-autocrypt", &[], &[], false),
    widget("security-message", &[], &[], false),
    widget("security-warnings", &[], &[], false),
    widget("security-composer", &[], &[], false),
    // M4c (WP124, ADR 0094): future widgets (any options) — the vault
    // recovery key (create / show once / verify / restore) and smartcards
    // (PKCS#11 modules, tokens, keys, PIN cache).
    widget("security-recovery-key", &[], &[], false),
    widget("security-smartcards", &[], &[], false),
];

#[cfg(test)]
mod tests {
    #[test]
    fn security_widgets_are_unique_and_kebab_case() {
        for spec in super::WIDGETS {
            assert!(crate::kdl_util::is_kebab(spec.kind), "{}", spec.kind);
            assert!(crate::widgets::find(spec.kind).is_some());
        }
    }

    #[test]
    fn security_widgets_are_registered() {
        let kinds = super::WIDGETS
            .iter()
            .map(|w| format!("            {}\n", w.kind))
            .collect::<String>();
        let text = format!(
            "layout \"x\" extends=\"default\" {{\n    window \"main\" {{\n        stack {{\n{kinds}        }}\n    }}\n}}\n"
        );
        crate::parse_layout(&text, "layout.kdl").unwrap();
        let bad = text.replace("security-keys", "security-keys bogus=#true");
        assert!(crate::parse_layout(&bad, "layout.kdl").is_err());
    }
}
