//! End-to-end encryption, sender authentication and data protection
//! (design.md, "Security & encryption"; plan M2a; ADR 0029).
//!
//! * **Message security** ([`SecurityStatus`]): what the core found when it
//!   read a message: signatures (OpenPGP or S/MIME) with signer, key,
//!   trust and certificate chain, encryption and whether it could decrypt,
//!   and header protection. Carried in [`crate::MessageBody::security`].
//! * **Sender authentication** ([`AuthResults`]): DKIM / SPF / DMARC / ARC
//!   verdicts from a trusted `Authentication-Results` header (RFC 8601),
//!   in [`crate::MessageBody::auth_results`].
//! * **Reader warnings** ([`MessageWarning`]): links whose text shows
//!   another target, look-alike (punycode) domains, dangerous attachment
//!   types; in [`crate::MessageBody::warnings`].
//! * **Keys and certificates** ([`CryptoKey`]): one keyring for OpenPGP
//!   keys and S/MIME certificates, with trust decisions, import / export,
//!   generation, lookup (WKD, keys.openpgp.org) and the Windows
//!   certificate store.
//! * **Sending** ([`SendCrypto`], [`IdentityCrypto`],
//!   [`RecipientKeyStatus`]): per-identity defaults, the composer's
//!   toggles and the key status of each recipient.
//! * **Data protection** ([`VaultStatus`], [`SecuritySettings`]):
//!   encryption at rest of the database, blobs and search index, and the
//!   global security settings.
//!
//! Wire conventions (ADR 0002 / 0012): kebab-case enums, `snake_case` fields,
//! `null` for absent values, opaque ids ([`crate::KeyId`]). Times are RFC
//! 3339 strings. Fingerprints are uppercase hexadecimal without spaces.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::error::ErrorCode;
use crate::ids::{AccountId, KeyId};
use crate::localized::LocalizedMessage;

// ---------------------------------------------------------------------------
// Technologies and trust
// ---------------------------------------------------------------------------

/// The two end-to-end standards.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum CryptoTechnology {
    /// OpenPGP (RFC 9580 / RFC 4880 keys, PGP/MIME RFC 3156).
    #[serde(rename = "openpgp")]
    OpenPgp,
    /// S/MIME 4.0 (RFC 8551, CMS).
    #[serde(rename = "smime")]
    Smime,
}

impl CryptoTechnology {
    /// Both technologies, OpenPGP first.
    pub const ALL: [CryptoTechnology; 2] = [Self::OpenPgp, Self::Smime];

    /// The wire name (`openpgp` / `smime`), also used in the store.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::OpenPgp => "openpgp",
            Self::Smime => "smime",
        }
    }
}

/// Which technology an identity prefers when both are possible.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum CryptoPreference {
    /// Whatever every recipient supports; OpenPGP when both work.
    #[default]
    Auto,
    /// OpenPGP.
    #[serde(rename = "openpgp")]
    OpenPgp,
    /// S/MIME.
    #[serde(rename = "smime")]
    Smime,
}

/// How far a key or certificate is trusted to belong to its addresses.
///
/// The effective level combines where the key came from with the user's
/// decision: a decision (`verified`, `rejected`) always wins; otherwise
/// the source decides (`autocrypt`, `autocrypt-gossip`, `unknown`); own
/// keys (with a secret part) are `own`. For S/MIME a valid chain to a
/// trusted root is reported in [`CertificateChain`], not here.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default, Serialize, Deserialize, TS,
)]
#[serde(rename_all = "kebab-case")]
pub enum TrustLevel {
    /// The user marked the key as not belonging to its addresses; it is
    /// never used for encryption and its signatures are not trusted.
    Rejected,
    /// Nothing is known (imported, looked up, or a valid S/MIME chain
    /// without a user decision).
    #[default]
    Unknown,
    /// Learned from an `Autocrypt-Gossip` header of a third party.
    AutocryptGossip,
    /// Learned from the peer's own `Autocrypt` header.
    Autocrypt,
    /// The user verified the fingerprint (or the certificate) in person or
    /// over another channel.
    Verified,
    /// One of the user's own keys.
    Own,
}

/// A trust decision the user can store ([`crate::Request::SetKeyTrust`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum TrustDecision {
    /// Forget the decision; the key's source decides again.
    Unset,
    /// The fingerprint was checked.
    Verified,
    /// The key does not belong to its addresses.
    Rejected,
}

impl TrustDecision {
    /// The wire / store name.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unset => "unset",
            Self::Verified => "verified",
            Self::Rejected => "rejected",
        }
    }
}

// ---------------------------------------------------------------------------
// Message security (read side)
// ---------------------------------------------------------------------------

/// End-to-end security of one message, as found when reading it. Absent
/// from [`crate::MessageBody::security`] when the message is neither
/// signed nor encrypted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct SecurityStatus {
    /// The technology of the outermost protection.
    pub technology: CryptoTechnology,
    /// Signatures found (usually one). Empty for encrypted-only mail.
    #[serde(default)]
    pub signatures: Vec<SignatureInfo>,
    /// Encryption, `null` for signed-only mail.
    #[serde(default)]
    pub encryption: Option<EncryptionInfo>,
    /// Header protection of an encrypted or signed message.
    #[serde(default)]
    pub header_protection: HeaderProtection,
    /// Only a part of the message is signed or encrypted (e.g. a mailing
    /// list appended a footer, or several inline PGP blocks). The UI must
    /// not show the whole message as protected.
    #[serde(default)]
    pub partial: bool,
    /// The protection is inline PGP (`-----BEGIN PGP MESSAGE-----` in a
    /// text part), which is read but never produced.
    #[serde(default)]
    pub inline_pgp: bool,
}

