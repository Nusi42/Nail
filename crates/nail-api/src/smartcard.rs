//! Smartcards and security tokens over PKCS#11 (M4c, ADR 0094).
//!
//! The user configures PKCS#11 modules (the vendor's DLL, e.g. OpenSC's
//! `opensc-pkcs11.dll`) in [`SmartcardSettings`]. The core lists the tokens
//! and their keys / certificates ([`crate::Request::ListSmartcards`]); a
//! key on a card is added to the keyring with
//! [`crate::Request::UseSmartcardKey`] and then signs / decrypts OpenPGP
//! and S/MIME mail like any own key — the private key never leaves the
//! card ([`crate::security::SecretStorage::Smartcard`]).
//!
//! # PIN prompts
//!
//! When an operation needs the card's PIN the core sends
//! [`crate::Event::PinRequested`] to the desktop UI and waits (at most
//! [`PIN_PROMPT_TIMEOUT_SECS`]) for [`crate::Request::AnswerPinPrompt`];
//! `pin: null` cancels. A wrong PIN asks again with `retry` set and the
//! card's remaining attempts. PINs are never stored, logged or sent to
//! remote sessions; smartcards are desktop-only.
//! [`crate::Event::PinPromptClosed`] closes a dialog that is no longer
//! needed (answered elsewhere, timed out, card removed).

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::engine::BoxFuture;
use crate::error::ApiError;
use crate::ids::{KeyId, PinPromptId};

/// How long the core waits for a PIN (seconds).
pub const PIN_PROMPT_TIMEOUT_SECS: u64 = 120;

/// Shortest PIN passed to a card.
pub const MIN_PIN_CHARS: usize = 4;

/// Longest PIN passed to a card.
pub const MAX_PIN_CHARS: usize = 64;

/// Longest the core may keep a card logged in after a PIN (minutes).
pub const MAX_PIN_CACHE_MINUTES: u32 = 480;

/// A configured PKCS#11 module.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct Pkcs11Module {
    /// Absolute path of the module (DLL).
    pub path: String,
    /// A name for display; `null` = the module's own description.
    #[serde(default)]
    pub name: Option<String>,
    /// Whether Nail loads it.
    #[serde(default = "yes")]
    pub enabled: bool,
}

fn yes() -> bool {
    true
}

/// Smartcard settings (desktop only; stored by the core).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct SmartcardSettings {
    /// The modules, in the order they are asked.
    pub modules: Vec<Pkcs11Module>,
    /// Keep a card logged in this many minutes after the PIN was entered
    /// (0 = ask for every operation; at most [`MAX_PIN_CACHE_MINUTES`]).
    pub pin_cache_minutes: u32,
}

labeled_enum! {
    /// Why smartcard settings are refused (`smartcard-settings-issue-*`).
    pub enum SmartcardSettingsIssue : "smartcard-settings-issue" {
        /// A module path is not absolute.
        RelativePath = "relative-path",
        /// A module listed twice.
        DuplicateModule = "duplicate-module",
        /// The PIN cache is longer than [`MAX_PIN_CACHE_MINUTES`].
        CacheTooLong = "cache-too-long",
    }
}

fn is_absolute(path: &str) -> bool {
    let b = path.as_bytes();
    path.starts_with('/')
        || path.starts_with(r"\\")
        || (b.len() >= 3
            && b[0].is_ascii_alphabetic()
            && b[1] == b':'
            && (b[2] == b'\\' || b[2] == b'/'))
}

impl SmartcardSettings {
    /// Problems (empty = valid).
    pub fn issues(&self) -> Vec<SmartcardSettingsIssue> {
        let mut issues = Vec::new();
        if self.modules.iter().any(|m| !is_absolute(&m.path)) {
            issues.push(SmartcardSettingsIssue::RelativePath);
        }
        let mut seen: Vec<String> = Vec::new();
        for module in &self.modules {
            let key = module.path.to_lowercase();
            if seen.contains(&key) {
                issues.push(SmartcardSettingsIssue::DuplicateModule);
                break;
            }
            seen.push(key);
        }
        if self.pin_cache_minutes > MAX_PIN_CACHE_MINUTES {
            issues.push(SmartcardSettingsIssue::CacheTooLong);
        }
        issues
    }
}

