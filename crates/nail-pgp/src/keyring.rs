//! The OpenPGP side of Nail's keyring (store schema v5, ADR 0029).
//!
//! Keys live in `nail_store`'s `crypto_keys` table: the binary
//! transferable public key, metadata for listing and lookups, and for own
//! keys the **OpenPGP-protected** transferable secret key:
//!
//! * [`SecretStorage::Passphrase`]: S2K-protected with the user's
//!   passphrase (iterated + salted SHA-256, AES-256). Signing and
//!   decrypting need [`Keyring::unlock`] first; the passphrase is then
//!   kept in memory ([`UnlockCache`]) for the requested time.
//! * [`SecretStorage::Keychain`]: S2K-protected with a random 256-bit
//!   secret that lives in the OS keychain ([`SecretStore`], reference
//!   `keychain:nail/openpgp/<FINGERPRINT>`); no prompt. The store row
//!   alone is useless without the keychain entry.
//!
//! The store never sees a plain secret key.

use std::collections::HashMap;
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use base64::Engine as _;
use nail_api::accounts::SecretRef;
use nail_api::engine::SecretStore;
use nail_api::security::{
    CryptoKey, CryptoTechnology, KeyAlgorithm, KeyEncoding, KeyFilter, KeyImportIssue,
    KeyImportResult, KeySource, RecipientKey, RecipientKeyState, SecretProtection, SecretStorage,
    TrustDecision, TrustLevel,
};
use nail_api::smartcard::SmartcardKeyRef;
use nail_api::{ErrorCode, KeyId, LocalizedMessage};
use nail_store::{KeyData, SecretKeyData, Store, StoredKey};
use pgp::composed::{
    EncryptionCaps, KeyType, SecretKeyParamsBuilder, SignedPublicKey, SignedSecretKey,
    SubkeyParamsBuilder,
};
use pgp::crypto::ecc_curve::ECCCurve;
use pgp::crypto::hash::HashAlgorithm;
use pgp::crypto::sym::SymmetricKeyAlgorithm;
use pgp::packet::{Subpacket, SubpacketData};
use pgp::ser::Serialize as _;
use pgp::types::{KeyDetails, Password, S2kParams, SecretParams, SigningKey, StringToKey, Tag};
use rand::{Rng, thread_rng};
use zeroize::Zeroizing;

use crate::error::{Error, Problem, Result};
use crate::key::{self, KeyInfo, ParsedKey};

/// Unix seconds now.
pub(crate) fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

/// RFC 3339 (UTC, seconds) of Unix seconds.
pub(crate) fn rfc3339(secs: i64) -> String {
    chrono::DateTime::from_timestamp(secs, 0)
        .unwrap_or_default()
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

/// Passphrases of unlocked keys, in memory only (per process; the core
/// owns one). Entries expire after the time given to
/// [`Keyring::unlock`]; without a time they stay until [`Self::clear`] or
/// the process ends.
#[derive(Default)]
pub struct UnlockCache {
    entries: Mutex<HashMap<String, CachedPassphrase>>,
}

/// A passphrase and when it expires (`None` = at exit).
type CachedPassphrase = (Zeroizing<String>, Option<Instant>);

impl std::fmt::Debug for UnlockCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UnlockCache")
            .field("unlocked", &self.lock().len())
            .finish()
    }
}

impl UnlockCache {
    /// An empty cache.
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, CachedPassphrase>> {
        self.entries.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn get(&self, fingerprint: &str) -> Option<Zeroizing<String>> {
        let mut entries = self.lock();
        match entries.get(fingerprint) {
            Some((_, Some(until))) if *until <= Instant::now() => {
                entries.remove(fingerprint);
                None
            }
            Some((pass, _)) => Some(pass.clone()),
            None => None,
        }
    }

    fn put(&self, fingerprint: &str, passphrase: &str, remember_minutes: Option<u32>) {
        let until =
            remember_minutes.map(|m| Instant::now() + Duration::from_secs(u64::from(m) * 60));
        self.lock().insert(
            fingerprint.to_owned(),
            (Zeroizing::new(passphrase.to_owned()), until),
        );
    }

    /// Forgets the passphrase of one key.
    pub fn forget(&self, fingerprint: &str) {
        self.lock().remove(fingerprint);
    }

    /// Forgets every passphrase (e.g. when the user locks Nail).
    pub fn clear(&self) {
        self.lock().clear();
    }

    /// Whether the key's passphrase is cached.
    pub fn is_unlocked(&self, fingerprint: &str) -> bool {
        self.get(fingerprint).is_some()
    }
}

/// An exported public key ([`nail_api::Response::KeyExported`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportedKey {
    /// Suggested file name (`<address>.asc` / `.pgp`).
    pub filename: String,
    /// `application/pgp-keys`.
    pub mime: String,
    /// The encoding of `data`.
    pub encoding: KeyEncoding,
    /// Armored text or base64 of the binary key.
    pub data: String,
}

/// An own secret key ready to sign or decrypt.
pub struct UnlockedKey {
    /// The key as stored.
    pub stored: StoredKey,
    /// The (still S2K-protected) secret key; `None` for a key on a
    /// smartcard (M4c).
    pub secret: Option<SignedSecretKey>,
    /// Its analysis.
    pub info: KeyInfo,
    password: Zeroizing<String>,
    /// The signing key on a smartcard (M4c).
    card: Option<crate::smartcard::CardSigningKey>,
}

impl std::fmt::Debug for UnlockedKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UnlockedKey")
            .field("key", &self.stored.id)
            .field("fingerprint", &self.info.fingerprint)
            .finish_non_exhaustive()
    }
}

impl UnlockedKey {
    /// The password that unlocks the secret key material.
    pub(crate) fn password(&self) -> Password {
        Password::from(self.password.as_str())
    }

    /// Whether the secret stays on a smartcard.
    pub fn on_smartcard(&self) -> bool {
        self.card.is_some()
    }

    /// The secret key; [`Error::Unsupported`] for a key on a smartcard
    /// (it cannot be exported, e.g. into an Autocrypt setup message).
    pub fn secret_key(&self) -> Result<&SignedSecretKey> {
        self.secret.as_ref().ok_or_else(|| {
            Error::Unsupported("the secret key is on a smartcard and cannot be exported".into())
        })
    }

    /// The card error of the last failed signature (rPGP reports card
    /// failures as its own errors).
    pub(crate) fn card_error(&self) -> Option<nail_pkcs11::CardError> {
        self.card
            .as_ref()
            .and_then(super::smartcard::CardSigningKey::last_error)
    }

    /// The component that signs at `now`.
    pub(crate) fn signing_key(&self, now: i64) -> Result<&dyn SigningKey> {
        if let Some(card) = &self.card {
            return Ok(card);
        }
        let secret = self.secret_key()?;
        Ok(match self.info.signing_component(now)? {
            None => &secret.primary_key,
            Some(index) => {
                let fingerprint = secret.public_subkeys_fingerprint(index);
                secret
                    .secret_subkeys
                    .iter()
                    .find(|s| Some(s.key.fingerprint()) == fingerprint)
                    .map(|s| -> &dyn SigningKey { &s.key })
                    .ok_or_else(|| Error::NoKey("the signing subkey has no secret part".into()))?
            }
        })
    }
}

trait SubkeyIndex {
    fn public_subkeys_fingerprint(&self, index: usize) -> Option<pgp::types::Fingerprint>;
}

impl SubkeyIndex for SignedSecretKey {
    /// `KeyInfo` indexes the subkeys of the public key, which lists the
    /// secret subkeys (in order) after the public-only ones.
    fn public_subkeys_fingerprint(&self, index: usize) -> Option<pgp::types::Fingerprint> {
        self.to_public_key()
            .public_subkeys
            .get(index)
            .map(|s| s.key.fingerprint())
    }
}

