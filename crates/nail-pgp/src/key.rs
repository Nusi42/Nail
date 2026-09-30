//! OpenPGP keys: parsing, validity analysis, merging, export.
//!
//! rPGP parses packets but leaves the "which self-signature counts"
//! policy to the application. [`KeyInfo::analyze`] applies it: only
//! self-signatures that verify count; the newest one of each component
//! decides its flags and expiry; revocations must verify too.
//! Third-party certifications are dropped on import ([`strip_foreign`]),
//! as keys.openpgp.org does, so flooded keys cannot bloat the keyring.

use std::io::BufReader;

use pgp::composed::{
    ArmorOptions, Deserializable, PublicOrSecret, SignedKeyDetails, SignedPublicKey,
    SignedPublicSubKey, SignedSecretKey,
};
use pgp::crypto::ecc_curve::ECCCurve;
use pgp::crypto::public_key::PublicKeyAlgorithm;
use pgp::packet::{PublicKey, Signature, SignatureType, SubpacketData};
use pgp::ser::Serialize;
use pgp::types::{KeyDetails, PublicParams, Tag};

use crate::error::{Error, Problem, Result};

/// Uppercase hex of bytes (fingerprints, key ids).
pub(crate) fn hex_upper(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes.iter().fold(String::new(), |mut s, b| {
        let _ = write!(s, "{b:02X}");
        s
    })
}

/// The address in a user id: `Name <a@b>` → `a@b`, or the whole id when
/// it is a bare address; lowercase.
pub fn email_of_user_id(user_id: &str) -> Option<String> {
    let candidate = match (user_id.rfind('<'), user_id.rfind('>')) {
        (Some(open), Some(close)) if open < close => &user_id[open + 1..close],
        _ => user_id,
    };
    let candidate = candidate.trim();
    let (local, domain) = candidate.split_once('@')?;
    if local.is_empty() || domain.is_empty() || candidate.contains(char::is_whitespace) {
        return None;
    }
    Some(candidate.to_lowercase())
}

/// Display name of a public-key algorithm, e.g. `Ed25519`, `RSA 3072`.
pub(crate) fn algorithm_name(alg: PublicKeyAlgorithm, params: &PublicParams) -> String {
    match params {
        PublicParams::RSA(_) => match params.to_bytes() {
            // The first MPI (n) starts with its bit length.
            Ok(bytes) if bytes.len() >= 2 => {
                format!("RSA {}", u16::from_be_bytes([bytes[0], bytes[1]]))
            }
            _ => "RSA".into(),
        },
        PublicParams::EdDSALegacy(_) | PublicParams::Ed25519(_) => "Ed25519".into(),
        PublicParams::Ed448(_) => "Ed448".into(),
        PublicParams::X25519(_) => "X25519".into(),
        PublicParams::X448(_) => "X448".into(),
        PublicParams::ECDH(p) => match p.curve() {
            ECCCurve::Curve25519Legacy => "X25519".into(),
            curve => format!("ECDH {}", curve.name()),
        },
        PublicParams::ECDSA(p) => format!("ECDSA {}", p.curve().name()),
        PublicParams::DSA(_) => "DSA".into(),
        PublicParams::Elgamal(_) => "ElGamal".into(),
        _ => format!("{alg:?}"),
    }
}

fn can_sign_by_algorithm(alg: PublicKeyAlgorithm) -> bool {
    matches!(
        alg,
        PublicKeyAlgorithm::RSA
            | PublicKeyAlgorithm::RSASign
            | PublicKeyAlgorithm::DSA
            | PublicKeyAlgorithm::ECDSA
            | PublicKeyAlgorithm::EdDSALegacy
            | PublicKeyAlgorithm::Ed25519
            | PublicKeyAlgorithm::Ed448
    )
}

fn can_encrypt_by_algorithm(alg: PublicKeyAlgorithm) -> bool {
    matches!(
        alg,
        PublicKeyAlgorithm::RSA
            | PublicKeyAlgorithm::RSAEncrypt
            | PublicKeyAlgorithm::ECDH
            | PublicKeyAlgorithm::Elgamal
            | PublicKeyAlgorithm::X25519
            | PublicKeyAlgorithm::X448
    )
}

fn created(sig: &Signature) -> i64 {
    sig.created().map_or(0, |t| i64::from(t.as_secs()))
}