impl SecurityStatus {
    /// Whether at least one signature was found.
    pub fn is_signed(&self) -> bool {
        !self.signatures.is_empty()
    }

    /// Whether the message was encrypted.
    pub fn is_encrypted(&self) -> bool {
        self.encryption.is_some()
    }

    /// Whether every signature is valid (and there is one).
    pub fn all_signatures_valid(&self) -> bool {
        self.is_signed()
            && self
                .signatures
                .iter()
                .all(|s| s.state == SignatureState::Valid)
    }
}

/// The outcome of checking one signature.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum SignatureState {
    /// Cryptographically valid with a usable key.
    Valid,
    /// The signature does not match the content (tampered or broken).
    Invalid,
    /// The signing key or certificate is not in the keyring.
    UnknownKey,
    /// Valid, but the key or certificate had expired when it signed.
    KeyExpired,
    /// Valid, but the key or certificate is revoked.
    KeyRevoked,
    /// The algorithm or format is not supported.
    Unsupported,
    /// The signature could not be checked (malformed data).
    Error,
}

/// One signature of a message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct SignatureInfo {
    /// The check's outcome.
    pub state: SignatureState,
    /// The signer's address from the key's user id or the certificate
    /// (`null` when the key is unknown).
    #[serde(default)]
    pub signer: Option<String>,
    /// Whether [`Self::signer`] matches the message's `From` address. A
    /// valid signature by someone else is shown as a warning.
    #[serde(default)]
    pub signer_matches_from: bool,
    /// The key or certificate in the keyring, `null` when unknown.
    #[serde(default)]
    pub key_id: Option<KeyId>,
    /// Fingerprint of the signing key (OpenPGP: the primary key; S/MIME:
    /// SHA-256 of the certificate), or the issuer key id when that is all
    /// the signature names.
    #[serde(default)]
    pub fingerprint: Option<String>,
    /// Trust in the signing key.
    #[serde(default)]
    pub trust: TrustLevel,
    /// Signature creation time, when the signature carries one.
    #[serde(default)]
    pub signed_at: Option<String>,
    /// S/MIME: the certificate chain; `null` for OpenPGP.
    #[serde(default)]
    pub chain: Option<CertificateChain>,
}

/// Whether and how an encrypted message was decrypted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum DecryptionState {
    /// Decrypted; the body shown is the decrypted content.
    Decrypted,
    /// None of the user's keys can decrypt it.
    NoKey,
    /// A matching secret key is protected by a passphrase that was not
    /// entered ([`crate::Request::UnlockKey`], then fetch again).
    Locked,
    /// Decryption failed (damaged data, integrity check failed).
    Failed,
    /// The algorithm or format is not supported.
    Unsupported,
}

/// Encryption of a message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct EncryptionInfo {
    /// Whether it could be decrypted.
    pub state: DecryptionState,
    /// The own key that decrypted it (or would, when `locked`).
    #[serde(default)]
    pub key_id: Option<KeyId>,
    /// Key ids / certificate identifiers the message is encrypted to, as
    /// the message names them (may be empty for hidden recipients).
    #[serde(default)]
    pub recipients: Vec<String>,
    /// The content cipher, e.g. `AES-256` or `AES-256-GCM`, when known.
    #[serde(default)]
    pub algorithm: Option<String>,
}

/// Header protection (protected subject and headers inside the
/// encrypted / signed part).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum HeaderProtection {
    /// The message carries no protected headers.
    #[default]
    None,
    /// Protected headers were found and used (e.g. the real subject
    /// replaced `...` on the wire).
    Protected,
    /// Protected headers differ from the outer headers in a way that is
    /// not explained by hiding (e.g. another `From`); the protected ones
    /// are shown and the UI warns.
    Mismatch,
}

/// Validation result of an S/MIME certificate chain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum ChainState {
    /// Chains to a root trusted by the system (Windows trust store).
    Valid,
    /// Chains to a root that is not trusted, or is self-signed.
    UntrustedRoot,
    /// An intermediate is missing.
    Incomplete,
    /// A certificate in the chain has expired (or is not yet valid).
    Expired,
    /// The certificate may not be used for e-mail (key usage / extended
    /// key usage).
    WrongUsage,
    /// A signature in the chain is invalid.
    Invalid,
}

/// Revocation checking of a certificate. M2 does not check revocation
/// (plan section 7, carried items); the UI says so.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum RevocationState {
    /// Not checked.
    #[default]
    NotChecked,
    /// Checked, not revoked.
    Good,
    /// Revoked.
    Revoked,
}

/// One certificate, for display.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct CertificateSummary {
    /// Subject distinguished name (RFC 4514 string).
    pub subject: String,
    /// Issuer distinguished name.
    pub issuer: String,
    /// Serial number, uppercase hex.
    pub serial: String,
    /// Start of validity.
    pub not_before: String,
    /// End of validity.
    pub not_after: String,
    /// SHA-256 fingerprint of the DER certificate.
    pub fingerprint: String,
}

/// An S/MIME certificate chain, leaf first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct CertificateChain {
    /// Overall result.
    pub state: ChainState,
    /// Revocation status of the leaf.
    #[serde(default)]
    pub revocation: RevocationState,
    /// The certificates, leaf first, root last (as far as they were found).
    pub certificates: Vec<CertificateSummary>,
}