/// The effective trust of a stored key (see [`TrustLevel`]): a user
/// decision wins; else own keys are `own`; else the source decides.
pub fn effective_trust(key: &StoredKey) -> TrustLevel {
    match key.decision {
        Some(TrustDecision::Verified) => TrustLevel::Verified,
        Some(TrustDecision::Rejected) => TrustLevel::Rejected,
        _ if key.is_own() => TrustLevel::Own,
        _ => match key.data.source {
            KeySource::Autocrypt => TrustLevel::Autocrypt,
            KeySource::AutocryptGossip => TrustLevel::AutocryptGossip,
            _ => TrustLevel::Unknown,
        },
    }
}

/// The protocol view of a stored key (both technologies; the S/MIME
/// chain is left to `nail-smime`).
pub fn to_crypto_key(key: &StoredKey, now: i64) -> CryptoKey {
    let data = &key.data;
    CryptoKey {
        id: key.id.clone(),
        technology: data.technology,
        fingerprint: data.fingerprint.clone(),
        user_ids: data.user_ids.clone(),
        emails: data.emails.clone(),
        algorithm: data.algorithm.clone(),
        created_at: rfc3339(data.created_at),
        expires_at: data.expires_at.map(rfc3339),
        expired: data.expires_at.is_some_and(|e| e <= now),
        revoked: data.revoked,
        can_sign: data.can_sign,
        can_encrypt: data.can_encrypt,
        has_secret: data.secret.is_some(),
        secret_storage: data.secret.as_ref().map(|s| s.storage),
        source: data.source,
        trust: effective_trust(key),
        issuer: data.issuer.clone(),
        serial: data.serial.clone(),
        chain: None,
        smartcard: data
            .secret
            .as_ref()
            .filter(|s| s.storage == SecretStorage::Smartcard)
            .and_then(|s| s.reference.as_deref())
            .and_then(|r| crate::smartcard::parse_references(r).ok())
            .and_then(|refs| refs.into_iter().next()),
    }
}

/// The outcome of decrypting a session key on a smartcard.
#[derive(Debug)]
pub(crate) enum CardDecryption {
    /// The session key, decrypted with this own key.
    Session(StoredKey, pgp::composed::PlainSessionKey),
    /// The card could not (removed, PIN not entered, …).
    Failed(StoredKey, nail_pkcs11::CardError),
}

/// Whether a key matches a [`KeyFilter`] (technology, own, text).
pub fn matches_filter(key: &StoredKey, filter: &KeyFilter) -> bool {
    if filter.technology.is_some_and(|t| t != key.data.technology) {
        return false;
    }
    if filter.own_only && !key.is_own() {
        return false;
    }
    let Some(text) = filter
        .text
        .as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty())
    else {
        return true;
    };
    let needle = text.to_lowercase();
    let compact: String = needle.chars().filter(|c| !c.is_whitespace()).collect();
    key.data
        .user_ids
        .iter()
        .any(|u| u.to_lowercase().contains(&needle))
        || key.data.emails.iter().any(|e| e.contains(&needle))
        || key.data.fingerprint.to_lowercase().contains(&compact)
}

fn source_rank(source: KeySource) -> u8 {
    match source {
        KeySource::Generated | KeySource::SetupMessage => 4,
        KeySource::Autocrypt => 3,
        KeySource::AutocryptGossip => 2,
        _ => 1,
    }
}

/// Keychain reference of the random key protecting an own key.
pub fn keychain_ref(fingerprint: &str) -> SecretRef {
    SecretRef(format!(
        "{}{}/openpgp/{}",
        SecretRef::SCHEME,
        SecretRef::SERVICE,
        fingerprint
    ))
}

fn components_protected(secret: &SignedSecretKey) -> bool {
    secret.primary_key.secret_params().is_encrypted()
        || secret
            .secret_subkeys
            .iter()
            .any(|s| s.key.secret_params().is_encrypted())
}

/// S2K parameters: a stretched passphrase (the default count), or a
/// single pass for the 256-bit random keychain secret.
fn s2k(stretch: bool) -> S2kParams {
    let mut rng = thread_rng();
    let sym_alg = SymmetricKeyAlgorithm::AES256;
    let mut iv = vec![0u8; sym_alg.block_size()];
    rng.fill(&mut iv[..]);
    let s2k = if stretch {
        StringToKey::new_default(&mut rng)
    } else {
        StringToKey::new_iterated(&mut rng, HashAlgorithm::Sha256, 0)
    };
    S2kParams::Cfb {
        sym_alg,
        s2k,
        iv: iv.into(),
    }
}

/// Protects every unprotected component with `password`.
fn protect(secret: &mut SignedSecretKey, password: &Password, stretch: bool) -> Result<()> {
    if !secret.primary_key.secret_params().is_encrypted() {
        secret
            .primary_key
            .set_password_with_s2k(password, s2k(stretch))?;
    }
    for sub in &mut secret.secret_subkeys {
        if !sub.key.secret_params().is_encrypted() {
            sub.key.set_password_with_s2k(password, s2k(stretch))?;
        }
    }
    Ok(())
}

/// Whether `password` unlocks the key (every protected component).
fn password_unlocks(secret: &SignedSecretKey, password: &Password) -> bool {
    let primary_ok = match secret.primary_key.secret_params() {
        SecretParams::Encrypted(_) => secret.primary_key.unlock(password, |_, _| Ok(())).is_ok(),
        SecretParams::Plain(_) => true,
    };
    primary_ok
        && secret
            .secret_subkeys
            .iter()
            .all(|s| match s.key.secret_params() {
                SecretParams::Encrypted(_) => s.key.unlock(password, |_, _| Ok(())).is_ok(),
                SecretParams::Plain(_) => true,
            })
}

/// Adds a key expiration to the self-signatures of a freshly generated
/// (unprotected) key.
fn set_expiry(secret: &mut SignedSecretKey, seconds: u32) -> Result<()> {
    let expiry = || {
        Subpacket::regular(SubpacketData::KeyExpirationTime(
            pgp::types::Duration::from_secs(seconds),
        ))
    };
    let primary = secret.primary_key.clone();
    let empty = Password::empty();
    for user in &mut secret.details.users {
        let mut signatures = Vec::new();
        for sig in &user.signatures {
            let Some(config) = sig.config() else { continue };
            let mut config = config.clone();
            config.hashed_subpackets.push(expiry()?);
            signatures.push(config.sign_certification(
                &primary,
                primary.public_key(),
                &empty,
                Tag::UserId,
                &user.id,
            )?);
        }
        user.signatures = signatures;
    }
    for sub in &mut secret.secret_subkeys {
        let mut signatures = Vec::new();
        for sig in &sub.signatures {
            let Some(config) = sig.config() else { continue };
            let mut config = config.clone();
            config.hashed_subpackets.push(expiry()?);
            signatures.push(config.sign_subkey_binding(
                &primary,
                primary.public_key(),
                &empty,
                sub.key.public_key(),
            )?);
        }
        sub.signatures = signatures;
    }
    Ok(())
}

/// Generates an unprotected key: Ed25519 (sign, certify) + X25519
/// (encrypt), or RSA with `rsa_bits` for both.
pub(crate) fn generate_secret(
    user_id: &str,
    algorithm: KeyAlgorithm,
    rsa_bits: u32,
    expires_days: Option<u32>,
) -> Result<SignedSecretKey> {
    let (primary_type, sub_type) = match algorithm {
        KeyAlgorithm::Curve25519 => (
            KeyType::Ed25519Legacy,
            KeyType::ECDH(ECCCurve::Curve25519Legacy),
        ),
        KeyAlgorithm::Rsa4096 => (KeyType::Rsa(rsa_bits), KeyType::Rsa(rsa_bits)),
    };
    let subkey = SubkeyParamsBuilder::default()
        .key_type(sub_type)
        .can_sign(false)
        .can_encrypt(EncryptionCaps::All)
        .can_authenticate(false)
        .build()
        .map_err(Error::crypto)?;
    let params = SecretKeyParamsBuilder::default()
        .key_type(primary_type)
        .can_certify(true)
        .can_sign(true)
        .can_encrypt(EncryptionCaps::None)
        .primary_user_id(user_id.to_owned())
        .preferred_symmetric_algorithms(
            vec![SymmetricKeyAlgorithm::AES256, SymmetricKeyAlgorithm::AES128].into(),
        )
        .preferred_hash_algorithms(vec![HashAlgorithm::Sha256, HashAlgorithm::Sha512].into())
        .preferred_compression_algorithms(Vec::new().into())
        .subkeys(vec![subkey])
        .build()
        .map_err(Error::crypto)?;
    let mut secret = params.generate(thread_rng())?;
    if let Some(days) = expires_days.filter(|d| *d > 0) {
        let seconds =
            u32::try_from(u64::from(days) * 86_400).map_err(|_| Problem::ExpiryTooFar {
                days: u64::from(days),
            })?;
        set_expiry(&mut secret, seconds)?;
    }
    Ok(secret)
}