fn has_key_flags(sig: &Signature) -> bool {
    sig.config().is_some_and(|c| {
        c.hashed_subpackets()
            .any(|p| matches!(p.data, SubpacketData::KeyFlags(_)))
    })
}

/// (sign, encrypt) capability a binding signature grants.
fn capabilities(sig: &Signature, alg: PublicKeyAlgorithm) -> (bool, bool) {
    if has_key_flags(sig) {
        let flags = sig.key_flags();
        (
            flags.sign(),
            flags.encrypt_comms() || flags.encrypt_storage(),
        )
    } else {
        (can_sign_by_algorithm(alg), can_encrypt_by_algorithm(alg))
    }
}

fn expiry(created_at: i64, sig: &Signature) -> Option<i64> {
    sig.key_expiration_time()
        .map(|d| i64::from(d.as_secs()))
        .filter(|secs| *secs > 0)
        .map(|secs| created_at + secs)
}

fn newest<'a>(sigs: impl Iterator<Item = &'a Signature>) -> Option<&'a Signature> {
    sigs.max_by_key(|s| created(s))
}

/// Whether a signature names `key` as its issuer (by fingerprint or key
/// id); signatures without an issuer are tried as well.
fn issued_by(sig: &Signature, key: &impl KeyDetails) -> bool {
    let fingerprints = sig.issuer_fingerprint();
    let ids = sig.issuer_key_id();
    if fingerprints.is_empty() && ids.is_empty() {
        return true;
    }
    let fp = key.fingerprint();
    let id = key.legacy_key_id();
    fingerprints.iter().any(|f| **f == fp) || ids.iter().any(|i| **i == id)
}

/// Validity of one subkey.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubkeyInfo {
    /// Index into `public_subkeys`.
    pub index: usize,
    /// Fingerprint (uppercase hex).
    pub fingerprint: String,
    /// Key id (uppercase hex, 16 digits).
    pub key_id: String,
    /// Algorithm name.
    pub algorithm: String,
    /// Creation time.
    pub created_at: i64,
    /// Expiry, if any.
    pub expires_at: Option<i64>,
    /// Revoked by a verified subkey revocation.
    pub revoked: bool,
    /// May sign (flags and a valid back-signature).
    pub sign: bool,
    /// May encrypt.
    pub encrypt: bool,
}

impl SubkeyInfo {
    /// Usable at `now` (not revoked or expired).
    pub fn usable_at(&self, now: i64) -> bool {
        !self.revoked && self.expires_at.is_none_or(|e| e > now) && self.created_at <= now + 86_400
    }
}

/// What a key is good for, as far as its verified self-signatures say.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyInfo {
    /// Primary key fingerprint (uppercase hex).
    pub fingerprint: String,
    /// Primary key id (uppercase hex, 16 digits).
    pub key_id: String,
    /// Valid user ids, primary first.
    pub user_ids: Vec<String>,
    /// Addresses from the user ids (lowercase, deduplicated).
    pub emails: Vec<String>,
    /// Primary key algorithm name.
    pub algorithm: String,
    /// Primary key creation (Unix seconds).
    pub created_at: i64,
    /// Primary key expiry.
    pub expires_at: Option<i64>,
    /// Revoked by a verified key revocation.
    pub revoked: bool,
    /// The primary key may sign.
    pub primary_sign: bool,
    /// The primary key may encrypt.
    pub primary_encrypt: bool,
    /// Subkeys with a valid binding.
    pub subkeys: Vec<SubkeyInfo>,
}