// ---------------------------------------------------------------------------
// Sender authentication (Authentication-Results)
// ---------------------------------------------------------------------------

/// A result value of RFC 8601 (plus ARC's).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum AuthVerdict {
    /// Passed.
    Pass,
    /// Failed.
    Fail,
    /// SPF soft fail.
    Softfail,
    /// No policy statement.
    Neutral,
    /// Not checked / no result.
    None,
    /// Temporary error.
    Temperror,
    /// Permanent error.
    Permerror,
    /// DKIM: a valid signature the verifier's local policy does not accept.
    Policy,
    /// A value this version does not know.
    Unknown,
}

impl AuthVerdict {
    /// The verdict as the rules engine's [`crate::rules::AuthResult`];
    /// `None` for `policy` and `unknown`.
    pub fn as_rule_result(self) -> Option<crate::rules::AuthResult> {
        use crate::rules::AuthResult as R;
        Some(match self {
            Self::Pass => R::Pass,
            Self::Fail => R::Fail,
            Self::Softfail => R::Softfail,
            Self::Neutral => R::Neutral,
            Self::None => R::None,
            Self::Temperror => R::Temperror,
            Self::Permerror => R::Permerror,
            Self::Policy | Self::Unknown => return None,
        })
    }
}

/// One method's result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct AuthMethodResult {
    /// The verdict.
    pub verdict: AuthVerdict,
    /// The domain the result is about: DKIM `header.d`, SPF
    /// `smtp.mailfrom` (its domain), DMARC `header.from`.
    #[serde(default)]
    pub domain: Option<String>,
    /// DKIM `header.s` (selector).
    #[serde(default)]
    pub selector: Option<String>,
    /// Whether `domain` aligns with the `From` domain (relaxed alignment,
    /// as DMARC defines it); `null` when not computed.
    #[serde(default)]
    pub aligned: Option<bool>,
    /// The `reason=` text, if any (diagnostic, English).
    #[serde(default)]
    pub reason: Option<String>,
}

/// Sender authentication verdicts from `Authentication-Results`.
///
/// Only headers added by a trusted server count: the account's own
/// receiving server (its `authserv-id`, configured or learned). Headers
/// with another id are ignored, because anyone can write one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct AuthResults {
    /// The `authserv-id` of the header that was used.
    pub authserv_id: String,
    /// DKIM results, one per signature.
    #[serde(default)]
    pub dkim: Vec<AuthMethodResult>,
    /// SPF.
    #[serde(default)]
    pub spf: Option<AuthMethodResult>,
    /// DMARC.
    #[serde(default)]
    pub dmarc: Option<AuthMethodResult>,
    /// ARC chain validation.
    #[serde(default)]
    pub arc: Option<AuthMethodResult>,
}

impl AuthResults {
    /// The badge verdict: DMARC when present, else the best aligned DKIM
    /// result, else SPF; `none` when nothing was checked.
    pub fn summary(&self) -> AuthVerdict {
        if let Some(dmarc) = &self.dmarc {
            return dmarc.verdict;
        }
        if self.dkim.iter().any(|d| d.verdict == AuthVerdict::Pass) {
            return AuthVerdict::Pass;
        }
        if let Some(first) = self.dkim.first() {
            return first.verdict;
        }
        self.spf.as_ref().map_or(AuthVerdict::None, |s| s.verdict)
    }
}

// ---------------------------------------------------------------------------
// Reader warnings
// ---------------------------------------------------------------------------

/// Why an attachment is flagged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum AttachmentRisk {
    /// A program or script (`.exe`, `.js`, `.ps1`, `.bat`, `.msi`, …).
    Executable,
    /// An Office document that can contain macros (`.docm`, `.xlsm`, …).
    Macro,
    /// A name that hides its real type (`invoice.pdf.exe`, or a
    /// right-to-left override character).
    DisguisedName,
    /// A container that hides its content from scanners (`.iso`, `.img`,
    /// `.vhd`, encrypted archives).
    Container,
    /// A shortcut or link file (`.lnk`, `.url`, `.scf`).
    Shortcut,
    /// The declared MIME type does not match the file name.
    TypeMismatch,
}

/// A warning shown in the reader. Links are numbered in document order
/// (zero-based) among the `<a href>` elements of
/// [`crate::MessageBody::sanitized_html`], or among the URLs of the text
/// body for text-only mail.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum MessageWarning {
    /// A link's visible text is (or looks like) another address than its
    /// target.
    LinkMismatch {
        /// The link's number.
        link: u32,
        /// The real target.
        href: String,
        /// The host the text shows.
        shown_host: String,
        /// The host the link goes to.
        target_host: String,
    },
    /// A link goes to an internationalized domain that looks like another
    /// (mixed scripts, confusable characters).
    LookalikeDomain {
        /// The link's number.
        link: u32,
        /// The real target.
        href: String,
        /// The host as sent (punycode, `xn--…`).
        ascii_host: String,
        /// The host as displayed (Unicode).
        unicode_host: String,
        /// The ASCII domain it imitates, when known.
        #[serde(default)]
        resembles: Option<String>,
    },
    /// A link to a raw IP address or a non-web scheme (`file:`, `data:`,
    /// `javascript:` are removed by the sanitizer; this covers the
    /// remaining unusual ones).
    SuspiciousLink {
        /// The link's number.
        link: u32,
        /// The real target.
        href: String,
    },
    /// A dangerous attachment.
    DangerousAttachment {
        /// Index into [`crate::MessageBody::attachments`].
        index: u32,
        /// Its file name.
        filename: String,
        /// Why.
        risk: AttachmentRisk,
    },
}