/// The OpenPGP keyring over the store, the OS keychain and the unlock
/// cache.
pub struct Keyring<'a> {
    store: &'a Store,
    secrets: &'a dyn SecretStore,
    unlocked: &'a UnlockCache,
    now: Option<i64>,
    cards: Option<&'a dyn nail_pkcs11::KeyResolver>,
}

impl std::fmt::Debug for Keyring<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Keyring")
            .field("now", &self.now)
            .field("smartcards", &self.cards.is_some())
            .finish_non_exhaustive()
    }
}

impl<'a> Keyring<'a> {
    /// A keyring view.
    pub fn new(store: &'a Store, secrets: &'a dyn SecretStore, unlocked: &'a UnlockCache) -> Self {
        Self {
            store,
            secrets,
            unlocked,
            now: None,
            cards: None,
        }
    }

    /// Uses smartcard keys through `cards` (M4c WP122). Without it, keys
    /// whose secret is on a card are [`Error::Unsupported`].
    #[must_use]
    pub fn with_smartcards(mut self, cards: &'a dyn nail_pkcs11::KeyResolver) -> Self {
        self.cards = Some(cards);
        self
    }

    fn cards(&self) -> Result<&'a dyn nail_pkcs11::KeyResolver> {
        self.cards
            .ok_or_else(|| Error::Unsupported("smartcards are not available here".into()))
    }

    /// The fingerprint of the keyring key (primary or subkey) a card key
    /// belongs to, for `SmartcardObject.openpgp_fingerprint`.
    pub fn fingerprint_for_card_key(
        &self,
        material: &nail_pkcs11::PublicKeyMaterial,
    ) -> Result<Option<(KeyId, String)>> {
        for (stored, public, _) in self.all_public()? {
            if let Some((_, fingerprint)) = crate::smartcard::matching_component(&public, material)
            {
                return Ok(Some((stored.id, fingerprint)));
            }
        }
        Ok(None)
    }

    /// Attaches a key on a smartcard to the keyring key it belongs to
    /// (`UseSmartcardKey`): the OpenPGP public key must be in the keyring
    /// already (the card holds no certificate); its primary key or a
    /// subkey must have the card key's public key. The entry's secret then
    /// stays on the card; a second card key of the same OpenPGP key (an
    /// OpenPGP card's separate signing and decryption keys) is added to
    /// the reference.
    pub fn use_smartcard_key(&self, key: &SmartcardKeyRef) -> Result<CryptoKey> {
        let card = self.cards()?.card_key(key)?;
        let material = card.public_key().ok_or_else(|| {
            Error::NoKey("the smartcard does not show the key's public part".into())
        })?;
        let (stored, _) = self
            .all_public()?
            .into_iter()
            .find_map(|(stored, public, _)| {
                crate::smartcard::matching_component(&public, material).map(|_| (stored, public))
            })
            .ok_or_else(|| {
                Error::NoKey("import the OpenPGP public key of the smartcard key first".into())
            })?;
        let mut refs = match &stored.data.secret {
            Some(secret) if secret.storage == SecretStorage::Smartcard => secret
                .reference
                .as_deref()
                .map(crate::smartcard::parse_references)
                .transpose()?
                .unwrap_or_default(),
            _ => Vec::new(),
        };
        if !refs.contains(key) {
            refs.push(key.clone());
        }
        let mut data = stored.data.clone();
        data.source = KeySource::Smartcard;
        data.secret = Some(SecretKeyData {
            storage: SecretStorage::Smartcard,
            data: None,
            reference: Some(crate::smartcard::reference_of(&refs)),
        });
        let (stored, _) = self.store.upsert_key(&data)?;
        Ok(to_crypto_key(&stored, self.now()))
    }

    /// The card keys of a stored smartcard key.
    fn card_refs(stored: &StoredKey) -> Result<Vec<SmartcardKeyRef>> {
        match &stored.data.secret {
            Some(secret) if secret.storage == SecretStorage::Smartcard => {
                crate::smartcard::parse_references(secret.reference.as_deref().unwrap_or("[]"))
            }
            _ => Ok(Vec::new()),
        }
    }

    /// An unlocked view of a key on a smartcard: the signing component's
    /// card key (the PIN is asked when the card signs).
    fn unlocked_card_key(&self, stored: &StoredKey) -> Result<UnlockedKey> {
        let cards = self.cards()?;
        let public = self.public_key(stored)?;
        let info = KeyInfo::analyze(&public)?;
        let component = match info.signing_component(self.now())? {
            None => crate::smartcard::Component::Primary(public.primary_key.clone()),
            Some(index) => crate::smartcard::Component::Subkey(
                public
                    .public_subkeys
                    .get(index)
                    .ok_or_else(|| Error::NoKey("the signing subkey is missing".into()))?
                    .key
                    .clone(),
            ),
        };
        let params = match &component {
            crate::smartcard::Component::Primary(k) => k.public_params().clone(),
            crate::smartcard::Component::Subkey(k) => k.public_params().clone(),
        };
        let mut last_error = None;
        for key_ref in Self::card_refs(stored)? {
            match cards.card_key(&key_ref) {
                Ok(card)
                    if card
                        .public_key()
                        .is_some_and(|m| crate::smartcard::same_key(&params, m)) =>
                {
                    return Ok(UnlockedKey {
                        stored: stored.clone(),
                        secret: None,
                        info,
                        password: Zeroizing::new(String::new()),
                        card: Some(crate::smartcard::CardSigningKey::new(component, card)),
                    });
                }
                Ok(_) => {}
                Err(err) => last_error = Some(err),
            }
        }
        Err(match last_error {
            Some(err) => err.into(),
            None => Error::NoKey("no smartcard key for the signing key".into()),
        })
    }

    /// Decrypts the session key of a message with an own key on a
    /// smartcard (RSA). `Ok(None)`: no card key is addressed.
    pub(crate) fn card_session_key(
        &self,
        pkesks: &[&pgp::packet::PublicKeyEncryptedSessionKey],
    ) -> Result<Option<CardDecryption>> {
        let Some(cards) = self.cards else {
            return Ok(None);
        };
        for stored in self
            .store
            .list_keys(Some(CryptoTechnology::OpenPgp), true)?
        {
            let refs = Self::card_refs(&stored)?;
            if refs.is_empty() {
                continue;
            }
            let public = self.public_key(&stored)?;
            let mut components: Vec<&pgp::types::PublicParams> = Vec::new();
            for pkesk in pkesks {
                if pkesk.match_identity(&public.primary_key) {
                    components.push(public.primary_key.public_params());
                }
                for sub in &public.public_subkeys {
                    if pkesk.match_identity(&sub.key) {
                        components.push(sub.key.public_params());
                    }
                }
            }
            if components.is_empty() {
                continue;
            }
            for key_ref in &refs {
                let card = match cards.card_key(key_ref) {
                    Ok(card) => card,
                    Err(err) => {
                        return Ok(Some(CardDecryption::Failed(stored.clone(), err)));
                    }
                };
                let Some(material) = card.public_key() else {
                    continue;
                };
                for pkesk in pkesks {
                    let addressed = (pkesk.match_identity(&public.primary_key)
                        && crate::smartcard::same_key(
                            public.primary_key.public_params(),
                            material,
                        ))
                        || public.public_subkeys.iter().any(|s| {
                            pkesk.match_identity(&s.key)
                                && crate::smartcard::same_key(s.key.public_params(), material)
                        });
                    if !addressed {
                        continue;
                    }
                    let modulus_len = match material {
                        nail_pkcs11::PublicKeyMaterial::Rsa { modulus, .. } => {
                            modulus.iter().skip_while(|b| **b == 0).count()
                        }
                        nail_pkcs11::PublicKeyMaterial::Ec { .. } => 0,
                    };
                    return Ok(Some(
                        match crate::smartcard::card_session_key(&card, pkesk, modulus_len) {
                            Ok(session) => CardDecryption::Session(stored.clone(), session),
                            Err(Error::Smartcard(err)) => {
                                CardDecryption::Failed(stored.clone(), err)
                            }
                            Err(err) => return Err(err),
                        },
                    ));
                }
            }
        }
        Ok(None)
    }

    /// Evaluates validity at a fixed time (tests).
    #[must_use]
    pub fn at(mut self, now: i64) -> Self {
        self.now = Some(now);
        self
    }

    /// The time validity is evaluated at.
    pub fn now(&self) -> i64 {
        self.now.unwrap_or_else(unix_now)
    }

    /// The store.
    pub fn store(&self) -> &Store {
        self.store
    }

    /// OpenPGP keys matching `filter` (own first). An S/MIME-only filter
    /// yields nothing.
    pub fn list(&self, filter: &KeyFilter) -> Result<Vec<CryptoKey>> {
        if filter.technology == Some(CryptoTechnology::Smime) {
            return Ok(Vec::new());
        }
        let now = self.now();
        Ok(self
            .store
            .list_keys(Some(CryptoTechnology::OpenPgp), filter.own_only)?
            .iter()
            .filter(|k| matches_filter(k, filter))
            .map(|k| to_crypto_key(k, now))
            .collect())
    }

    /// One key.
    pub fn get(&self, key_id: &KeyId) -> Result<CryptoKey> {
        Ok(to_crypto_key(&self.store.get_key(key_id)?, self.now()))
    }

    fn stored_openpgp(&self, key_id: &KeyId) -> Result<StoredKey> {
        let key = self.store.get_key(key_id)?;
        if key.data.technology != CryptoTechnology::OpenPgp {
            return Err(Problem::NotOpenPgpKey {
                key: key_id.to_string(),
            }
            .into());
        }
        Ok(key)
    }

    /// The transferable public key of a stored key.
    pub fn public_key(&self, key: &StoredKey) -> Result<SignedPublicKey> {
        key::public_from_binary(&key.data.public_data)
    }

    /// Every stored OpenPGP public key with its analysis (keys that no
    /// longer parse are skipped).
    pub fn all_public(&self) -> Result<Vec<(StoredKey, SignedPublicKey, KeyInfo)>> {
        let mut out = Vec::new();
        for stored in self
            .store
            .list_keys(Some(CryptoTechnology::OpenPgp), false)?
        {
            let Ok(public) = self.public_key(&stored) else {
                continue;
            };
            let Ok(info) = KeyInfo::analyze(&public) else {
                continue;
            };
            out.push((stored, public, info));
        }
        Ok(out)
    }

    fn key_data(
        &self,
        public: &SignedPublicKey,
        info: &KeyInfo,
        source: KeySource,
        secret: Option<SecretKeyData>,
    ) -> Result<KeyData> {
        let now = self.now();
        Ok(KeyData {
            technology: CryptoTechnology::OpenPgp,
            fingerprint: info.fingerprint.clone(),
            user_ids: info.user_ids.clone(),
            emails: info.emails.clone(),
            algorithm: info.algorithm.clone(),
            created_at: info.created_at,
            expires_at: info.expires_at,
            revoked: info.revoked,
            can_sign: info.can_sign(now),
            can_encrypt: info.can_encrypt(now),
            source,
            public_data: key::to_binary(public)?,
            secret,
            issuer: None,
            serial: None,
        })
    }

    /// Stores a public key (merged with a stored one of the same
    /// fingerprint). The source only changes when the new one ranks
    /// higher (Autocrypt over gossip over anything else); own keys keep
    /// theirs. Returns the stored key and whether it is new.
    pub fn store_public(
        &self,
        public: &SignedPublicKey,
        source: KeySource,
    ) -> Result<(StoredKey, bool)> {
        let mut public = public.clone();
        key::strip_foreign(&mut public);
        let info = KeyInfo::analyze(&public)?;
        let existing = self
            .store
            .find_key(CryptoTechnology::OpenPgp, &info.fingerprint)?;
        let (merged, source) = match &existing {
            Some(old) => {
                let old_key = self.public_key(old)?;
                let merged = key::merge(&old_key, &public);
                let keep_old = old.is_own() || source_rank(old.data.source) >= source_rank(source);
                (merged, if keep_old { old.data.source } else { source })
            }
            None => (public, source),
        };
        let info = KeyInfo::analyze(&merged)?;
        let data = self.key_data(&merged, &info, source, None)?;
        Ok(self.store.upsert_key(&data)?)
    }

    /// Stores an own secret key. An unprotected key is protected with a
    /// random keychain secret; a passphrase-protected one is kept as it
    /// is ([`SecretStorage::Passphrase`]). A stored secret part of the
    /// same key is kept (only the public part is merged).
    pub fn store_secret(
        &self,
        secret: &SignedSecretKey,
        source: KeySource,
    ) -> Result<(StoredKey, bool)> {
        let mut secret = secret.clone();
        key::strip_foreign_secret(&mut secret);
        let public = secret.to_public_key();
        let info = KeyInfo::analyze(&public)?;
        let existing = self
            .store
            .find_key(CryptoTechnology::OpenPgp, &info.fingerprint)?;
        if existing.as_ref().is_some_and(StoredKey::is_own) {
            return self.store_public(&public, source);
        }
        let secret_data = if components_protected(&secret) {
            SecretKeyData {
                storage: SecretStorage::Passphrase,
                data: Some(secret_bytes(&secret)?),
                reference: None,
            }
        } else {
            self.protect_with_keychain(&mut secret, &info.fingerprint)?
        };
        let merged = match &existing {
            Some(old) => key::merge(&self.public_key(old)?, &public),
            None => public,
        };
        let info = KeyInfo::analyze(&merged)?;
        let data = self.key_data(&merged, &info, source, Some(secret_data))?;
        Ok(self.store.upsert_key(&data)?)
    }

    fn protect_with_keychain(
        &self,
        secret: &mut SignedSecretKey,
        fingerprint: &str,
    ) -> Result<SecretKeyData> {
        let mut random = Zeroizing::new([0u8; 32]);
        thread_rng().fill(&mut random[..]);
        let value = Zeroizing::new(base64::engine::general_purpose::STANDARD.encode(&random[..]));
        let reference = keychain_ref(fingerprint);
        protect(secret, &Password::from(value.as_str()), false)?;
        self.secrets.set(&reference, &value)?;
        Ok(SecretKeyData {
            storage: SecretStorage::Keychain,
            data: Some(secret_bytes(secret)?),
            reference: Some(reference.0),
        })
    }

    /// Imports keys from `ImportKeys` data (armored text or base64 of
    /// binary packets).
    pub fn import(&self, data: &str, encoding: KeyEncoding) -> Result<KeyImportResult> {
        let bytes = match encoding {
            KeyEncoding::Text => data.as_bytes().to_vec(),
            KeyEncoding::Base64 => base64::engine::general_purpose::STANDARD
                .decode(data.trim())
                .map_err(|e| Error::NotBase64(e.to_string()))?,
        };
        self.import_bytes(&bytes, KeySource::Imported)
    }

    /// Imports every key in armored or binary data.
    pub fn import_bytes(&self, data: &[u8], source: KeySource) -> Result<KeyImportResult> {
        let (keys, parse_issues) = key::parse_keys(data);
        let mut result = KeyImportResult {
            keys: Vec::new(),
            created: 0,
            updated: 0,
            issues: parse_issues
                .into_iter()
                .map(|(item, message)| KeyImportIssue {
                    item,
                    code: ErrorCode::Crypto,
                    // rPGP's text stays an English `detail` argument.
                    localized: Some(
                        LocalizedMessage::new("pgp-import-unreadable")
                            .arg("detail", message.as_str()),
                    ),
                    message,
                })
                .collect(),
        };
        if keys.is_empty() && result.issues.is_empty() {
            result.issues.push(KeyImportIssue {
                item: "#1".into(),
                code: ErrorCode::InvalidRequest,
                message: "no OpenPGP key found".into(),
                localized: Some(LocalizedMessage::new("pgp-import-no-key")),
            });
        }
        let now = self.now();
        for parsed in keys {
            let stored = match &parsed {
                ParsedKey::Public(public) => self.store_public(public, source),
                ParsedKey::Secret(secret) => self.store_secret(secret, source),
            };
            match stored {
                Ok((key, created)) => {
                    if created {
                        result.created += 1;
                    } else {
                        result.updated += 1;
                    }
                    result.keys.push(to_crypto_key(&key, now));
                }
                Err(e) => {
                    let item = KeyInfo::analyze(&parsed.public()).map_or_else(
                        |_| hex_fingerprint(&parsed.public()),
                        |info| info.fingerprint,
                    );
                    result.issues.push(KeyImportIssue {
                        item,
                        code: e.code(),
                        message: e.to_string(),
                        localized: Some(import_issue(&e)),
                    });
                }
            }
        }
        Ok(result)
    }

    /// Generates an own key for `name <email>` (`GenerateKey`).
    pub fn generate(
        &self,
        name: &str,
        email: &str,
        algorithm: KeyAlgorithm,
        expires_days: Option<u32>,
        protection: &SecretProtection,
    ) -> Result<CryptoKey> {
        self.generate_with_bits(name, email, algorithm, 4096, expires_days, protection)
    }

    pub(crate) fn generate_with_bits(
        &self,
        name: &str,
        email: &str,
        algorithm: KeyAlgorithm,
        rsa_bits: u32,
        expires_days: Option<u32>,
        protection: &SecretProtection,
    ) -> Result<CryptoKey> {
        let email = email.trim();
        if key::email_of_user_id(email).is_none() {
            return Err(Problem::NotAnAddress {
                address: email.to_owned(),
            }
            .into());
        }
        let name = name.trim();
        let user_id = if name.is_empty() {
            format!("<{email}>")
        } else {
            format!("{name} <{email}>")
        };
        let mut secret = generate_secret(&user_id, algorithm, rsa_bits, expires_days)?;
        let (stored, _) = match protection {
            SecretProtection::Keychain => self.store_secret(&secret, KeySource::Generated)?,
            SecretProtection::Passphrase { passphrase } => {
                if passphrase.is_empty() {
                    return Err(Problem::EmptyPassphrase.into());
                }
                protect(&mut secret, &Password::from(passphrase.as_str()), true)?;
                self.store_secret(&secret, KeySource::Generated)?
            }
        };
        Ok(to_crypto_key(&stored, self.now()))
    }

    /// Exports a public key (`ExportPublicKey`).
    pub fn export_public(&self, key_id: &KeyId, encoding: KeyEncoding) -> Result<ExportedKey> {
        let stored = self.stored_openpgp(key_id)?;
        let public = self.public_key(&stored)?;
        let stem = stored
            .data
            .emails
            .first()
            .cloned()
            .unwrap_or_else(|| stored.data.fingerprint.clone());
        let (filename, data) = match encoding {
            KeyEncoding::Text => (format!("{stem}.asc"), key::to_armored(&public)?),
            KeyEncoding::Base64 => (
                format!("{stem}.pgp"),
                base64::engine::general_purpose::STANDARD.encode(key::to_binary(&public)?),
            ),
        };
        Ok(ExportedKey {
            filename,
            mime: "application/pgp-keys".into(),
            encoding,
            data,
        })
    }

    /// Checks a passphrase and keeps it for `remember_minutes` (`null` =
    /// until Nail quits). Keychain-protected keys need no unlocking.
    pub fn unlock(
        &self,
        key_id: &KeyId,
        passphrase: &str,
        remember_minutes: Option<u32>,
    ) -> Result<()> {
        let stored = self.stored_openpgp(key_id)?;
        let Some(secret) = &stored.data.secret else {
            return Err(Problem::NoSecretKey {
                key: key_id.to_string(),
            }
            .into());
        };
        if secret.storage != SecretStorage::Passphrase {
            return Ok(());
        }
        let key = Self::secret_key(&stored)?;
        if !password_unlocks(&key, &Password::from(passphrase)) {
            return Err(Error::BadPassphrase);
        }
        self.unlocked
            .put(&stored.data.fingerprint, passphrase, remember_minutes);
        Ok(())
    }

    /// Forgets a key's cached passphrase.
    pub fn lock(&self, key_id: &KeyId) -> Result<()> {
        let stored = self.store.get_key(key_id)?;
        self.unlocked.forget(&stored.data.fingerprint);
        Ok(())
    }

    /// Deletes a key (`DeleteKey`), and the keychain entry of its secret.
    pub fn delete(&self, key_id: &KeyId, delete_secret: bool) -> Result<bool> {
        let stored = match self.store.get_key(key_id) {
            Ok(k) => k,
            Err(e) if e.is_not_found() => return Ok(false),
            Err(e) => return Err(e.into()),
        };
        let deleted = self.store.delete_key(key_id, delete_secret)?;
        if deleted {
            self.unlocked.forget(&stored.data.fingerprint);
            if let Some(reference) = stored
                .data
                .secret
                .as_ref()
                .filter(|s| s.storage == SecretStorage::Keychain)
                .and_then(|s| s.reference.clone())
            {
                self.secrets.delete(&SecretRef(reference))?;
            }
        }
        Ok(deleted)
    }

    /// Stores a trust decision (`SetKeyTrust`).
    pub fn set_trust(&self, key_id: &KeyId, decision: TrustDecision) -> Result<CryptoKey> {
        let stored = self.store.get_key(key_id)?;
        self.store.set_trust_decision(
            stored.data.technology,
            &stored.data.fingerprint,
            decision,
        )?;
        self.get(key_id)
    }

    fn secret_key(stored: &StoredKey) -> Result<SignedSecretKey> {
        let data = stored
            .data
            .secret
            .as_ref()
            .and_then(|s| s.data.as_deref())
            .ok_or_else(|| Error::NoKey(format!("{} has no secret key", stored.id)))?;
        key::secret_from_binary(data)
    }

    /// The password of an own key: from the keychain, or the cached
    /// passphrase; `None` when the passphrase is needed.
    fn password_for(&self, stored: &StoredKey) -> Result<Option<Zeroizing<String>>> {
        let Some(secret) = &stored.data.secret else {
            return Ok(None);
        };
        match secret.storage {
            SecretStorage::Keychain => {
                let reference = secret
                    .reference
                    .clone()
                    .ok_or_else(|| Error::NoKey("keychain reference missing".into()))?;
                let value = self.secrets.get(&SecretRef(reference))?.ok_or_else(|| {
                    Error::NoKey(format!(
                        "the keychain entry of {} is missing",
                        stored.data.fingerprint
                    ))
                })?;
                Ok(Some(Zeroizing::new(value)))
            }
            SecretStorage::Passphrase => Ok(self.unlocked.get(&stored.data.fingerprint)),
            SecretStorage::WindowsStore => Err(Problem::WindowsStore.into()),
            // M4c WP122: keys on a smartcard sign / decrypt on the card;
            // the card asks for its PIN itself.
            SecretStorage::Smartcard => Ok(Some(Zeroizing::new(String::new()))),
        }
    }

    /// An own key ready to use; [`Error::Locked`] when its passphrase is
    /// needed.
    pub fn unlocked_key(&self, stored: &StoredKey) -> Result<UnlockedKey> {
        if stored
            .data
            .secret
            .as_ref()
            .is_some_and(|s| s.storage == SecretStorage::Smartcard)
        {
            return self.unlocked_card_key(stored);
        }
        let secret = Self::secret_key(stored)?;
        let password = self.password_for(stored)?.ok_or_else(|| Error::Locked {
            key_id: stored.id.clone(),
        })?;
        let info = KeyInfo::analyze(&secret.to_public_key())?;
        Ok(UnlockedKey {
            stored: stored.clone(),
            secret: Some(secret),
            info,
            password,
            card: None,
        })
    }

    /// Own keys with their secret key (not unlocked).
    pub(crate) fn own_secret_keys(&self) -> Result<Vec<(StoredKey, SignedSecretKey)>> {
        let mut out = Vec::new();
        for stored in self
            .store
            .list_keys(Some(CryptoTechnology::OpenPgp), true)?
        {
            if let Ok(secret) = Self::secret_key(&stored) {
                out.push((stored, secret));
            }
        }
        Ok(out)
    }

    /// Tries to get the password of an own key without failing when it is
    /// locked (decryption: `None` = locked).
    pub(crate) fn try_password(&self, stored: &StoredKey) -> Result<Option<Zeroizing<String>>> {
        self.password_for(stored)
    }

    /// The own key to sign with for an identity: the configured
    /// fingerprint (`IdentityCrypto::openpgp_key`), else the newest own
    /// key for `email` that can sign now.
    pub fn own_key_for(&self, email: &str, fingerprint: Option<&str>) -> Result<StoredKey> {
        let now = self.now();
        if let Some(fingerprint) = fingerprint {
            return self
                .store
                .find_key(CryptoTechnology::OpenPgp, fingerprint)?
                .filter(StoredKey::is_own)
                .ok_or_else(|| Error::NoKey(format!("no own OpenPGP key {fingerprint}")));
        }
        self.store
            .keys_for_email(email, Some(CryptoTechnology::OpenPgp))?
            .into_iter()
            .filter(|k| k.is_own() && k.data.can_sign && !k.data.revoked)
            .filter(|k| k.data.expires_at.is_none_or(|e| e > now))
            .max_by_key(|k| k.data.created_at)
            .ok_or_else(|| Error::NoKey(format!("no own OpenPGP key for {email}")))
    }

    /// The trust of a key **for one address**. Autocrypt vouches only for
    /// the binding of a key to the address of its header: a key learned
    /// from Autocrypt (gossip) has that level only for an address whose
    /// peer record (in any account) names it; for every other address of
    /// the key it is `unknown`. User decisions and own keys apply to all
    /// addresses ([`effective_trust`]).
    pub fn trust_for(&self, key: &StoredKey, email: &str) -> Result<TrustLevel> {
        let base = effective_trust(key);
        if !matches!(base, TrustLevel::Autocrypt | TrustLevel::AutocryptGossip) {
            return Ok(base);
        }
        let fingerprint = &key.data.fingerprint;
        let names = |f: &Option<String>| {
            f.as_deref()
                .is_some_and(|f| f.eq_ignore_ascii_case(fingerprint))
        };
        let peers = self.store.autocrypt_peers_for_email(email)?;
        let level = if peers.iter().any(|p| names(&p.key_fingerprint)) {
            TrustLevel::Autocrypt
        } else if peers.iter().any(|p| names(&p.gossip_fingerprint)) {
            TrustLevel::AutocryptGossip
        } else {
            TrustLevel::Unknown
        };
        Ok(level.min(base))
    }

    /// The best OpenPGP key of a recipient: usable keys first, then by
    /// trust for this address ([`Self::trust_for`]), then the newest.
    /// `None` when there is no key at all.
    pub fn recipient_key(&self, email: &str) -> Result<Option<(RecipientKey, StoredKey)>> {
        let now = self.now();
        let email = email.trim().to_lowercase();
        let mut candidates: Vec<(RecipientKey, StoredKey)> = Vec::new();
        for k in self
            .store
            .keys_for_email(&email, Some(CryptoTechnology::OpenPgp))?
        {
            let trust = self.trust_for(&k, &email)?;
            candidates.push({
                let state = if k.data.revoked {
                    RecipientKeyState::Revoked
                } else if k.data.expires_at.is_some_and(|e| e <= now) {
                    RecipientKeyState::Expired
                } else if trust == TrustLevel::Rejected {
                    RecipientKeyState::Rejected
                } else if !self.can_encrypt_now(&k) {
                    RecipientKeyState::CannotEncrypt
                } else {
                    RecipientKeyState::Usable
                };
                (
                    RecipientKey {
                        key_id: k.id.clone(),
                        fingerprint: k.data.fingerprint.clone(),
                        state,
                        trust,
                        source: k.data.source,
                    },
                    k,
                )
            });
        }
        candidates.sort_by_key(|(r, k)| {
            (
                r.state == RecipientKeyState::Usable,
                r.trust,
                k.data.created_at,
            )
        });
        Ok(candidates.pop())
    }

    fn can_encrypt_now(&self, key: &StoredKey) -> bool {
        self.public_key(key)
            .ok()
            .and_then(|p| KeyInfo::analyze(&p).ok())
            .is_some_and(|info| info.encryption_component(self.now()).is_ok())
    }

    /// Public keys to encrypt to, one per address (their best usable
    /// key). [`Error::NoKey`] names the addresses without one.
    pub fn encryption_keys(&self, emails: &[String]) -> Result<Vec<SignedPublicKey>> {
        let mut keys = Vec::new();
        let mut missing = Vec::new();
        for email in emails {
            match self.recipient_key(email)? {
                Some((r, stored)) if r.state == RecipientKeyState::Usable => {
                    keys.push(self.public_key(&stored)?);
                }
                _ => missing.push(email.clone()),
            }
        }
        if missing.is_empty() {
            Ok(keys)
        } else {
            Err(Error::NoKey(format!(
                "no usable OpenPGP key for {}",
                missing.join(", ")
            )))
        }
    }
}