impl KeyInfo {
    /// Analyzes a transferable public key. Fails when no user id or
    /// direct-key self-signature verifies (then nothing binds the key to
    /// anyone).
    pub fn analyze(key: &SignedPublicKey) -> Result<Self> {
        let primary = &key.primary_key;
        let created_at = i64::from(primary.created_at().as_secs());
        let details = &key.details;

        // User ids with their newest valid certification.
        let mut users: Vec<(String, &Signature)> = Vec::new();
        for user in &details.users {
            let valid: Vec<&Signature> = user
                .signatures
                .iter()
                .filter(|s| issued_by(s, primary))
                .filter(|s| {
                    s.verify_certification(primary, Tag::UserId, &user.id)
                        .is_ok()
                })
                .collect();
            let cert = newest(
                valid
                    .iter()
                    .copied()
                    .filter(|s| s.typ() != Some(SignatureType::CertRevocation)),
            );
            let revocation = newest(
                valid
                    .iter()
                    .copied()
                    .filter(|s| s.typ() == Some(SignatureType::CertRevocation)),
            );
            let Some(cert) = cert else { continue };
            if revocation.is_some_and(|r| created(r) >= created(cert)) {
                continue;
            }
            let Some(text) = user.id.as_str() else {
                continue;
            };
            users.push((text.to_owned(), cert));
        }
        let direct = newest(
            details
                .direct_signatures
                .iter()
                .filter(|s| s.typ() == Some(SignatureType::Key))
                .filter(|s| s.verify_key(primary).is_ok()),
        );
        if users.is_empty() && direct.is_none() {
            return Err(Problem::NoSelfSignature.into());
        }
        // Primary user id first (the newest one flagged primary).
        let primary_index = users
            .iter()
            .enumerate()
            .filter(|(_, (_, s))| s.is_primary())
            .max_by_key(|(_, (_, s))| created(s))
            .map_or(0, |(i, _)| i);
        if primary_index > 0 {
            let user = users.remove(primary_index);
            users.insert(0, user);
        }
        let binding = users.first().map(|(_, s)| *s).or(direct);
        let alg = primary.algorithm();
        let (primary_sign, primary_encrypt) = binding
            .map(|s| capabilities(s, alg))
            .unwrap_or((false, false));
        let expires_at = binding
            .and_then(|s| expiry(created_at, s))
            .or_else(|| direct.and_then(|s| expiry(created_at, s)));
        let revoked = details.revocation_signatures.iter().any(|s| {
            s.typ() == Some(SignatureType::KeyRevocation) && s.verify_key(primary).is_ok()
        });

        let subkeys = key
            .public_subkeys
            .iter()
            .enumerate()
            .filter_map(|(index, sub)| analyze_subkey(index, sub, primary))
            .collect();

        let mut emails: Vec<String> = Vec::new();
        for (uid, _) in &users {
            if let Some(email) = email_of_user_id(uid)
                && !emails.contains(&email)
            {
                emails.push(email);
            }
        }
        Ok(Self {
            fingerprint: hex_upper(primary.fingerprint().as_bytes()),
            key_id: hex_upper(primary.legacy_key_id().as_ref()),
            user_ids: users.into_iter().map(|(u, _)| u).collect(),
            emails,
            algorithm: algorithm_name(alg, primary.public_params()),
            created_at,
            expires_at,
            revoked,
            primary_sign,
            primary_encrypt,
            subkeys,
        })
    }

    /// Expired at `now`.
    pub fn expired_at(&self, now: i64) -> bool {
        self.expires_at.is_some_and(|e| e <= now)
    }

    /// Usable at `now`: not revoked, not expired.
    pub fn valid_at(&self, now: i64) -> bool {
        !self.revoked && !self.expired_at(now)
    }

    /// Some component can sign (ignoring the primary key's expiry, which
    /// [`CryptoKey::expired`](nail_api::security::CryptoKey) reports).
    pub fn can_sign(&self, now: i64) -> bool {
        self.primary_sign || self.subkeys.iter().any(|s| s.sign && s.usable_at(now))
    }

    /// Some component can encrypt.
    pub fn can_encrypt(&self, now: i64) -> bool {
        self.primary_encrypt || self.subkeys.iter().any(|s| s.encrypt && s.usable_at(now))
    }

    /// The component to sign with at `now`: a usable signing subkey
    /// (newest) or the primary key. `None` = the primary key; `Some(i)` =
    /// subkey `i`. Errors when the key cannot sign now.
    pub fn signing_component(&self, now: i64) -> Result<Option<usize>> {
        if !self.valid_at(now) {
            return Err(Error::NoKey(format!(
                "key {} is expired or revoked",
                self.fingerprint
            )));
        }
        if let Some(sub) = self
            .subkeys
            .iter()
            .filter(|s| s.sign && s.usable_at(now))
            .max_by_key(|s| s.created_at)
        {
            return Ok(Some(sub.index));
        }
        if self.primary_sign {
            return Ok(None);
        }
        Err(Error::NoKey(format!(
            "key {} cannot sign",
            self.fingerprint
        )))
    }