// ---------------------------------------------------------------------------
// Keys and certificates
// ---------------------------------------------------------------------------

/// Where a key or certificate came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum KeySource {
    /// Generated by Nail.
    Generated,
    /// Imported from a file or pasted text.
    Imported,
    /// A PKCS#12 (`.p12` / `.pfx`) file.
    Pkcs12,
    /// The Windows certificate store (the private key stays there).
    WindowsStore,
    /// The peer's `Autocrypt` header.
    Autocrypt,
    /// An `Autocrypt-Gossip` header.
    AutocryptGossip,
    /// An Autocrypt setup message (own key from another device).
    SetupMessage,
    /// Web Key Directory lookup.
    Wkd,
    /// keys.openpgp.org lookup.
    Keyserver,
    /// A certificate taken from a signed S/MIME message.
    Message,
    /// A key on a smartcard / token (M4c; the private key stays there).
    Smartcard,
}

impl KeySource {
    /// The wire / store name.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Generated => "generated",
            Self::Imported => "imported",
            Self::Pkcs12 => "pkcs12",
            Self::WindowsStore => "windows-store",
            Self::Autocrypt => "autocrypt",
            Self::AutocryptGossip => "autocrypt-gossip",
            Self::SetupMessage => "setup-message",
            Self::Wkd => "wkd",
            Self::Keyserver => "keyserver",
            Self::Message => "message",
            Self::Smartcard => "smartcard",
        }
    }
}

/// How the secret part of an own key is protected at rest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum SecretStorage {
    /// Encrypted with a random key kept in the OS keychain (no prompt).
    Keychain,
    /// Encrypted with a passphrase the user enters
    /// ([`crate::Request::UnlockKey`]).
    Passphrase,
    /// Held by the Windows certificate store (CNG); Nail never sees it.
    WindowsStore,
    /// On a smartcard / token, used through PKCS#11 with a PIN (M4c,
    /// [`CryptoKey::smartcard`]); Nail never sees it.
    Smartcard,
}

/// How to protect a new secret key.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum SecretProtection {
    /// A random key in the OS keychain (the default).
    #[default]
    Keychain,
    /// A passphrase.
    Passphrase {
        /// The passphrase (never stored).
        passphrase: String,
    },
}

/// An OpenPGP key or S/MIME certificate in the keyring.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "independent properties of a key, shown in the keyring"
)]
pub struct CryptoKey {
    /// Opaque id.
    pub id: KeyId,
    /// OpenPGP or S/MIME.
    pub technology: CryptoTechnology,
    /// OpenPGP: primary key fingerprint; S/MIME: SHA-256 of the DER
    /// certificate. Uppercase hex.
    pub fingerprint: String,
    /// OpenPGP user ids (`Ada Lovelace <ada@example.org>`) or the S/MIME
    /// subject, primary first.
    pub user_ids: Vec<String>,
    /// Addresses the key is for (lowercase), from user ids or the
    /// certificate's subject alternative names.
    pub emails: Vec<String>,
    /// Public-key algorithm, e.g. `Ed25519`, `RSA 4096`, `ECDSA P-256`.
    pub algorithm: String,
    /// Creation time (OpenPGP) or start of validity (S/MIME).
    pub created_at: String,
    /// Expiry, `null` for keys that do not expire.
    #[serde(default)]
    pub expires_at: Option<String>,
    /// Whether it is expired now.
    #[serde(default)]
    pub expired: bool,
    /// Whether it is revoked (OpenPGP revocation signature).
    #[serde(default)]
    pub revoked: bool,
    /// Whether it can sign (a usable signing (sub)key / key usage).
    pub can_sign: bool,
    /// Whether it can encrypt.
    pub can_encrypt: bool,
    /// Whether the secret part is available (an own key).
    pub has_secret: bool,
    /// How the secret part is protected; `null` without one.
    #[serde(default)]
    pub secret_storage: Option<SecretStorage>,
    /// Where it came from.
    pub source: KeySource,
    /// Effective trust (see [`TrustLevel`]).
    pub trust: TrustLevel,
    /// S/MIME: issuer distinguished name; `null` for OpenPGP.
    #[serde(default)]
    pub issuer: Option<String>,
    /// S/MIME: serial number (uppercase hex); `null` for OpenPGP.
    #[serde(default)]
    pub serial: Option<String>,
    /// S/MIME: the chain as last validated; `null` for OpenPGP.
    #[serde(default)]
    pub chain: Option<CertificateChain>,
    /// Where the private key lives when it is on a smartcard (M4c,
    /// ADR 0094); absent otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub smartcard: Option<crate::smartcard::SmartcardKeyRef>,
}

/// Filters of [`crate::Request::ListKeys`].
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct KeyFilter {
    /// Only this technology; `null` = both.
    pub technology: Option<CryptoTechnology>,
    /// Case-insensitive substring of a user id, address or fingerprint;
    /// `null` = all.
    pub text: Option<String>,
    /// Only keys with a secret part (own keys).
    pub own_only: bool,
}

/// How key data in a request or response is encoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum KeyEncoding {
    /// Text: ASCII-armored OpenPGP or PEM certificates.
    #[default]
    Text,
    /// Base64 (standard alphabet, padded) of binary data (OpenPGP packets,
    /// DER certificates).
    Base64,
}