labeled_enum! {
    /// What a token object is (`smartcard-object-*`).
    pub enum SmartcardObjectKind : "smartcard-object" {
        /// A private key (usable, never readable).
        PrivateKey = "private-key",
        /// An X.509 certificate.
        Certificate = "certificate",
        /// A public key without a certificate (OpenPGP cards).
        PublicKey = "public-key",
    }
}

labeled_enum! {
    /// What a key on a card may do (`smartcard-usage-*`).
    pub enum SmartcardKeyUsage : "smartcard-usage" {
        /// Sign.
        Sign = "sign",
        /// Decrypt / unwrap.
        Decrypt = "decrypt",
    }
}

/// Facts of a certificate on a token.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct SmartcardCertificate {
    /// Subject distinguished name.
    pub subject: String,
    /// Issuer distinguished name.
    pub issuer: String,
    /// Serial number (uppercase hex).
    pub serial: String,
    /// Start of validity (RFC 3339).
    pub not_before: String,
    /// End of validity (RFC 3339).
    pub not_after: String,
    /// Addresses (subject alternative names, lowercase).
    #[serde(default)]
    pub emails: Vec<String>,
    /// SHA-256 of the DER certificate (uppercase hex) — the S/MIME
    /// fingerprint Nail uses.
    pub sha256: String,
}

/// A key or certificate on a token.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct SmartcardObject {
    /// What it is.
    pub kind: SmartcardObjectKind,
    /// `CKA_ID` (lowercase hex) — keys and their certificates share it.
    pub id: String,
    /// `CKA_LABEL`.
    #[serde(default)]
    pub label: String,
    /// Algorithm, e.g. `RSA 2048`, `ECDSA P-256`, `Ed25519`.
    #[serde(default)]
    pub algorithm: Option<String>,
    /// What a private key may do.
    #[serde(default)]
    pub usages: Vec<SmartcardKeyUsage>,
    /// Certificate facts (certificates, and keys with a matching one).
    #[serde(default)]
    pub certificate: Option<SmartcardCertificate>,
    /// OpenPGP fingerprint, when the card is an OpenPGP card and the key
    /// matches a known OpenPGP key.
    #[serde(default)]
    pub openpgp_fingerprint: Option<String>,
    /// The keyring entry already using this key, if any.
    #[serde(default)]
    pub key_id: Option<KeyId>,
}

/// A token (a card in a reader, a USB key).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct SmartcardToken {
    /// The module path it was found through.
    pub module: String,
    /// PKCS#11 slot id.
    pub slot_id: u64,
    /// Token label.
    pub label: String,
    /// Manufacturer.
    #[serde(default)]
    pub manufacturer: String,
    /// Model.
    #[serde(default)]
    pub model: String,
    /// Serial number (stable; with the module it identifies the token).
    pub serial: String,
    /// Whether a login (PIN) is needed for private keys.
    #[serde(default)]
    pub login_required: bool,
    /// The card says few PIN attempts are left.
    #[serde(default)]
    pub pin_count_low: bool,
    /// The PIN is locked (too many wrong tries; needs the PUK / vendor tool).
    #[serde(default)]
    pub pin_locked: bool,
    /// Its keys and certificates (public objects; private keys are listed
    /// without a login when the token allows it).
    #[serde(default)]
    pub objects: Vec<SmartcardObject>,
}

/// A stable reference to a key on a token, stored with keyring entries
/// that live on a card.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
pub struct SmartcardKeyRef {
    /// The module path.
    pub module: String,
    /// The token's serial number.
    pub token_serial: String,
    /// The key's `CKA_ID` (lowercase hex).
    pub object_id: String,
}