    /// The component to encrypt to at `now` (the newest usable encryption
    /// subkey, else the primary key when it may encrypt).
    pub fn encryption_component(&self, now: i64) -> Result<Option<usize>> {
        if !self.valid_at(now) {
            return Err(Error::NoKey(format!(
                "key {} is expired or revoked",
                self.fingerprint
            )));
        }
        if let Some(sub) = self
            .subkeys
            .iter()
            .filter(|s| s.encrypt && s.usable_at(now))
            .max_by_key(|s| s.created_at)
        {
            return Ok(Some(sub.index));
        }
        if self.primary_encrypt {
            return Ok(None);
        }
        Err(Error::NoKey(format!(
            "key {} cannot encrypt",
            self.fingerprint
        )))
    }
}

fn analyze_subkey(
    index: usize,
    sub: &SignedPublicSubKey,
    primary: &PublicKey,
) -> Option<SubkeyInfo> {
    let key = &sub.key;
    let binding = newest(sub.signatures.iter().filter(|s| {
        s.typ() == Some(SignatureType::SubkeyBinding)
            && s.verify_subkey_binding(primary, key).is_ok()
    }))?;
    let created_at = i64::from(key.created_at().as_secs());
    let (mut sign, encrypt) = capabilities(binding, key.algorithm());
    if sign {
        // A signing subkey must cross-certify its primary key.
        sign = binding
            .embedded_signature()
            .is_some_and(|back| back.verify_primary_key_binding(key, primary).is_ok());
    }
    let revoked = sub.signatures.iter().any(|s| {
        s.typ() == Some(SignatureType::SubkeyRevocation)
            && s.verify_subkey_binding(primary, key).is_ok()
    });
    Some(SubkeyInfo {
        index,
        fingerprint: hex_upper(key.fingerprint().as_bytes()),
        key_id: hex_upper(key.legacy_key_id().as_ref()),
        algorithm: algorithm_name(key.algorithm(), key.public_params()),
        created_at,
        expires_at: expiry(created_at, binding),
        revoked,
        sign,
        encrypt,
    })
}

/// A key found in import data.
#[derive(Debug, Clone)]
pub enum ParsedKey {
    /// A transferable public key.
    Public(Box<SignedPublicKey>),
    /// A transferable secret key.
    Secret(Box<SignedSecretKey>),
}

impl ParsedKey {
    /// The public part.
    pub fn public(&self) -> SignedPublicKey {
        match self {
            Self::Public(k) => (**k).clone(),
            Self::Secret(k) => k.to_public_key(),
        }
    }
}

/// Parses armored (one or more blocks) or binary key data. Items that
/// fail to parse are reported as issues (position, message).
pub fn parse_keys(data: &[u8]) -> (Vec<ParsedKey>, Vec<(String, String)>) {
    let mut keys = Vec::new();
    let mut issues = Vec::new();
    let text = std::str::from_utf8(data).ok();
    let blocks: Vec<&str> = text.map(armor_blocks).unwrap_or_default();
    let mut push = |result: pgp::errors::Result<PublicOrSecret>, position: String| match result {
        Ok(PublicOrSecret::Public(k)) => keys.push(ParsedKey::Public(Box::new(k))),
        Ok(PublicOrSecret::Secret(k)) => keys.push(ParsedKey::Secret(Box::new(k))),
        Err(e) => issues.push((position, e.to_string())),
    };
    if blocks.is_empty() {
        match PublicOrSecret::from_bytes_many(BufReader::new(data)) {
            Ok(iter) => {
                for (i, item) in iter.enumerate() {
                    push(item, format!("#{}", i + 1));
                }
            }
            Err(e) => push(Err(e), "#1".into()),
        }
    } else {
        for (b, block) in blocks.iter().enumerate() {
            match PublicOrSecret::from_armor_many(block.as_bytes()) {
                Ok((iter, _)) => {
                    for (i, item) in iter.enumerate() {
                        push(item, format!("#{}.{}", b + 1, i + 1));
                    }
                }
                Err(e) => push(Err(e), format!("#{}", b + 1)),
            }
        }
    }
    (keys, issues)
}

/// The ASCII-armored key blocks in a text (anything around them, like
/// mail text, is ignored).
fn armor_blocks(text: &str) -> Vec<&str> {
    let mut blocks = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("-----BEGIN PGP ") {
        let block = &rest[start..];
        let Some(end_marker) = block.find("-----END PGP ") else {
            break;
        };
        let end = block[end_marker..]
            .find('\n')
            .map_or(block.len(), |p| end_marker + p + 1);
        let candidate = &block[..end];
        if candidate.starts_with("-----BEGIN PGP PUBLIC KEY BLOCK-----")
            || candidate.starts_with("-----BEGIN PGP PRIVATE KEY BLOCK-----")
        {
            blocks.push(candidate);
        }
        rest = &block[end..];
    }
    blocks
}