fn secret_bytes(secret: &SignedSecretKey) -> Result<Vec<u8>> {
    Ok(secret.to_bytes()?)
}

fn hex_fingerprint(public: &SignedPublicKey) -> String {
    key::hex_upper(public.primary_key.fingerprint().as_bytes())
}

/// The user-facing text of a key that could not be stored on import
/// (library details stay an English `detail` argument).
fn import_issue(error: &Error) -> LocalizedMessage {
    match error {
        Error::Crypto(detail)
        | Error::Invalid(detail)
        | Error::Unsupported(detail)
        | Error::NoKey(detail)
        | Error::NotBase64(detail) => {
            LocalizedMessage::new("pgp-import-unusable").arg("detail", detail.as_str())
        }
        Error::Problem(problem) => problem.localized(),
        Error::Secrets(_) => LocalizedMessage::new("pgp-import-keychain"),
        Error::Smartcard(err) => LocalizedMessage::new(err.kind.message_id()),
        Error::Store(_) | Error::Locked { .. } | Error::BadPassphrase => {
            LocalizedMessage::new("pgp-import-store")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{fixture, fixture_text};
    use nail_api::engine::MemorySecretStore;
    use pretty_assertions::assert_eq;

    pub(crate) struct Env {
        pub store: Store,
        pub secrets: MemorySecretStore,
        pub cache: UnlockCache,
    }

    impl Env {
        pub(super) fn new() -> Self {
            Self {
                store: Store::open_in_memory().unwrap(),
                secrets: MemorySecretStore::new(),
                cache: UnlockCache::new(),
            }
        }

        pub(super) fn keyring(&self) -> Keyring<'_> {
            Keyring::new(&self.store, &self.secrets, &self.cache)
        }
    }

    #[test]
    fn import_gnupg_public_keys_and_merge_revocation() {
        let env = Env::new();
        let ring = env.keyring();
        let result = ring
            .import(&fixture_text("alice.pub.asc"), KeyEncoding::Text)
            .unwrap();
        assert_eq!((result.created, result.updated), (1, 0));
        assert!(result.issues.is_empty(), "{:?}", result.issues);
        let alice = &result.keys[0];
        assert_eq!(alice.technology, CryptoTechnology::OpenPgp);
        assert_eq!(alice.emails, vec!["alice@example.org"]);
        assert_eq!(alice.algorithm, "Ed25519");
        assert_eq!(alice.created_at, "2026-01-01T12:00:00Z");
        assert!(alice.can_sign && alice.can_encrypt && !alice.has_secret);
        assert_eq!(alice.trust, TrustLevel::Unknown);
        assert_eq!(alice.source, KeySource::Imported);

        // Re-import: updated, not created.
        let again = ring
            .import(&fixture_text("alice.pub.asc"), KeyEncoding::Text)
            .unwrap();
        assert_eq!((again.created, again.updated), (0, 1));

        // eve without her revocation, then with it: the merge keeps it.
        let (keys, _) = key::parse_keys(&fixture("eve.pub.asc"));
        let mut eve = keys[0].public();
        let revocations = std::mem::take(&mut eve.details.revocation_signatures);
        assert!(!revocations.is_empty(), "{revocations:?}");
        let (stored, _) = ring.store_public(&eve, KeySource::Imported).unwrap();
        assert!(!stored.data.revoked);
        let result = ring
            .import_bytes(&fixture("eve.pub.asc"), KeySource::Imported)
            .unwrap();
        assert!(result.keys[0].revoked);
    }

    #[test]
    fn import_base64_binary_and_report_garbage() {
        let env = Env::new();
        let ring = env.keyring();
        let (keys, _) = key::parse_keys(&fixture("bob.pub.asc"));
        let binary = key::to_binary(&keys[0].public()).unwrap();
        let data = base64::engine::general_purpose::STANDARD.encode(binary);
        let result = ring.import(&data, KeyEncoding::Base64).unwrap();
        assert_eq!(result.created, 1);
        let refused = ring.import("***", KeyEncoding::Base64).unwrap_err();
        assert!(matches!(refused, Error::NotBase64(_)), "{refused:?}");
        let api: nail_api::ApiError = refused.into();
        assert_eq!(api.code, ErrorCode::InvalidRequest);
        assert_eq!(
            api.localized.map(|l| l.id).as_deref(),
            Some("pgp-import-not-base64")
        );
        let result = ring.import("hello", KeyEncoding::Text).unwrap();
        assert!(result.keys.is_empty(), "{:?}", result.keys);
        assert_eq!(result.issues.len(), 1);
        // Nail's own text goes to the UI as a Fluent id (ADR 0043).
        let localized = result.issues[0].localized.as_ref().expect("localized");
        assert!(localized.id.starts_with("pgp-import-"), "{localized:?}");
    }

    #[test]
    fn import_issues_are_localized_with_catalog_ids() {
        let catalog = crate::testutil::crypto_catalog();
        let has = |id: &str| catalog.lines().any(|l| l.starts_with(&format!("{id} =")));
        let cases = [
            import_issue(&Error::Crypto("bad self-signature".into())),
            import_issue(&Error::Invalid("no user id".into())),
            import_issue(&Error::Secrets(nail_api::engine::EngineError::permanent(
                ErrorCode::Internal,
                "keychain down",
            ))),
            import_issue(&Error::BadPassphrase),
            LocalizedMessage::new("pgp-import-unreadable"),
            LocalizedMessage::new("pgp-import-no-key"),
        ];
        for message in cases {
            assert!(has(&message.id), "{} is not in core-crypto.ftl", message.id);
        }
        assert_eq!(
            import_issue(&Error::Crypto("bad self-signature".into())).args["detail"],
            "bad self-signature".into()
        );
    }

    #[test]
    fn imported_secret_keys_keep_or_get_protection() {
        let env = Env::new();
        let ring = env.keyring();
        // alice's secret key is protected with "alice": stored as is.
        let result = ring
            .import(&fixture_text("alice.sec.asc"), KeyEncoding::Text)
            .unwrap();
        let alice = &result.keys[0];
        assert!(alice.has_secret);
        assert_eq!(alice.secret_storage, Some(SecretStorage::Passphrase));
        assert_eq!(alice.trust, TrustLevel::Own);
        // bob's is unprotected: protected with a keychain secret.
        let result = ring
            .import(&fixture_text("bob.sec.asc"), KeyEncoding::Text)
            .unwrap();
        let bob = &result.keys[0];
        assert_eq!(bob.secret_storage, Some(SecretStorage::Keychain));
        assert_eq!(env.secrets.len(), 1);
        let stored = env.store.get_key(&bob.id).unwrap();
        let secret =
            key::secret_from_binary(stored.data.secret.unwrap().data.as_deref().unwrap()).unwrap();
        assert!(components_protected(&secret));
        // The store row alone does not unlock it.
        assert!(!password_unlocks(&secret, &Password::from("")));

        // Importing the public key again keeps the secret part.
        ring.import(&fixture_text("bob.pub.asc"), KeyEncoding::Text)
            .unwrap();
        assert!(env.store.get_key(&bob.id).unwrap().is_own());
    }

    #[test]
    fn unlock_checks_the_passphrase_and_caches_it() {
        let env = Env::new();
        let ring = env.keyring();
        let alice = ring
            .import(&fixture_text("alice.sec.asc"), KeyEncoding::Text)
            .unwrap()
            .keys
            .remove(0);
        let stored = env.store.get_key(&alice.id).unwrap();
        assert!(matches!(
            ring.unlocked_key(&stored),
            Err(Error::Locked { key_id }) if key_id == alice.id
        ));
        assert!(matches!(
            ring.unlock(&alice.id, "wrong", None),
            Err(Error::BadPassphrase)
        ));
        ring.unlock(&alice.id, "alice", Some(5)).unwrap();
        assert!(env.cache.is_unlocked(&alice.fingerprint));
        let unlocked = ring.unlocked_key(&stored).unwrap();
        assert!(unlocked.signing_key(ring.now()).is_ok());
        ring.lock(&alice.id).unwrap();
        assert!(ring.unlocked_key(&stored).is_err());
        // A zero-minute unlock expires at once.
        ring.unlock(&alice.id, "alice", Some(0)).unwrap();
        assert!(!env.cache.is_unlocked(&alice.fingerprint));
        // Public keys cannot be unlocked.
        let bob = ring
            .import(&fixture_text("bob.pub.asc"), KeyEncoding::Text)
            .unwrap()
            .keys
            .remove(0);
        assert!(matches!(
            ring.unlock(&bob.id, "x", None),
            Err(Error::Problem(Problem::NoSecretKey { .. }))
        ));
    }

    #[test]
    fn generate_curve25519_with_keychain_and_passphrase() {
        let env = Env::new();
        let ring = env.keyring();
        let key = ring
            .generate(
                "Ada Lovelace",
                "ada@example.org",
                KeyAlgorithm::Curve25519,
                None,
                &SecretProtection::Keychain,
            )
            .unwrap();
        assert_eq!(key.user_ids, vec!["Ada Lovelace <ada@example.org>"]);
        assert_eq!(key.algorithm, "Ed25519");
        assert_eq!(key.source, KeySource::Generated);
        assert_eq!(key.secret_storage, Some(SecretStorage::Keychain));
        assert!(key.can_sign && key.can_encrypt && key.expires_at.is_none());
        assert_eq!(key.trust, TrustLevel::Own);
        let stored = env.store.get_key(&key.id).unwrap();
        let unlocked = ring.unlocked_key(&stored).unwrap();
        assert_eq!(unlocked.info.subkeys[0].algorithm, "X25519");
        assert_eq!(
            stored.data.secret.unwrap().reference.unwrap(),
            keychain_ref(&key.fingerprint).0
        );

        let locked = ring
            .generate(
                "",
                "ada@example.org",
                KeyAlgorithm::Curve25519,
                Some(30),
                &SecretProtection::Passphrase {
                    passphrase: "correct horse".into(),
                },
            )
            .unwrap();
        assert_eq!(locked.user_ids, vec!["<ada@example.org>"]);
        assert_eq!(locked.secret_storage, Some(SecretStorage::Passphrase));
        let expires = chrono::DateTime::parse_from_rfc3339(locked.expires_at.as_deref().unwrap())
            .unwrap()
            .timestamp();
        let created = chrono::DateTime::parse_from_rfc3339(&locked.created_at)
            .unwrap()
            .timestamp();
        assert_eq!(expires - created, 30 * 86_400);
        // Both keys verify their (re-signed) bindings.
        let public = ring
            .public_key(&env.store.get_key(&locked.id).unwrap())
            .unwrap();
        let info = KeyInfo::analyze(&public).unwrap();
        assert_eq!(info.subkeys[0].expires_at, info.expires_at);
        assert!(info.can_encrypt(created + 10));
        assert!(info.encryption_component(created + 31 * 86_400).is_err());
        ring.unlock(&locked.id, "correct horse", None).unwrap();

        // The newest usable own key for the address signs.
        let own = ring.own_key_for("ADA@example.org", None).unwrap();
        assert!(own.id == locked.id || own.id == key.id);
        let own = ring.own_key_for("x@y", Some(&key.fingerprint)).unwrap();
        assert_eq!(own.id, key.id);
        assert!(matches!(
            ring.own_key_for("nobody@example.org", None),
            Err(Error::NoKey(_))
        ));

        assert!(matches!(
            ring.generate(
                "A",
                "not an address",
                KeyAlgorithm::Curve25519,
                None,
                &SecretProtection::Keychain
            ),
            Err(Error::Problem(Problem::NotAnAddress { .. }))
        ));
        assert!(matches!(
            ring.generate(
                "A",
                "a@b.example",
                KeyAlgorithm::Curve25519,
                None,
                &SecretProtection::Passphrase {
                    passphrase: String::new()
                }
            ),
            Err(Error::Problem(Problem::EmptyPassphrase))
        ));
    }

    #[test]
    fn generate_rsa() {
        let env = Env::new();
        let ring = env.keyring();
        // 2048 bits keeps the test fast; `generate` uses 4096.
        let key = ring
            .generate_with_bits(
                "Old School",
                "rsa@example.org",
                KeyAlgorithm::Rsa4096,
                2048,
                None,
                &SecretProtection::Keychain,
            )
            .unwrap();
        assert_eq!(key.algorithm, "RSA 2048");
        assert!(key.can_sign && key.can_encrypt);
    }

    #[test]
    fn export_list_trust_and_delete() {
        let env = Env::new();
        let ring = env.keyring();
        let own = ring
            .generate(
                "Ada",
                "ada@example.org",
                KeyAlgorithm::Curve25519,
                None,
                &SecretProtection::Keychain,
            )
            .unwrap();
        let bob = ring
            .import(&fixture_text("bob.pub.asc"), KeyEncoding::Text)
            .unwrap()
            .keys
            .remove(0);

        let exported = ring.export_public(&bob.id, KeyEncoding::Text).unwrap();
        assert_eq!(exported.filename, "bob@example.net.asc");
        assert_eq!(exported.mime, "application/pgp-keys");
        assert!(
            exported
                .data
                .starts_with("-----BEGIN PGP PUBLIC KEY BLOCK-----")
        );
        let binary = ring.export_public(&own.id, KeyEncoding::Base64).unwrap();
        assert_eq!(binary.filename, "ada@example.org.pgp");
        // The export never contains secret material.
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(&binary.data)
            .unwrap();
        let (keys, _) = key::parse_keys(&bytes);
        assert!(matches!(keys[0], ParsedKey::Public(_)));

        let all = ring.list(&KeyFilter::default()).unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].id, own.id, "own keys first");
        let own_only = ring
            .list(&KeyFilter {
                own_only: true,
                ..KeyFilter::default()
            })
            .unwrap();
        assert_eq!(own_only.len(), 1);
        let by_text = ring
            .list(&KeyFilter {
                text: Some("BOB@".into()),
                ..KeyFilter::default()
            })
            .unwrap();
        assert_eq!(by_text.len(), 1);
        let fp_part: String = bob.fingerprint[..8].to_lowercase();
        let spaced = format!("{} {}", &fp_part[..4], &fp_part[4..]);
        assert_eq!(
            ring.list(&KeyFilter {
                text: Some(spaced),
                ..KeyFilter::default()
            })
            .unwrap()
            .len(),
            1
        );
        assert_eq!(
            ring.list(&KeyFilter {
                technology: Some(CryptoTechnology::Smime),
                ..KeyFilter::default()
            })
            .unwrap()
            .len(),
            0
        );

        let verified = ring.set_trust(&bob.id, TrustDecision::Verified).unwrap();
        assert_eq!(verified.trust, TrustLevel::Verified);
        let rejected = ring.set_trust(&bob.id, TrustDecision::Rejected).unwrap();
        assert_eq!(rejected.trust, TrustLevel::Rejected);
        let unset = ring.set_trust(&bob.id, TrustDecision::Unset).unwrap();
        assert_eq!(unset.trust, TrustLevel::Unknown);

        // An own key needs delete_secret; its keychain entry goes with it.
        assert!(matches!(ring.delete(&own.id, false), Err(Error::Store(_))));
        assert_eq!(env.secrets.len(), 1);
        assert!(ring.delete(&own.id, true).unwrap());
        assert_eq!(env.secrets.len(), 0);
        assert!(!ring.delete(&own.id, true).unwrap());
        assert!(ring.delete(&bob.id, false).unwrap());
    }

    #[test]
    fn recipient_keys_prefer_usable_and_trusted() {
        let env = Env::new();
        let ring = env.keyring().at(1_790_000_000);
        for name in ["alice.pub.asc", "dave.pub.asc", "eve.pub.asc"] {
            ring.import(&fixture_text(name), KeyEncoding::Text).unwrap();
        }
        let (alice, _) = ring.recipient_key("Alice@example.org").unwrap().unwrap();
        assert_eq!(alice.state, RecipientKeyState::Usable);
        assert_eq!(alice.trust, TrustLevel::Unknown);
        let (dave, _) = ring.recipient_key("dave@example.org").unwrap().unwrap();
        assert_eq!(dave.state, RecipientKeyState::Expired);
        let (eve, _) = ring.recipient_key("eve@example.org").unwrap().unwrap();
        assert_eq!(eve.state, RecipientKeyState::Revoked);
        assert!(ring.recipient_key("nobody@example.org").unwrap().is_none());

        ring.set_trust(&alice.key_id, TrustDecision::Rejected)
            .unwrap();
        let (alice, _) = ring.recipient_key("alice@example.org").unwrap().unwrap();
        assert_eq!(alice.state, RecipientKeyState::Rejected);

        let err = ring
            .encryption_keys(&["alice@example.org".into(), "dave@example.org".into()])
            .unwrap_err();
        assert!(
            matches!(&err, Error::NoKey(m) if m.contains("alice@example.org") && m.contains("dave@example.org")),
            "{err}"
        );
        ring.set_trust(&alice.key_id, TrustDecision::Unset).unwrap();
        assert_eq!(
            ring.encryption_keys(&["alice@example.org".into()])
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn autocrypt_source_ranks_above_imported() {
        let env = Env::new();
        let ring = env.keyring();
        let (keys, _) = key::parse_keys(&fixture("alice.pub.asc"));
        let alice = keys[0].public();
        let (stored, _) = ring
            .store_public(&alice, KeySource::AutocryptGossip)
            .unwrap();
        assert_eq!(effective_trust(&stored), TrustLevel::AutocryptGossip);
        let (stored, _) = ring.store_public(&alice, KeySource::Autocrypt).unwrap();
        assert_eq!(effective_trust(&stored), TrustLevel::Autocrypt);
        let (stored, _) = ring.store_public(&alice, KeySource::Wkd).unwrap();
        assert_eq!(stored.data.source, KeySource::Autocrypt);
        let (stored, _) = ring
            .store_public(&alice, KeySource::AutocryptGossip)
            .unwrap();
        assert_eq!(stored.data.source, KeySource::Autocrypt);
    }

    #[test]
    fn unlock_cache_debug_hides_passphrases() {
        let cache = UnlockCache::new();
        cache.put("AB", "secret", None);
        let debug = format!("{cache:?}");
        assert!(!debug.contains("secret"), "{debug}");
        cache.clear();
        assert!(!cache.is_unlocked("AB"));
    }
}