impl SmartcardKeyRef {
    /// The RFC 7512 `pkcs11:` URI of the key (serial and id, percent
    /// encoded), for logs and diagnostics.
    pub fn uri(&self) -> String {
        let mut id = String::new();
        let bytes = self.object_id.as_bytes();
        for pair in bytes.chunks(2) {
            id.push('%');
            for &b in pair {
                id.push(b.to_ascii_uppercase() as char);
            }
        }
        let serial: String = self
            .token_serial
            .bytes()
            .map(|b| {
                if b.is_ascii_alphanumeric() || b == b'-' || b == b'.' || b == b'_' {
                    (b as char).to_string()
                } else {
                    format!("%{b:02X}")
                }
            })
            .collect();
        format!("pkcs11:serial={serial};id={id};type=private")
    }
}

labeled_enum! {
    /// What a PIN is needed for (`pin-purpose-*`).
    pub enum PinPurpose : "pin-purpose" {
        /// Listing private objects / logging in.
        Login = "login",
        /// Signing a message.
        Sign = "sign",
        /// Decrypting a message.
        Decrypt = "decrypt",
    }
}

/// A PIN prompt shown by the desktop UI ([`crate::Event::PinRequested`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct PinPrompt {
    /// The prompt (answer with [`crate::Request::AnswerPinPrompt`]).
    pub prompt_id: PinPromptId,
    /// The token's label.
    pub token_label: String,
    /// The token's serial number.
    pub token_serial: String,
    /// What for.
    pub purpose: PinPurpose,
    /// The key's label or certificate subject, if known.
    #[serde(default)]
    pub key_label: Option<String>,
    /// Attempts left, when the card tells.
    #[serde(default)]
    pub attempts_left: Option<u32>,
    /// The previous PIN was wrong.
    #[serde(default)]
    pub retry: bool,
    /// When the core stops waiting (RFC 3339).
    pub expires_at: String,
}

/// What a crypto backend asks the core for (Rust only): the core assigns
/// the prompt id and expiry, shows [`PinPrompt`] and returns the PIN.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PinRequest {
    /// The token's label.
    pub token_label: String,
    /// The token's serial number.
    pub token_serial: String,
    /// What for.
    pub purpose: PinPurpose,
    /// The key's label, if known.
    pub key_label: Option<String>,
    /// Attempts left, when the card tells.
    pub attempts_left: Option<u32>,
    /// The previous PIN was wrong.
    pub retry: bool,
}

/// Asks the user for a token PIN (implemented by the core over events;
/// used by `nail-pkcs11`). `Ok(None)` = cancelled or timed out; the caller
/// must not keep the PIN beyond the login.
pub trait PinSource: Send + Sync + std::fmt::Debug {
    /// Ask once.
    fn request_pin(&self, request: PinRequest) -> BoxFuture<'_, Result<Option<String>, ApiError>>;
}

labeled_enum! {
    /// Why a PIN dialog closed without the user's answer
    /// (`pin-prompt-closed-*`).
    pub enum PinPromptCloseReason : "pin-prompt-closed" {
        /// Answered in another window.
        Answered = "answered",
        /// The core stopped waiting.
        TimedOut = "timed-out",
        /// The token was removed.
        TokenRemoved = "token-removed",
        /// The operation was cancelled.
        Cancelled = "cancelled",
    }
}

labeled_enum! {
    /// Smartcard failures (`smartcard-error-*`, the `localized` texts of
    /// errors from smartcard requests and crypto operations).
    pub enum SmartcardError : "smartcard-error" {
        /// The module file does not exist.
        ModuleNotFound = "module-not-found",
        /// The module could not be loaded or initialized (wrong
        /// architecture, missing dependencies).
        ModuleLoadFailed = "module-load-failed",
        /// No token in the reader / the token was removed.
        TokenNotPresent = "token-not-present",
        /// The PIN was wrong.
        PinIncorrect = "pin-incorrect",
        /// The PIN is locked.
        PinLocked = "pin-locked",
        /// The user cancelled the PIN prompt (or it timed out).
        PinCancelled = "pin-cancelled",
        /// The key is not on the token (another card inserted).
        KeyNotFound = "key-not-found",
        /// The token cannot do the needed operation.
        MechanismUnsupported = "mechanism-unsupported",
        /// The device reported an error.
        DeviceError = "device-error",
    }
}