/// Drops third-party certifications and signatures that do not name the
/// primary key as issuer.
pub fn strip_foreign(key: &mut SignedPublicKey) {
    let primary = key.primary_key.clone();
    strip_details(&mut key.details, &primary);
}

fn strip_details(details: &mut SignedKeyDetails, primary: &PublicKey) {
    for user in &mut details.users {
        user.signatures.retain(|s| issued_by(s, primary));
    }
    details.users.retain(|u| !u.signatures.is_empty());
    for attr in &mut details.user_attributes {
        attr.signatures.retain(|s| issued_by(s, primary));
    }
    details.user_attributes.retain(|a| !a.signatures.is_empty());
}

/// Same for a secret key.
pub fn strip_foreign_secret(key: &mut SignedSecretKey) {
    let primary = key.primary_key.public_key().clone();
    strip_details(&mut key.details, &primary);
}

fn push_unique(into: &mut Vec<Signature>, from: &[Signature]) {
    for sig in from {
        if !into.contains(sig) {
            into.push(sig.clone());
        }
    }
}

/// Merges `new` into `old` (same primary key): new user ids, subkeys and
/// signatures (revocations, renewed expiry) are added; nothing is
/// removed.
pub fn merge(old: &SignedPublicKey, new: &SignedPublicKey) -> SignedPublicKey {
    let mut merged = old.clone();
    push_unique(
        &mut merged.details.revocation_signatures,
        &new.details.revocation_signatures,
    );
    push_unique(
        &mut merged.details.direct_signatures,
        &new.details.direct_signatures,
    );
    for user in &new.details.users {
        match merged.details.users.iter_mut().find(|u| u.id == user.id) {
            Some(existing) => push_unique(&mut existing.signatures, &user.signatures),
            None => merged.details.users.push(user.clone()),
        }
    }
    for attr in &new.details.user_attributes {
        match merged
            .details
            .user_attributes
            .iter_mut()
            .find(|a| a.attr == attr.attr)
        {
            Some(existing) => push_unique(&mut existing.signatures, &attr.signatures),
            None => merged.details.user_attributes.push(attr.clone()),
        }
    }
    for sub in &new.public_subkeys {
        match merged
            .public_subkeys
            .iter_mut()
            .find(|s| s.key.fingerprint() == sub.key.fingerprint())
        {
            Some(existing) => push_unique(&mut existing.signatures, &sub.signatures),
            None => merged.public_subkeys.push(sub.clone()),
        }
    }
    merged
}

/// Binary transferable public key.
pub fn to_binary(key: &SignedPublicKey) -> Result<Vec<u8>> {
    Ok(key.to_bytes()?)
}

/// ASCII-armored public key.
pub fn to_armored(key: &SignedPublicKey) -> Result<String> {
    Ok(key.to_armored_string(ArmorOptions::default())?)
}

/// Parses one binary transferable public key (as stored).
pub fn public_from_binary(data: &[u8]) -> Result<SignedPublicKey> {
    SignedPublicKey::from_bytes(BufReader::new(data)).map_err(Error::crypto)
}

/// Parses one binary transferable secret key (as stored).
pub fn secret_from_binary(data: &[u8]) -> Result<SignedSecretKey> {
    SignedSecretKey::from_bytes(BufReader::new(data)).map_err(Error::crypto)
}