/// An item that could not be imported.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct KeyImportIssue {
    /// What it was (fingerprint or position), for display.
    pub item: String,
    /// Error class.
    pub code: ErrorCode,
    /// Diagnostic detail (English).
    pub message: String,
    /// The user-facing text (Fluent id + arguments) when the core has a more
    /// specific one than the error class; else `null` (ADR 0043).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub localized: Option<LocalizedMessage>,
}

/// Result of an import.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct KeyImportResult {
    /// Keys that are new or were updated, as stored now.
    pub keys: Vec<CryptoKey>,
    /// How many of them are new.
    pub created: u32,
    /// How many already existed and were merged (new user ids,
    /// signatures, revocations).
    pub updated: u32,
    /// Items that were skipped.
    #[serde(default)]
    pub issues: Vec<KeyImportIssue>,
}

/// Algorithm of a generated OpenPGP key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum KeyAlgorithm {
    /// Ed25519 signing primary key + X25519 encryption subkey (default).
    #[default]
    Curve25519,
    /// RSA 4096 primary + subkey, for old correspondents.
    Rsa4096,
}

/// Places a key lookup asks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum KeyLookupSource {
    /// Web Key Directory of the address's domain (advanced, then direct
    /// method).
    Wkd,
    /// keys.openpgp.org (verified addresses only).
    KeysOpenpgpOrg,
}

/// A key found by [`crate::Request::LookupKeys`], not imported yet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct KeyLookupCandidate {
    /// Where it was found.
    pub source: KeyLookupSource,
    /// Primary key fingerprint.
    pub fingerprint: String,
    /// User ids.
    pub user_ids: Vec<String>,
    /// Algorithm, e.g. `Ed25519`.
    pub algorithm: String,
    /// Creation time.
    pub created_at: String,
    /// Expiry, if any.
    #[serde(default)]
    pub expires_at: Option<String>,
    /// The key in the keyring with this fingerprint, if any.
    #[serde(default)]
    pub known: Option<KeyId>,
    /// ASCII-armored public key, for [`crate::Request::ImportKeys`].
    pub armored: String,
}

/// A source that failed during a lookup.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct KeyLookupFailure {
    /// The source.
    pub source: KeyLookupSource,
    /// Error class (`network`, `tls`, `not_found` is not a failure).
    pub code: ErrorCode,
    /// Diagnostic detail (English).
    pub message: String,
    /// The user-facing text (Fluent id + arguments) when the core has a more
    /// specific one than the error class; else `null` (ADR 0043).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub localized: Option<LocalizedMessage>,
}

/// Windows certificate stores Nail can read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum WindowsCertStore {
    /// Current user, "Personal" (`MY`): own certificates with private keys.
    Personal,
    /// Current user, "Other People" (`AddressBook`): correspondents.
    OtherPeople,
}

/// A certificate in a Windows certificate store.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct WindowsCertificate {
    /// The store.
    pub store: WindowsCertStore,
    /// SHA-1 thumbprint as Windows shows it (uppercase hex), used to
    /// address the certificate.
    pub thumbprint: String,
    /// Display details.
    pub certificate: CertificateSummary,
    /// E-mail addresses (subject alternative names, `E=`).
    pub emails: Vec<String>,
    /// Whether the store holds a private key for it.
    pub has_private_key: bool,
    /// Whether key usage allows S/MIME (digital signature / key
    /// encipherment, e-mail protection).
    pub usable_for_email: bool,
    /// The key in Nail's keyring for it, if already imported.
    #[serde(default)]
    pub imported: Option<KeyId>,
}

// ---------------------------------------------------------------------------
// Sending
// ---------------------------------------------------------------------------

/// Autocrypt `prefer-encrypt` (Autocrypt Level 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum AutocryptPreference {
    /// `nopreference`.
    #[default]
    #[serde(rename = "nopreference")]
    NoPreference,
    /// `mutual`: encrypt by default when both sides say so.
    Mutual,
}

/// An identity's end-to-end settings (`crypto` node of an identity in
/// `accounts.kdl`). `null` in [`crate::Identity::crypto`] means all
/// defaults.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "independent per-identity defaults (one checkbox each)"
)]
pub struct IdentityCrypto {
    /// Preferred technology.
    pub technology: CryptoPreference,
    /// Sign new messages by default.
    pub sign: bool,
    /// Encrypt new messages by default when every recipient has a usable
    /// key (the composer shows why when not).
    pub encrypt: bool,
    /// Own OpenPGP key (primary key fingerprint); `null` = the newest
    /// usable own key for the identity's address.
    pub openpgp_key: Option<String>,
    /// Own S/MIME certificate (SHA-256 fingerprint); `null` = the newest
    /// usable own certificate for the address.
    pub smime_certificate: Option<String>,
    /// Send `Autocrypt` headers and learn from received ones.
    pub autocrypt: bool,
    /// Autocrypt `prefer-encrypt`.
    pub prefer_encrypt: AutocryptPreference,
    /// Protect headers of encrypted mail (the subject becomes `...` on
    /// the wire).
    pub protect_headers: bool,
}

impl Default for IdentityCrypto {
    fn default() -> Self {
        Self {
            technology: CryptoPreference::Auto,
            sign: false,
            encrypt: false,
            openpgp_key: None,
            smime_certificate: None,
            autocrypt: true,
            prefer_encrypt: AutocryptPreference::NoPreference,
            protect_headers: true,
        }
    }
}