/// Whether a typed PIN may be passed to a card (length within
/// [`MIN_PIN_CHARS`]..=[`MAX_PIN_CHARS`], no control characters); the
/// card decides the rest.
pub fn pin_is_plausible(pin: &str) -> bool {
    let chars = pin.chars().count();
    (MIN_PIN_CHARS..=MAX_PIN_CHARS).contains(&chars) && !pin.chars().any(char::is_control)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::labels::assert_labels;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    #[test]
    fn labels_match_the_wire() {
        assert_labels(
            SmartcardSettingsIssue::ALL,
            SmartcardSettingsIssue::as_str,
            SmartcardSettingsIssue::message_id,
        );
        assert_labels(
            SmartcardObjectKind::ALL,
            SmartcardObjectKind::as_str,
            SmartcardObjectKind::message_id,
        );
        assert_labels(
            SmartcardKeyUsage::ALL,
            SmartcardKeyUsage::as_str,
            SmartcardKeyUsage::message_id,
        );
        assert_labels(PinPurpose::ALL, PinPurpose::as_str, PinPurpose::message_id);
        assert_labels(
            PinPromptCloseReason::ALL,
            PinPromptCloseReason::as_str,
            PinPromptCloseReason::message_id,
        );
        assert_labels(
            SmartcardError::ALL,
            SmartcardError::as_str,
            SmartcardError::message_id,
        );
    }

    #[test]
    fn settings_validation() {
        let mut settings = SmartcardSettings::default();
        assert!(settings.issues().is_empty());
        settings.modules.push(Pkcs11Module {
            path: r"C:\Program Files\OpenSC Project\OpenSC\pkcs11\opensc-pkcs11.dll".into(),
            name: None,
            enabled: true,
        });
        assert!(settings.issues().is_empty());
        settings.modules.push(Pkcs11Module {
            path: r"c:\program files\opensc project\opensc\pkcs11\OPENSC-PKCS11.dll".into(),
            name: None,
            enabled: false,
        });
        settings.modules.push(Pkcs11Module {
            path: "opensc.dll".into(),
            name: None,
            enabled: true,
        });
        settings.pin_cache_minutes = MAX_PIN_CACHE_MINUTES + 1;
        assert_eq!(
            settings.issues(),
            vec![
                SmartcardSettingsIssue::RelativePath,
                SmartcardSettingsIssue::DuplicateModule,
                SmartcardSettingsIssue::CacheTooLong
            ]
        );
        let parsed: Pkcs11Module =
            serde_json::from_value(json!({"path": "/usr/lib/x.so"})).unwrap();
        assert!(parsed.enabled);
    }

    #[test]
    fn key_ref_uri() {
        let key = SmartcardKeyRef {
            module: "m".into(),
            token_serial: "0006 1234".into(),
            object_id: "0a1b".into(),
        };
        assert_eq!(
            key.uri(),
            "pkcs11:serial=0006%201234;id=%0A%1B;type=private"
        );
    }

    #[test]
    fn pins() {
        assert!(pin_is_plausible("123456"));
        assert!(!pin_is_plausible("123"));
        assert!(!pin_is_plausible("1234\n"));
        assert!(!pin_is_plausible(&"1".repeat(MAX_PIN_CHARS + 1)));
    }

    #[test]
    fn prompt_wire_shape() {
        let prompt = PinPrompt {
            prompt_id: "p1".into(),
            token_label: "OpenPGP card".into(),
            token_serial: "000612345678".into(),
            purpose: PinPurpose::Sign,
            key_label: Some("Signature key".into()),
            attempts_left: Some(3),
            retry: false,
            expires_at: "2026-10-02T10:02:00Z".into(),
        };
        assert_eq!(
            serde_json::to_value(&prompt).unwrap(),
            json!({
                "prompt_id": "p1", "token_label": "OpenPGP card",
                "token_serial": "000612345678", "purpose": "sign",
                "key_label": "Signature key", "attempts_left": 3, "retry": false,
                "expires_at": "2026-10-02T10:02:00Z"
            })
        );
    }
}