/// The minimal key for an `Autocrypt` header (Autocrypt Level 1 §2.1.1):
/// the primary key, the user id for `addr` with its newest valid
/// self-signature, and the encryption subkey with its binding. Falls back
/// to the primary user id when no user id matches `addr`.
pub fn minimal_for(key: &SignedPublicKey, addr: &str, now: i64) -> Result<SignedPublicKey> {
    let info = KeyInfo::analyze(key)?;
    let primary = &key.primary_key;
    let addr = addr.trim().to_lowercase();
    let user = key
        .details
        .users
        .iter()
        .filter(|u| {
            u.id.as_str()
                .and_then(email_of_user_id)
                .is_some_and(|e| e == addr)
        })
        .find_map(|u| {
            let sig = newest(u.signatures.iter().filter(|s| {
                s.typ() != Some(SignatureType::CertRevocation)
                    && s.verify_certification(primary, Tag::UserId, &u.id).is_ok()
            }))?;
            Some(pgp::types::SignedUser::new(u.id.clone(), vec![sig.clone()]))
        });
    let user = match user {
        Some(u) => u,
        None => {
            let first = info
                .user_ids
                .first()
                .ok_or_else(|| Error::NoKey("the key has no user id".into()))?;
            let u = key
                .details
                .users
                .iter()
                .find(|u| u.id.as_str() == Some(first.as_str()))
                .ok_or_else(|| Error::NoKey("the key has no user id".into()))?;
            let sig = newest(
                u.signatures
                    .iter()
                    .filter(|s| s.verify_certification(primary, Tag::UserId, &u.id).is_ok()),
            )
            .ok_or_else(|| Error::NoKey("the key has no user id".into()))?;
            pgp::types::SignedUser::new(u.id.clone(), vec![sig.clone()])
        }
    };
    let subkeys = match info.encryption_component(now)? {
        Some(index) => {
            let sub = &key.public_subkeys[index];
            let binding = newest(sub.signatures.iter().filter(|s| {
                s.typ() == Some(SignatureType::SubkeyBinding)
                    && s.verify_subkey_binding(primary, &sub.key).is_ok()
            }))
            .cloned();
            vec![SignedPublicSubKey::new(
                sub.key.clone(),
                binding.into_iter().collect(),
            )]
        }
        None => Vec::new(),
    };
    Ok(SignedPublicKey::new(
        primary.clone(),
        SignedKeyDetails::new(Vec::new(), Vec::new(), vec![user], Vec::new()),
        subkeys,
    ))
}