impl IdentityCrypto {
    /// Whether everything is at its default (the writer then omits the
    /// `crypto` node).
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

/// Sign / encrypt choice of one message ([`crate::Draft::crypto`] and
/// [`crate::Request::SendDraft`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct SendCrypto {
    /// Sign.
    pub sign: bool,
    /// Encrypt to every recipient (and to the sender's own key). Sending
    /// fails with `no_key` when a recipient has no usable key; it never
    /// falls back to plain text.
    pub encrypt: bool,
    /// The technology; `null` = the identity's preference, else what every
    /// recipient supports.
    #[serde(default)]
    pub technology: Option<CryptoTechnology>,
    /// Protect headers when encrypting; `null` = the identity's setting.
    #[serde(default)]
    pub protect_headers: Option<bool>,
}

impl SendCrypto {
    /// Neither sign nor encrypt.
    pub fn none() -> Self {
        Self {
            sign: false,
            encrypt: false,
            technology: None,
            protect_headers: None,
        }
    }

    /// Whether anything is requested.
    pub fn is_active(&self) -> bool {
        self.sign || self.encrypt
    }
}

/// Usability of a recipient's key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum RecipientKeyState {
    /// Usable for encryption.
    Usable,
    /// Expired.
    Expired,
    /// Revoked.
    Revoked,
    /// The user rejected it.
    Rejected,
    /// It cannot encrypt (no encryption subkey / key usage).
    CannotEncrypt,
    /// S/MIME: the chain does not validate.
    InvalidChain,
}

/// The best key of one technology for a recipient.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct RecipientKey {
    /// The key.
    pub key_id: KeyId,
    /// Its fingerprint.
    pub fingerprint: String,
    /// Usability.
    pub state: RecipientKeyState,
    /// Trust.
    pub trust: TrustLevel,
    /// Where it came from.
    pub source: KeySource,
}

/// Autocrypt peer state of a recipient (Autocrypt Level 1, per own
/// account).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct AutocryptPeer {
    /// The peer's address.
    pub email: String,
    /// When mail from the peer was last seen.
    pub last_seen: String,
    /// Time of the last `Autocrypt` header seen, `null` when only gossip.
    #[serde(default)]
    pub autocrypt_at: Option<String>,
    /// The peer's `prefer-encrypt`.
    pub prefer_encrypt: AutocryptPreference,
    /// Fingerprint of the peer's Autocrypt key, if any.
    #[serde(default)]
    pub key_fingerprint: Option<String>,
    /// Fingerprint of the last gossiped key, if any.
    #[serde(default)]
    pub gossip_fingerprint: Option<String>,
}

/// Autocrypt's recommendation for one message (Autocrypt Level 1,
/// "Provide a recommendation for message encryption").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum AutocryptRecommendation {
    /// Encryption is not possible.
    Disable,
    /// Possible, but likely to be unreadable (stale or gossip keys).
    Discourage,
    /// Possible; offer the toggle, off.
    Available,
    /// Both sides prefer encryption; turn it on.
    Encrypt,
}

/// Where a lookup for a recipient stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum KeyLookupState {
    /// No online lookup was made (not asked, or the setting is off).
    #[default]
    NotAttempted,
    /// A key was found online and imported.
    Found,
    /// Nothing was found online.
    NotFound,
    /// The lookup failed (offline, TLS).
    Failed,
}

/// Key status of one recipient while composing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct RecipientKeyStatus {
    /// The address (lowercase).
    pub email: String,
    /// Best OpenPGP key, if any.
    #[serde(default)]
    pub openpgp: Option<RecipientKey>,
    /// Best S/MIME certificate, if any.
    #[serde(default)]
    pub smime: Option<RecipientKey>,
    /// Autocrypt peer state, if any.
    #[serde(default)]
    pub autocrypt: Option<AutocryptPeer>,
    /// Online lookup.
    #[serde(default)]
    pub lookup: KeyLookupState,
}

/// Whether a technology works for a whole message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct CryptoAvailability {
    /// The sender has a usable own key / certificate (to sign, and to
    /// encrypt to self).
    pub own_key: bool,
    /// Every recipient has a usable key.
    pub all_recipients: bool,
}

/// Answer of [`crate::Request::CheckRecipientKeys`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct RecipientKeysReport {
    /// One entry per distinct recipient, in request order.
    pub recipients: Vec<RecipientKeyStatus>,
    /// OpenPGP for the whole message.
    pub openpgp: CryptoAvailability,
    /// S/MIME for the whole message.
    pub smime: CryptoAvailability,
    /// Autocrypt's recommendation.
    pub autocrypt: AutocryptRecommendation,
    /// What the core would do by default (identity defaults, Autocrypt
    /// `mutual`, replies to encrypted mail), for the composer's toggles.
    pub suggested: SendCrypto,
}

// ---------------------------------------------------------------------------
// Data protection
// ---------------------------------------------------------------------------

/// State of encryption at rest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum VaultState {
    /// Data on disk is not encrypted (the default until the maintainer decides;
    /// the UI recommends turning it on).
    #[default]
    Plain,
    /// Being encrypted (resumes after a crash or restart).
    Encrypting,
    /// Database, blobs and search index are encrypted.
    Encrypted,
    /// Being decrypted (resumes after a crash or restart).
    Decrypting,
}

impl VaultState {
    /// The wire / store name.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Plain => "plain",
            Self::Encrypting => "encrypting",
            Self::Encrypted => "encrypted",
            Self::Decrypting => "decrypting",
        }
    }
}

/// What a migration is working on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum VaultPhase {
    /// The metadata database.
    Database,
    /// Message and attachment blobs.
    Blobs,
    /// The search index.
    Index,
}

impl VaultPhase {
    /// The wire / store name.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Database => "database",
            Self::Blobs => "blobs",
            Self::Index => "index",
        }
    }
}

/// Encryption at rest: state and migration progress.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct VaultStatus {
    /// Current state.
    pub state: VaultState,
    /// While migrating: the current phase.
    #[serde(default)]
    pub phase: Option<VaultPhase>,
    /// While migrating: items done in this phase.
    #[serde(default)]
    pub done: u64,
    /// While migrating: items in this phase (0 while counting).
    #[serde(default)]
    pub total: u64,
    /// Whether the data key is in the OS keychain.
    #[serde(default)]
    pub key_in_keychain: bool,
    /// The data is encrypted but its key is not in the keychain (a new
    /// Windows profile, a reset Credential Manager): Nail cannot open the
    /// data until the user restores the key with the recovery key
    /// ([`crate::Request::RestoreVaultKey`]; M4c, ADR 0094).
    #[serde(default)]
    pub needs_recovery: bool,
    /// The recovery key (M4c).
    #[serde(default)]
    pub recovery: crate::recovery::RecoveryKeyStatus,
    /// The last migration error, if it stopped (it is retried on start).
    #[serde(default)]
    pub error: Option<ErrorCode>,
    /// Diagnostic detail (English), else `null`.
    #[serde(default)]
    pub message: Option<String>,
    /// The user-facing text (Fluent id + arguments) when the core has a more
    /// specific one than the error class; else `null` (ADR 0043).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub localized: Option<LocalizedMessage>,
}

impl VaultStatus {
    /// Unencrypted, nothing running.
    pub fn plain() -> Self {
        Self {
            state: VaultState::Plain,
            phase: None,
            done: 0,
            total: 0,
            key_in_keychain: false,
            needs_recovery: false,
            recovery: crate::recovery::RecoveryKeyStatus::default(),
            error: None,
            message: None,
            localized: None,
        }
    }
}

/// When the core looks up recipients' keys online.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum KeyLookupPolicy {
    /// Never (nothing leaves the machine).
    Off,
    /// When the user asks (key manager, the composer's "look up" button),
    /// the default.
    #[default]
    OnDemand,
    /// Also automatically while composing, for recipients without a key.
    Automatic,
}

/// Global security settings (stored by the core).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct SecuritySettings {
    /// Accounts whose decrypted content is added to the search index
    /// (opt-in; the index then counts as sensitive and is encrypted with
    /// the rest when encryption at rest is on). Empty by default.
    pub index_decrypted: Vec<AccountId>,
    /// Online key lookup.
    pub key_lookup: KeyLookupPolicy,
    /// Sources for online lookups, in order.
    pub lookup_sources: Vec<KeyLookupSource>,
}