/// The key restricted to `addr`, for a key learned from an `Autocrypt` or
/// `Autocrypt-Gossip` header: only the user ids for `addr` that carry a
/// valid self-certification are kept (with all their self-signatures, so
/// revocations stay); every other user id and user attribute is dropped.
/// The header only vouches for the binding of the key to `addr`, so no
/// other address may be learned from it. [`Error::Invalid`] when the key
/// has no valid user id for `addr`.
pub fn bind_to_address(key: &SignedPublicKey, addr: &str) -> Result<SignedPublicKey> {
    let addr = addr.trim().to_lowercase();
    let primary = &key.primary_key;
    let users: Vec<_> = key
        .details
        .users
        .iter()
        .filter(|u| {
            u.id.as_str()
                .and_then(email_of_user_id)
                .is_some_and(|e| e == addr)
        })
        .filter(|u| {
            u.signatures.iter().any(|s| {
                s.typ() != Some(SignatureType::CertRevocation)
                    && s.verify_certification(primary, Tag::UserId, &u.id).is_ok()
            })
        })
        .map(|u| {
            let signatures = u
                .signatures
                .iter()
                .filter(|s| s.verify_certification(primary, Tag::UserId, &u.id).is_ok())
                .cloned()
                .collect();
            pgp::types::SignedUser::new(u.id.clone(), signatures)
        })
        .collect();
    if users.is_empty() {
        return Err(Problem::NoUserIdFor {
            address: addr.to_string(),
        }
        .into());
    }
    Ok(SignedPublicKey::new(
        primary.clone(),
        SignedKeyDetails::new(
            key.details.revocation_signatures.clone(),
            key.details.direct_signatures.clone(),
            users,
            Vec::new(),
        ),
        key.public_subkeys.clone(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{fixture, fixture_text};

    const DAY: i64 = 86_400;
    // 2026-09-29, after every fixture key's creation.
    const NOW: i64 = 1_790_000_000;

    fn public(name: &str) -> SignedPublicKey {
        let (keys, issues) = parse_keys(&fixture(name));
        assert!(issues.is_empty(), "{issues:?}");
        keys[0].public()
    }

    #[test]
    fn user_id_addresses() {
        assert_eq!(
            email_of_user_id("Ada Lovelace <Ada@Example.org>").as_deref(),
            Some("ada@example.org")
        );
        assert_eq!(
            email_of_user_id("bob@example.net").as_deref(),
            Some("bob@example.net")
        );
        assert_eq!(email_of_user_id("No Address"), None);
        assert_eq!(email_of_user_id("a b@c"), None);
        assert_eq!(email_of_user_id("<@x>"), None);
    }

    #[test]
    fn gnupg_curve25519_key_is_analyzed() {
        let info = KeyInfo::analyze(&public("alice.pub.asc")).unwrap();
        assert_eq!(info.fingerprint.len(), 40);
        assert_eq!(info.user_ids, vec!["Alice Example <alice@example.org>"]);
        assert_eq!(info.emails, vec!["alice@example.org"]);
        assert_eq!(info.algorithm, "Ed25519");
        assert_eq!(info.created_at, 1_767_268_800);
        assert_eq!(info.expires_at, None);
        assert!(!info.revoked);
        assert!(info.primary_sign && !info.primary_encrypt);
        assert_eq!(info.subkeys.len(), 1);
        assert_eq!(info.subkeys[0].algorithm, "X25519");
        assert!(info.subkeys[0].encrypt && !info.subkeys[0].sign);
        assert!(info.can_sign(NOW) && info.can_encrypt(NOW));
        assert_eq!(info.signing_component(NOW).unwrap(), None);
        assert_eq!(info.encryption_component(NOW).unwrap(), Some(0));
        assert_eq!(info.key_id, info.fingerprint[24..]);
    }

    #[test]
    fn gnupg_rsa_key_is_analyzed() {
        let info = KeyInfo::analyze(&public("carol.pub.asc")).unwrap();
        assert_eq!(info.algorithm, "RSA 3072");
        assert!(info.primary_sign);
        assert_eq!(info.subkeys[0].algorithm, "RSA 3072");
        assert_eq!(info.encryption_component(NOW).unwrap(), Some(0));
    }

    #[test]
    fn expired_and_revoked_keys() {
        let dave = KeyInfo::analyze(&public("dave.pub.asc")).unwrap();
        // Created 2020-01-01, one year of validity.
        assert_eq!(dave.created_at, 1_577_880_000);
        let expires = dave.expires_at.unwrap();
        assert!((expires - dave.created_at - 366 * DAY).abs() <= DAY);
        assert!(dave.expired_at(NOW) && !dave.valid_at(NOW));
        assert!(!dave.expired_at(dave.created_at + 10));
        assert!(matches!(dave.signing_component(NOW), Err(Error::NoKey(_))));
        assert!(matches!(
            dave.encryption_component(NOW),
            Err(Error::NoKey(_))
        ));

        let eve = KeyInfo::analyze(&public("eve.pub.asc")).unwrap();
        assert!(eve.revoked && !eve.valid_at(NOW));
        assert!(eve.encryption_component(NOW).is_err());
    }

    #[test]
    fn parse_reports_issues_and_reads_binary_and_several_blocks() {
        let mut both = fixture_text("alice.pub.asc");
        both.push_str("some text in between\n");
        both.push_str(&fixture_text("bob.pub.asc"));
        let (keys, issues) = parse_keys(both.as_bytes());
        assert!(issues.is_empty(), "{issues:?}");
        assert_eq!(keys.len(), 2);

        let binary = to_binary(&keys[1].public()).unwrap();
        let (keys, _) = parse_keys(&binary);
        assert_eq!(keys.len(), 1);

        let (keys, issues) = parse_keys(
            b"-----BEGIN PGP PUBLIC KEY BLOCK-----\n\nAAAA\n-----END PGP PUBLIC KEY BLOCK-----\n",
        );
        assert!(keys.is_empty());
        assert_eq!(issues.len(), 1);
        let (keys, _) = parse_keys(b"\x99garbage");
        assert!(keys.is_empty());

        let (keys, issues) = parse_keys(&fixture("bob.sec.asc"));
        assert!(issues.is_empty());
        assert!(matches!(keys[0], ParsedKey::Secret(_)));
    }

    #[test]
    fn merge_adds_revocations_and_armored_roundtrip() {
        let eve = public("eve.pub.asc");
        let mut unrevoked = eve.clone();
        unrevoked.details.revocation_signatures.clear();
        assert!(!KeyInfo::analyze(&unrevoked).unwrap().revoked);
        let merged = merge(&unrevoked, &eve);
        assert!(KeyInfo::analyze(&merged).unwrap().revoked);
        // Merging twice adds nothing.
        assert_eq!(merge(&merged, &eve), merged);

        let armored = to_armored(&merged).unwrap();
        let (keys, _) = parse_keys(armored.as_bytes());
        assert_eq!(keys[0].public(), merged);
    }

    #[test]
    fn minimal_key_for_autocrypt() {
        let alice = public("alice.pub.asc");
        let minimal = minimal_for(&alice, "ALICE@example.org", NOW).unwrap();
        assert_eq!(minimal.details.users.len(), 1);
        assert_eq!(minimal.public_subkeys.len(), 1);
        assert_eq!(minimal.details.users[0].signatures.len(), 1);
        let info = KeyInfo::analyze(&minimal).unwrap();
        assert_eq!(
            info.fingerprint,
            KeyInfo::analyze(&alice).unwrap().fingerprint
        );
        assert!(info.can_encrypt(NOW));
        // Unknown address: falls back to the primary user id.
        assert!(minimal_for(&alice, "other@example.org", NOW).is_ok());
    }

    /// A fresh Ed25519 key with several user ids (the first is primary).
    fn multi_uid_key(user_ids: &[&str]) -> SignedPublicKey {
        use pgp::composed::{KeyType, SecretKeyParamsBuilder};
        let mut builder = SecretKeyParamsBuilder::default();
        builder
            .key_type(KeyType::Ed25519Legacy)
            .can_certify(true)
            .can_sign(true)
            .primary_user_id(user_ids[0].to_owned());
        for extra in &user_ids[1..] {
            builder.user_id(*extra);
        }
        builder
            .build()
            .unwrap()
            .generate(rand::thread_rng())
            .unwrap()
            .to_public_key()
    }

    #[test]
    fn autocrypt_keys_are_bound_to_their_address() {
        let key = multi_uid_key(&["CEO <ceo@bank.example>", "Mallory <Mallory@Evil.example>"]);
        assert_eq!(
            KeyInfo::analyze(&key).unwrap().emails,
            vec!["ceo@bank.example", "mallory@evil.example"]
        );
        // Only the header's address survives.
        let bound = bind_to_address(&key, "mallory@evil.example").unwrap();
        let info = KeyInfo::analyze(&bound).unwrap();
        assert_eq!(info.emails, vec!["mallory@evil.example"]);
        assert_eq!(
            info.fingerprint,
            KeyInfo::analyze(&key).unwrap().fingerprint
        );
        // A key without a user id for the address is refused.
        assert!(matches!(
            bind_to_address(&key, "other@evil.example"),
            Err(Error::Problem(Problem::NoUserIdFor { .. }))
        ));
        // A user id whose self-signature does not verify does not count.
        let mut forged = key.clone();
        let foreign = multi_uid_key(&["x@evil.example"]);
        forged.details.users[1].signatures = foreign.details.users[0].signatures.clone();
        assert!(bind_to_address(&forged, "mallory@evil.example").is_err());
        // Revocations of the key are kept.
        let eve = public("eve.pub.asc");
        let eve_addr = KeyInfo::analyze(&eve).unwrap().emails[0].clone();
        assert!(
            KeyInfo::analyze(&bind_to_address(&eve, &eve_addr).unwrap())
                .unwrap()
                .revoked
        );
    }

    #[test]
    fn third_party_certifications_are_stripped() {
        // Certify bob's user id with alice's key (a third-party signature).
        let mut bob = public("bob.pub.asc");
        let (keys, _) = parse_keys(&fixture("carol.sec.asc"));
        let ParsedKey::Secret(carol) = &keys[0] else {
            panic!("secret key expected");
        };
        let user = bob.details.users[0].clone();
        let config = pgp::packet::SignatureConfig::from_key(
            rand::thread_rng(),
            &carol.primary_key,
            SignatureType::CertGeneric,
        )
        .unwrap();
        let mut config = config;
        config.hashed_subpackets = vec![
            pgp::packet::Subpacket::regular(SubpacketData::IssuerFingerprint(
                carol.primary_key.fingerprint(),
            ))
            .unwrap(),
        ];
        let sig = config
            .sign_certification_third_party(
                &carol.primary_key,
                &pgp::types::Password::empty(),
                &bob.primary_key,
                Tag::UserId,
                &user.id,
            )
            .unwrap();
        bob.details.users[0].signatures.push(sig);
        assert_eq!(bob.details.users[0].signatures.len(), 2);
        // Still analyzable (the foreign signature is ignored) ...
        assert!(KeyInfo::analyze(&bob).is_ok());
        // ... and removed by strip_foreign.
        strip_foreign(&mut bob);
        assert_eq!(bob.details.users[0].signatures.len(), 1);
    }
}