impl Default for SecuritySettings {
    fn default() -> Self {
        Self {
            index_decrypted: Vec::new(),
            key_lookup: KeyLookupPolicy::OnDemand,
            lookup_sources: vec![KeyLookupSource::Wkd, KeyLookupSource::KeysOpenpgpOrg],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn enums_use_their_wire_names() {
        assert_eq!(
            serde_json::to_value(CryptoTechnology::OpenPgp).unwrap(),
            json!("openpgp")
        );
        assert_eq!(
            serde_json::to_value(CryptoPreference::Smime).unwrap(),
            json!("smime")
        );
        for tech in CryptoTechnology::ALL {
            assert_eq!(serde_json::to_value(tech).unwrap(), json!(tech.as_str()));
        }
        assert_eq!(
            serde_json::to_value(TrustLevel::AutocryptGossip).unwrap(),
            json!("autocrypt-gossip")
        );
        assert_eq!(
            serde_json::to_value(AutocryptPreference::NoPreference).unwrap(),
            json!("nopreference")
        );
        assert_eq!(
            serde_json::to_value(KeyLookupSource::KeysOpenpgpOrg).unwrap(),
            json!("keys-openpgp-org")
        );
        for source in [KeySource::WindowsStore, KeySource::SetupMessage] {
            assert_eq!(
                serde_json::to_value(source).unwrap(),
                json!(source.as_str())
            );
        }
        for phase in [VaultPhase::Database, VaultPhase::Blobs, VaultPhase::Index] {
            assert_eq!(serde_json::to_value(phase).unwrap(), json!(phase.as_str()));
        }
        assert_eq!(
            serde_json::to_value(VaultState::Encrypting).unwrap(),
            json!(VaultState::Encrypting.as_str())
        );
        assert_eq!(
            serde_json::to_value(TrustDecision::Verified).unwrap(),
            json!(TrustDecision::Verified.as_str())
        );
    }

    #[test]
    fn trust_levels_are_ordered_from_worst_to_best() {
        assert!(TrustLevel::Rejected < TrustLevel::Unknown);
        assert!(TrustLevel::Unknown < TrustLevel::AutocryptGossip);
        assert!(TrustLevel::Autocrypt < TrustLevel::Verified);
        assert!(TrustLevel::Verified < TrustLevel::Own);
    }

    #[test]
    fn security_status_helpers() {
        let signature = SignatureInfo {
            state: SignatureState::Valid,
            signer: Some("ada@example.org".into()),
            signer_matches_from: true,
            key_id: Some("key-1".into()),
            fingerprint: Some("ABCD".into()),
            trust: TrustLevel::Verified,
            signed_at: None,
            chain: None,
        };
        let mut status = SecurityStatus {
            technology: CryptoTechnology::OpenPgp,
            signatures: vec![signature.clone()],
            encryption: None,
            header_protection: HeaderProtection::None,
            partial: false,
            inline_pgp: false,
        };
        assert!(status.is_signed() && !status.is_encrypted());
        assert!(status.all_signatures_valid());
        status.signatures.push(SignatureInfo {
            state: SignatureState::Invalid,
            ..signature
        });
        assert!(!status.all_signatures_valid());
        status.signatures.clear();
        assert!(!status.all_signatures_valid());

        // Minimal JSON parses with defaults.
        let parsed: SecurityStatus = serde_json::from_value(json!({
            "technology": "smime",
            "encryption": {"state": "locked"}
        }))
        .unwrap();
        assert!(parsed.is_encrypted() && !parsed.is_signed());
        assert_eq!(parsed.header_protection, HeaderProtection::None);
        assert_eq!(parsed.encryption.unwrap().state, DecryptionState::Locked);
    }

    #[test]
    fn auth_results_summary_prefers_dmarc_then_dkim_then_spf() {
        let result = |verdict| AuthMethodResult {
            verdict,
            domain: Some("example.org".into()),
            selector: None,
            aligned: Some(true),
            reason: None,
        };
        let mut auth = AuthResults {
            authserv_id: "mx.example.net".into(),
            dkim: vec![],
            spf: None,
            dmarc: None,
            arc: None,
        };
        assert_eq!(auth.summary(), AuthVerdict::None);
        auth.spf = Some(result(AuthVerdict::Softfail));
        assert_eq!(auth.summary(), AuthVerdict::Softfail);
        auth.dkim = vec![result(AuthVerdict::Fail), result(AuthVerdict::Pass)];
        assert_eq!(auth.summary(), AuthVerdict::Pass);
        auth.dkim.pop();
        assert_eq!(auth.summary(), AuthVerdict::Fail);
        auth.dmarc = Some(result(AuthVerdict::Pass));
        assert_eq!(auth.summary(), AuthVerdict::Pass);
        assert_eq!(
            AuthVerdict::Softfail.as_rule_result(),
            Some(crate::rules::AuthResult::Softfail)
        );
        assert_eq!(AuthVerdict::Policy.as_rule_result(), None);
    }

    #[test]
    fn warnings_are_kind_tagged() {
        let warning = MessageWarning::LookalikeDomain {
            link: 2,
            href: "https://xn--pple-43d.com/login".into(),
            ascii_host: "xn--pple-43d.com".into(),
            unicode_host: "аpple.com".into(),
            resembles: Some("apple.com".into()),
        };
        let value = serde_json::to_value(&warning).unwrap();
        assert_eq!(value["kind"], json!("lookalike-domain"));
        assert_eq!(
            serde_json::from_value::<MessageWarning>(value).unwrap(),
            warning
        );
        let attachment = MessageWarning::DangerousAttachment {
            index: 0,
            filename: "invoice.pdf.exe".into(),
            risk: AttachmentRisk::DisguisedName,
        };
        assert_eq!(
            serde_json::to_value(&attachment).unwrap(),
            json!({"kind": "dangerous-attachment", "index": 0,
                   "filename": "invoice.pdf.exe", "risk": "disguised-name"})
        );
    }

    #[test]
    fn identity_crypto_defaults() {
        let parsed: IdentityCrypto = serde_json::from_value(json!({"sign": true})).unwrap();
        assert!(parsed.sign && !parsed.encrypt);
        assert!(parsed.autocrypt && parsed.protect_headers);
        assert!(!parsed.is_default());
        assert!(IdentityCrypto::default().is_default());
        assert_eq!(
            serde_json::to_value(IdentityCrypto::default()).unwrap()["technology"],
            json!("auto")
        );
    }

    #[test]
    fn send_crypto_and_protection() {
        assert!(!SendCrypto::none().is_active());
        let parsed: SendCrypto =
            serde_json::from_value(json!({"sign": true, "encrypt": false})).unwrap();
        assert!(parsed.is_active());
        assert_eq!(parsed.technology, None);
        assert_eq!(
            serde_json::to_value(SecretProtection::Passphrase {
                passphrase: "x".into()
            })
            .unwrap(),
            json!({"kind": "passphrase", "passphrase": "x"})
        );
        assert_eq!(
            serde_json::to_value(SecretProtection::default()).unwrap(),
            json!({"kind": "keychain"})
        );
    }

    #[test]
    fn settings_and_vault_defaults() {
        let settings: SecuritySettings = serde_json::from_value(json!({})).unwrap();
        assert_eq!(settings, SecuritySettings::default());
        assert_eq!(settings.key_lookup, KeyLookupPolicy::OnDemand);
        assert!(settings.index_decrypted.is_empty());
        let vault = VaultStatus::plain();
        assert_eq!(vault.state, VaultState::Plain);
        let parsed: VaultStatus = serde_json::from_value(json!({"state": "encrypted"})).unwrap();
        assert_eq!(parsed.state, VaultState::Encrypted);
        assert_eq!(parsed.done, 0);
    }
}
