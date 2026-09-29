//! S/MIME in the store's keyring (schema v5, ADR 0029): importing
//! certificates and PKCS#12 files, protecting private keys, the protocol's
//! `CryptoKey` view, and the keys a read or a send needs.
//!
//! Private keys are stored only encrypted ([`nail_store::SecretKeyData`]):
//!
//! * `keychain`: AES-256-GCM with a random 256-bit key kept in the OS
//!   keychain (`keychain:nail/smime-key/<fingerprint>`), no prompt;
//! * `passphrase`: AES-256-GCM with a key from PBKDF2-HMAC-SHA256
//!   (600 000 iterations, random salt); [`Keyring::unlock`] keeps the
//!   decrypted key in memory ([`UnlockCache`]) for a while;
//! * `windows-store`: nothing is stored but the certificate's thumbprint;
//!   the key stays in the Windows certificate store (CNG).
//!
//! The blob format is `NSM1 | mode | [iterations u32 | salt 16] | nonce 12
//! | ciphertext+tag`, with `NSM1 | mode | fingerprint` as associated data,
//! so a blob cannot be moved to another certificate.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

use aes_gcm::aead::{Aead, KeyInit, Payload};
use base64::Engine as _;
use nail_api::engine::SecretStore;
use nail_api::security::{
    ChainState, CryptoKey, CryptoTechnology, KeyEncoding, KeyFilter, KeyImportResult, KeySource,
    RecipientKey, RecipientKeyState, SecretProtection, SecretStorage, TrustDecision, TrustLevel,
};
use nail_api::{KeyId, SecretRef};
use nail_store::{KeyData, SecretKeyData, Store, StoredKey};
use rand::RngCore;
use rand::rngs::OsRng;
use zeroize::Zeroizing;

use crate::cert::{Cert, Purpose};
use crate::chain::{self, TrustAnchors};
use crate::enveloped::DecryptionKey;
use crate::error::{Error, Result};
use crate::keys::{PrivateKeyOps, SoftwareKey};
use crate::mime::{self, ReadContext, ReadOutcome};
use crate::signed::Signer;
use crate::time::rfc3339;

const MAGIC: &[u8; 4] = b"NSM1";
const MODE_KEYCHAIN: u8 = 1;
const MODE_PASSPHRASE: u8 = 2;
/// PBKDF2-HMAC-SHA256 iterations for new passphrase-protected keys
/// (OWASP 2023 recommendation).
pub const PBKDF2_ITERATIONS: u32 = 600_000;

/// Unlocked keys by fingerprint, with their expiry (Unix seconds).
type UnlockedKeys = HashMap<String, (Arc<SoftwareKey>, Option<i64>)>;

/// Private keys unlocked with a passphrase (and keychain-protected keys
/// once opened), kept in memory until they expire or Nail quits. Shared by
/// all [`Keyring`] handles of a process.
#[derive(Default)]
pub struct UnlockCache {
    entries: Mutex<UnlockedKeys>,
}

impl std::fmt::Debug for UnlockCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UnlockCache")
            .field("keys", &self.lock().len())
            .finish()
    }
}

impl UnlockCache {
    /// An empty cache.
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, UnlockedKeys> {
        self.entries.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn get(&self, fingerprint: &str, now: i64) -> Option<Arc<SoftwareKey>> {
        let mut entries = self.lock();
        match entries.get(fingerprint) {
            Some((_, Some(until))) if *until <= now => {
                entries.remove(fingerprint);
                None
            }
            Some((key, _)) => Some(key.clone()),
            None => None,
        }
    }

    fn put(&self, fingerprint: &str, key: Arc<SoftwareKey>, until: Option<i64>) {
        self.lock().insert(fingerprint.to_owned(), (key, until));
    }

    fn forget(&self, fingerprint: &str) {
        self.lock().remove(fingerprint);
    }

    /// Forgets every unlocked key.
    pub fn lock_all(&self) {
        self.lock().clear();
    }
}

/// A public key export ([`nail_api::Response::KeyExported`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportedKey {
    /// Suggested file name.
    pub filename: String,
    /// MIME type.
    pub mime: String,
    /// How `data` is encoded.
    pub encoding: KeyEncoding,
    /// PEM text or base64 DER.
    pub data: String,
}

/// An own certificate and its key, ready to sign.
#[derive(Debug, Clone)]
pub struct OwnSigner {
    /// The key in the keyring.
    pub key_id: KeyId,
    /// The certificate.
    pub cert: Cert,
    /// Intermediates to include.
    pub chain: Vec<Cert>,
    /// The private key.
    pub key: Arc<dyn PrivateKeyOps>,
}

impl OwnSigner {
    /// The signer for [`crate::mime::sign_entity`].
    pub fn signer(&self) -> Signer<'_> {
        Signer {
            key: self.key.as_ref(),
            cert: &self.cert,
            chain: &self.chain,
        }
    }
}

/// Own certificates with their keys (`None` while locked).
#[derive(Debug, Default)]
pub struct OwnKeys {
    entries: Vec<(Cert, Option<Arc<dyn PrivateKeyOps>>)>,
}

impl OwnKeys {
    /// The keys for [`crate::mime::read`] / [`crate::enveloped::decrypt`].
    pub fn decryption_keys(&self) -> Vec<DecryptionKey<'_>> {
        self.entries
            .iter()
            .map(|(cert, key)| DecryptionKey {
                cert,
                key: key.as_deref(),
            })
            .collect()
    }
}

/// S/MIME operations on the store's keyring.
pub struct Keyring<'a> {
    store: &'a Store,
    secrets: &'a dyn SecretStore,
    anchors: &'a dyn TrustAnchors,
    unlocked: &'a UnlockCache,
    pbkdf2_iterations: u32,
    now: Option<i64>,
}

impl<'a> Keyring<'a> {
    /// A keyring over the store, the OS keychain, the trust anchors and
    /// the process's unlock cache.
    pub fn new(
        store: &'a Store,
        secrets: &'a dyn SecretStore,
        anchors: &'a dyn TrustAnchors,
        unlocked: &'a UnlockCache,
    ) -> Self {
        Self {
            store,
            secrets,
            anchors,
            unlocked,
            pbkdf2_iterations: PBKDF2_ITERATIONS,
            now: None,
        }
    }

    /// Uses fewer PBKDF2 iterations for new keys (tests only; the count is
    /// stored with each key, so reading is unaffected).
    pub fn with_pbkdf2_iterations(mut self, iterations: u32) -> Self {
        self.pbkdf2_iterations = iterations.max(1);
        self
    }

    /// Validates and expires against a fixed time (tests).
    pub fn at(mut self, now: i64) -> Self {
        self.now = Some(now);
        self
    }

    fn now(&self) -> i64 {
        self.now.unwrap_or_else(crate::time::now)
    }

    // -- import -----------------------------------------------------------

    /// Imports certificates (PEM, DER, PKCS#7 bundle). Existing ones keep
    /// their source and secret part.
    pub fn import_certificates(&self, data: &[u8], source: KeySource) -> Result<KeyImportResult> {
        let certs = Cert::parse_many(data)?;
        self.store_certificates(&certs, source)
    }

    fn store_certificates(&self, certs: &[Cert], source: KeySource) -> Result<KeyImportResult> {
        let mut result = KeyImportResult {
            keys: Vec::new(),
            created: 0,
            updated: 0,
            issues: Vec::new(),
        };
        for cert in certs {
            let existing = self
                .store
                .find_key(CryptoTechnology::Smime, cert.fingerprint())?;
            let source = existing.as_ref().map_or(source, |k| k.data.source);
            let (stored, created) = self.store.upsert_key(&key_data(cert, source, None))?;
            if created {
                result.created += 1;
            } else {
                result.updated += 1;
            }
            result.keys.push(self.to_crypto_key(&stored)?);
        }
        Ok(result)
    }

    /// Imports a PKCS#12 file: the own certificate with its private key
    /// (protected as asked) and the chain certificates.
    pub fn import_pkcs12(
        &self,
        data: &[u8],
        passphrase: &str,
        protection: &SecretProtection,
    ) -> Result<KeyImportResult> {
        let contents = crate::pkcs12::parse(data, passphrase)?;
        let secret = self.protect(&contents.pkcs8, contents.cert.fingerprint(), protection)?;
        let (stored, created) =
            self.store
                .upsert_key(&key_data(&contents.cert, KeySource::Pkcs12, Some(secret)))?;
        if let SecretProtection::Passphrase { .. } = protection {
            // Freshly imported: usable until Nail quits or it is locked.
            self.unlocked
                .put(contents.cert.fingerprint(), Arc::new(contents.key), None);
        }
        let mut result = self.store_certificates(&contents.chain, KeySource::Pkcs12)?;
        result.keys.insert(0, self.to_crypto_key(&stored)?);
        if created {
            result.created += 1;
        } else {
            result.updated += 1;
        }
        Ok(result)
    }

    /// Imports a certificate from the Windows store; with a private key
    /// ("Personal") it becomes an own key whose secret stays in the store.
    #[cfg(windows)]
    pub fn import_windows_certificate(
        &self,
        store: nail_api::security::WindowsCertStore,
        thumbprint: &str,
    ) -> Result<KeyImportResult> {
        let found = crate::windows::find_certificate(store, thumbprint)?;
        let secret = found.has_private_key.then(|| SecretKeyData {
            storage: SecretStorage::WindowsStore,
            data: None,
            reference: Some(found.cert.thumbprint()),
        });
        let (stored, created) =
            self.store
                .upsert_key(&key_data(&found.cert, KeySource::WindowsStore, secret))?;
        Ok(KeyImportResult {
            keys: vec![self.to_crypto_key(&stored)?],
            created: u32::from(created),
            updated: u32::from(!created),
            issues: Vec::new(),
        })
    }

    /// Not available off Windows.
    #[cfg(not(windows))]
    pub fn import_windows_certificate(
        &self,
        _store: nail_api::security::WindowsCertStore,
        _thumbprint: &str,
    ) -> Result<KeyImportResult> {
        Err(Error::Unsupported("the Windows certificate store".into()))
    }

    /// The certificates of a Windows store, with `imported` filled in.
    #[cfg(windows)]
    pub fn windows_certificates(
        &self,
        store: nail_api::security::WindowsCertStore,
    ) -> Result<Vec<nail_api::security::WindowsCertificate>> {
        let mut list = crate::windows::list_certificates(store)?;
        for item in &mut list {
            item.imported = self
                .store
                .find_key(CryptoTechnology::Smime, &item.certificate.fingerprint)?
                .map(|k| k.id);
        }
        Ok(list)
    }

    /// Not available off Windows.
    #[cfg(not(windows))]
    pub fn windows_certificates(
        &self,
        _store: nail_api::security::WindowsCertStore,
    ) -> Result<Vec<nail_api::security::WindowsCertificate>> {
        Err(Error::Unsupported("the Windows certificate store".into()))
    }

    // -- views ------------------------------------------------------------

    /// Every S/MIME certificate in the keyring (chain building pool).
    pub fn known_certificates(&self) -> Result<Vec<Cert>> {
        self.store
            .list_keys(Some(CryptoTechnology::Smime), false)?
            .iter()
            .map(|k| Cert::from_der(&k.data.public_data))
            .collect()
    }

    /// The protocol view of a stored certificate, its chain validated now.
    pub fn to_crypto_key(&self, stored: &StoredKey) -> Result<CryptoKey> {
        let pool = self.known_certificates()?;
        self.crypto_key_with(stored, &pool)
    }

    fn crypto_key_with(&self, stored: &StoredKey, pool: &[Cert]) -> Result<CryptoKey> {
        let cert = Cert::from_der(&stored.data.public_data)?;
        let now = self.now();
        let validation = chain::validate(&cert, pool, self.anchors, Purpose::Any, now);
        Ok(CryptoKey {
            id: stored.id.clone(),
            technology: CryptoTechnology::Smime,
            fingerprint: stored.data.fingerprint.clone(),
            user_ids: stored.data.user_ids.clone(),
            emails: stored.data.emails.clone(),
            algorithm: stored.data.algorithm.clone(),
            created_at: rfc3339(stored.data.created_at),
            expires_at: stored.data.expires_at.map(rfc3339),
            expired: stored.data.expires_at.is_some_and(|t| t < now),
            revoked: stored.data.revoked,
            can_sign: stored.data.can_sign,
            can_encrypt: stored.data.can_encrypt,
            has_secret: stored.is_own(),
            secret_storage: stored.data.secret.as_ref().map(|s| s.storage),
            source: stored.data.source,
            trust: trust_level(stored),
            issuer: stored.data.issuer.clone(),
            serial: stored.data.serial.clone(),
            chain: Some(validation.chain),
        })
    }

    /// One key.
    pub fn get(&self, id: &KeyId) -> Result<CryptoKey> {
        let stored = self.smime_key(id)?;
        self.to_crypto_key(&stored)
    }

    fn smime_key(&self, id: &KeyId) -> Result<StoredKey> {
        let stored = self.store.get_key(id)?;
        if stored.data.technology != CryptoTechnology::Smime {
            return Err(Error::Store(nail_store::Error::NotFound {
                kind: "S/MIME certificate",
                id: id.to_string(),
            }));
        }
        Ok(stored)
    }

    /// S/MIME keys matching the filter (own first). An OpenPGP-only filter
    /// yields nothing.
    pub fn list(&self, filter: &KeyFilter) -> Result<Vec<CryptoKey>> {
        if filter.technology == Some(CryptoTechnology::OpenPgp) {
            return Ok(Vec::new());
        }
        let text = filter.text.as_deref().map(str::to_lowercase);
        let pool = self.known_certificates()?;
        let mut out = Vec::new();
        for stored in self
            .store
            .list_keys(Some(CryptoTechnology::Smime), filter.own_only)?
        {
            let matches = text.as_deref().is_none_or(|t| {
                stored.data.fingerprint.to_lowercase().contains(t)
                    || stored.data.emails.iter().any(|e| e.contains(t))
                    || stored
                        .data
                        .user_ids
                        .iter()
                        .any(|u| u.to_lowercase().contains(t))
            });
            if matches {
                out.push(self.crypto_key_with(&stored, &pool)?);
            }
        }
        Ok(out)
    }

    /// Deletes a certificate. An own key needs `delete_secret` (else
    /// `conflict`); its keychain entry goes with it. A key held by the
    /// Windows store stays there. The trust decision is kept (it is keyed
    /// by fingerprint). Returns whether it existed.
    pub fn delete(&self, id: &KeyId, delete_secret: bool) -> Result<bool> {
        let Ok(stored) = self.smime_key(id) else {
            return Ok(false);
        };
        let deleted = self.store.delete_key(id, delete_secret)?;
        if let Some(secret) = stored.data.secret.as_ref().filter(|_| deleted) {
            self.unlocked.forget(&stored.data.fingerprint);
            if secret.storage == SecretStorage::Keychain
                && let Some(reference) = secret.reference.as_deref()
            {
                let reference =
                    SecretRef::parse(reference).map_err(|e| Error::Keychain(e.to_string()))?;
                self.secrets
                    .delete(&reference)
                    .map_err(|e| Error::Keychain(e.message))?;
            }
        }
        Ok(deleted)
    }

    /// Exports a certificate: PEM (`text`) or base64 DER.
    pub fn export_public(&self, id: &KeyId, encoding: KeyEncoding) -> Result<ExportedKey> {
        let stored = self.smime_key(id)?;
        let cert = Cert::from_der(&stored.data.public_data)?;
        let stem = cert
            .emails()
            .into_iter()
            .next()
            .or_else(|| cert.common_name())
            .unwrap_or_else(|| cert.fingerprint()[..16].to_owned());
        let stem: String = stem
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || matches!(c, '@' | '.' | '-' | '_') {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        Ok(match encoding {
            KeyEncoding::Text => ExportedKey {
                filename: format!("{stem}.pem"),
                mime: "application/x-pem-file".into(),
                encoding,
                data: cert.to_pem(),
            },
            KeyEncoding::Base64 => ExportedKey {
                filename: format!("{stem}.cer"),
                mime: "application/pkix-cert".into(),
                encoding,
                data: base64::engine::general_purpose::STANDARD.encode(cert.der()),
            },
        })
    }

    // -- private keys -----------------------------------------------------

    fn protect(
        &self,
        pkcs8: &[u8],
        fingerprint: &str,
        protection: &SecretProtection,
    ) -> Result<SecretKeyData> {
        match protection {
            SecretProtection::Keychain => {
                let mut wrapping = Zeroizing::new([0u8; 32]);
                OsRng.fill_bytes(wrapping.as_mut());
                let reference = keychain_ref(fingerprint)?;
                self.secrets
                    .set(
                        &reference,
                        &base64::engine::general_purpose::STANDARD.encode(wrapping.as_ref()),
                    )
                    .map_err(|e| Error::Keychain(e.message))?;
                let data = seal(MODE_KEYCHAIN, &[], wrapping.as_ref(), fingerprint, pkcs8)?;
                Ok(SecretKeyData {
                    storage: SecretStorage::Keychain,
                    data: Some(data),
                    reference: Some(reference.as_str().to_owned()),
                })
            }
            SecretProtection::Passphrase { passphrase } => {
                if passphrase.is_empty() {
                    return Err(Error::Crypto("an empty passphrase protects nothing".into()));
                }
                let mut salt = [0u8; 16];
                OsRng.fill_bytes(&mut salt);
                let iterations = self.pbkdf2_iterations;
                let key = derive(passphrase, &salt, iterations);
                let mut header = iterations.to_be_bytes().to_vec();
                header.extend_from_slice(&salt);
                let data = seal(MODE_PASSPHRASE, &header, key.as_ref(), fingerprint, pkcs8)?;
                Ok(SecretKeyData {
                    storage: SecretStorage::Passphrase,
                    data: Some(data),
                    reference: None,
                })
            }
        }
    }

    /// Unlocks a passphrase-protected key for `remember_minutes` (`None` =
    /// until Nail quits). Keys protected otherwise need no unlocking.
    pub fn unlock(
        &self,
        id: &KeyId,
        passphrase: &str,
        remember_minutes: Option<u32>,
    ) -> Result<()> {
        let stored = self.smime_key(id)?;
        let Some(secret) = &stored.data.secret else {
            return Err(Error::NoKey("not an own key".into()));
        };
        if secret.storage != SecretStorage::Passphrase {
            return Ok(());
        }
        let pkcs8 = open_passphrase(secret, &stored.data.fingerprint, passphrase)?;
        let key = SoftwareKey::from_pkcs8_der(&pkcs8)?;
        let until = remember_minutes.map(|m| self.now() + i64::from(m) * 60);
        self.unlocked
            .put(&stored.data.fingerprint, Arc::new(key), until);
        Ok(())
    }

    /// The private key of an own certificate: [`Error::Locked`] while a
    /// passphrase is needed.
    pub fn private_key(&self, stored: &StoredKey) -> Result<Arc<dyn PrivateKeyOps>> {
        let Some(secret) = &stored.data.secret else {
            return Err(Error::NoKey(format!(
                "no private key for {}",
                stored.data.fingerprint
            )));
        };
        let fingerprint = &stored.data.fingerprint;
        match secret.storage {
            SecretStorage::Passphrase => self
                .unlocked
                .get(fingerprint, self.now())
                .map(|k| k as Arc<dyn PrivateKeyOps>)
                .ok_or_else(|| Error::Locked(fingerprint.clone())),
            SecretStorage::Keychain => {
                if let Some(key) = self.unlocked.get(fingerprint, self.now()) {
                    return Ok(key);
                }
                let reference = secret
                    .reference
                    .as_deref()
                    .ok_or_else(|| Error::Keychain("missing keychain reference".into()))?;
                let reference =
                    SecretRef::parse(reference).map_err(|e| Error::Keychain(e.to_string()))?;
                let wrapping = self
                    .secrets
                    .get(&reference)
                    .map_err(|e| Error::Keychain(e.message))?
                    .ok_or_else(|| Error::Keychain("the key's keychain entry is gone".into()))?;
                let wrapping = Zeroizing::new(
                    base64::engine::general_purpose::STANDARD
                        .decode(wrapping.trim())
                        .map_err(|_| Error::Keychain("bad keychain entry".into()))?,
                );
                let data = secret
                    .data
                    .as_deref()
                    .ok_or_else(|| Error::malformed("keychain key without data"))?;
                let pkcs8 = open(MODE_KEYCHAIN, &wrapping, fingerprint, data, 0)?;
                let key = Arc::new(SoftwareKey::from_pkcs8_der(&pkcs8)?);
                self.unlocked.put(fingerprint, key.clone(), None);
                Ok(key)
            }
            SecretStorage::WindowsStore => windows_key(secret),
        }
    }

    /// Every own certificate with its key, `None` where locked or broken.
    pub fn own_keys(&self) -> Result<OwnKeys> {
        let mut entries = Vec::new();
        for stored in self.store.list_keys(Some(CryptoTechnology::Smime), true)? {
            let cert = Cert::from_der(&stored.data.public_data)?;
            let key = self.private_key(&stored).ok();
            entries.push((cert, key));
        }
        Ok(OwnKeys { entries })
    }

    // -- reading ----------------------------------------------------------

    /// Reads an S/MIME message with the keyring's certificates, own keys and
    /// trust decisions; `key_id` / `trust` of signatures and encryption are
    /// filled in. Nothing is written to the store.
    pub fn read_message(&self, entity: &[u8], from: Option<&str>) -> Result<Option<ReadOutcome>> {
        let known = self.known_certificates()?;
        let own = self.own_keys()?;
        let keys = own.decryption_keys();
        let ctx = ReadContext {
            anchors: self.anchors,
            known_certs: &known,
            keys: &keys,
            from,
            now: self.now(),
        };
        let Some(mut outcome) = mime::read(entity, &ctx)? else {
            return Ok(None);
        };
        for (signature, signer) in outcome
            .status
            .signatures
            .iter_mut()
            .zip(outcome.signers.iter_mut())
        {
            if let Some(fp) = signature.fingerprint.clone()
                && let Some(stored) = self.store.find_key(CryptoTechnology::Smime, &fp)?
            {
                signature.key_id = Some(stored.id.clone());
                signature.trust = trust_level(&stored);
                signer.info.key_id = signature.key_id.clone();
                signer.info.trust = signature.trust;
            }
        }
        let decrypting = outcome.decrypted_with.clone().or(outcome.locked.clone());
        if let (Some(fp), Some(encryption)) = (decrypting, outcome.status.encryption.as_mut()) {
            encryption.key_id = self
                .store
                .find_key(CryptoTechnology::Smime, &fp)?
                .map(|k| k.id);
        }
        Ok(Some(outcome))
    }

    /// Stores the certificates of valid signatures (so replies can be
    /// encrypted) and the intermediates the message carried. Returns the
    /// keys of newly stored signer certificates.
    pub fn remember_certificates(&self, outcome: &ReadOutcome) -> Result<Vec<KeyId>> {
        let mut new = Vec::new();
        for signer in &outcome.signers {
            let (Some(cert), nail_api::security::SignatureState::Valid) =
                (&signer.cert, signer.info.state)
            else {
                continue;
            };
            if !cert.is_mail_certificate() {
                continue;
            }
            if self
                .store
                .find_key(CryptoTechnology::Smime, cert.fingerprint())?
                .is_none()
            {
                let (stored, _) =
                    self.store
                        .upsert_key(&key_data(cert, KeySource::Message, None))?;
                new.push(stored.id);
            }
        }
        for cert in outcome.certificates.iter().filter(|c| c.is_ca()) {
            if self
                .store
                .find_key(CryptoTechnology::Smime, cert.fingerprint())?
                .is_none()
            {
                self.store
                    .upsert_key(&key_data(cert, KeySource::Message, None))?;
            }
        }
        Ok(new)
    }

    // -- sending ----------------------------------------------------------

    /// The own certificate to sign as `email` (the identity's configured
    /// certificate when `fingerprint` is given). [`Error::NoKey`] without
    /// one, [`Error::Locked`] while its passphrase is needed.
    pub fn signer(&self, email: &str, fingerprint: Option<&str>) -> Result<OwnSigner> {
        let now = self.now();
        let candidates: Vec<StoredKey> = self
            .store
            .list_keys(Some(CryptoTechnology::Smime), true)?
            .into_iter()
            .filter(|k| match fingerprint {
                Some(fp) => k.data.fingerprint.eq_ignore_ascii_case(fp),
                None => k.data.emails.iter().any(|e| e.eq_ignore_ascii_case(email)),
            })
            .filter(|k| k.data.can_sign && k.data.expires_at.is_none_or(|t| t >= now))
            .collect();
        let stored = candidates
            .into_iter()
            .max_by_key(|k| k.data.created_at)
            .ok_or_else(|| Error::NoKey(format!("no S/MIME signing certificate for {email}")))?;
        let key = self.private_key(&stored)?;
        let cert = Cert::from_der(&stored.data.public_data)?;
        let pool = self.known_certificates()?;
        let path = chain::validate(&cert, &pool, self.anchors, Purpose::Sign, now).path;
        let chain = path
            .into_iter()
            .skip(1)
            .filter(|c| !c.is_self_issued())
            .collect();
        Ok(OwnSigner {
            key_id: stored.id,
            cert,
            chain,
            key,
        })
    }

    /// The best certificate to encrypt to `email`, with its usability.
    pub fn recipient_key(&self, email: &str) -> Result<Option<RecipientKey>> {
        Ok(self
            .recipient_candidates(email)?
            .into_iter()
            .next()
            .map(|(stored, _, state)| RecipientKey {
                key_id: stored.id.clone(),
                fingerprint: stored.data.fingerprint.clone(),
                state,
                trust: trust_level(&stored),
                source: stored.data.source,
            }))
    }

    fn recipient_candidates(
        &self,
        email: &str,
    ) -> Result<Vec<(StoredKey, Cert, RecipientKeyState)>> {
        let now = self.now();
        let pool = self.known_certificates()?;
        let mut out = Vec::new();
        for stored in self
            .store
            .keys_for_email(email, Some(CryptoTechnology::Smime))?
        {
            let cert = Cert::from_der(&stored.data.public_data)?;
            let state = if stored.decision == Some(TrustDecision::Rejected) {
                RecipientKeyState::Rejected
            } else if !cert.can_encrypt() {
                RecipientKeyState::CannotEncrypt
            } else if !cert.valid_at(now) {
                RecipientKeyState::Expired
            } else {
                let validation = chain::validate(&cert, &pool, self.anchors, Purpose::Encrypt, now);
                let verified = stored.decision == Some(TrustDecision::Verified);
                match validation.state() {
                    ChainState::Valid => RecipientKeyState::Usable,
                    ChainState::UntrustedRoot | ChainState::Incomplete if verified => {
                        RecipientKeyState::Usable
                    }
                    ChainState::Expired => RecipientKeyState::Expired,
                    _ => RecipientKeyState::InvalidChain,
                }
            };
            out.push((stored, cert, state));
        }
        out.sort_by_key(|(stored, cert, state)| {
            (
                rank(*state),
                std::cmp::Reverse(cert.not_before()),
                stored.id.to_string(),
            )
        });
        Ok(out)
    }

    /// Certificates to encrypt to every address; [`Error::NoKey`] naming
    /// the addresses without a usable certificate (nothing is ever sent
    /// unencrypted instead).
    pub fn encryption_certificates(&self, emails: &[&str]) -> Result<Vec<Cert>> {
        let mut certs = Vec::new();
        let mut missing = Vec::new();
        for email in emails {
            match self
                .recipient_candidates(email)?
                .into_iter()
                .find(|(_, _, state)| *state == RecipientKeyState::Usable)
            {
                Some((_, cert, _)) => {
                    if !certs.contains(&cert) {
                        certs.push(cert);
                    }
                }
                None => missing.push(email.to_lowercase()),
            }
        }
        if !missing.is_empty() {
            return Err(Error::NoKey(format!(
                "no usable S/MIME certificate for {}",
                missing.join(", ")
            )));
        }
        Ok(certs)
    }
}

fn rank(state: RecipientKeyState) -> u8 {
    match state {
        RecipientKeyState::Usable => 0,
        RecipientKeyState::InvalidChain => 1,
        RecipientKeyState::Expired => 2,
        RecipientKeyState::CannotEncrypt => 3,
        RecipientKeyState::Revoked => 4,
        RecipientKeyState::Rejected => 5,
    }
}

/// Effective trust: a decision wins, own keys are `own`, else unknown (a
/// valid chain is reported in the chain, not here).
fn trust_level(stored: &StoredKey) -> TrustLevel {
    match stored.decision {
        Some(TrustDecision::Verified) => TrustLevel::Verified,
        Some(TrustDecision::Rejected) => TrustLevel::Rejected,
        _ if stored.is_own() => TrustLevel::Own,
        _ => TrustLevel::Unknown,
    }
}

fn key_data(cert: &Cert, source: KeySource, secret: Option<SecretKeyData>) -> KeyData {
    KeyData {
        technology: CryptoTechnology::Smime,
        fingerprint: cert.fingerprint().to_owned(),
        user_ids: vec![cert.subject()],
        emails: cert.emails(),
        algorithm: cert.algorithm_name(),
        created_at: cert.not_before(),
        expires_at: Some(cert.not_after()),
        revoked: false,
        can_sign: cert.can_sign(),
        can_encrypt: cert.can_encrypt(),
        source,
        public_data: cert.der().to_vec(),
        secret,
        issuer: Some(cert.issuer()),
        serial: Some(cert.serial()),
    }
}

fn keychain_ref(fingerprint: &str) -> Result<SecretRef> {
    SecretRef::parse(&format!(
        "{}{}/smime-key/{fingerprint}",
        SecretRef::SCHEME,
        SecretRef::SERVICE
    ))
    .map_err(|e| Error::Keychain(e.to_string()))
}

fn derive(passphrase: &str, salt: &[u8], iterations: u32) -> Zeroizing<[u8; 32]> {
    let mut key = Zeroizing::new([0u8; 32]);
    pbkdf2::pbkdf2_hmac::<sha2_legacy::Sha256>(
        passphrase.as_bytes(),
        salt,
        iterations,
        key.as_mut(),
    );
    key
}

fn aad(mode: u8, fingerprint: &str) -> Vec<u8> {
    let mut aad = MAGIC.to_vec();
    aad.push(mode);
    aad.extend_from_slice(fingerprint.as_bytes());
    aad
}

fn seal(
    mode: u8,
    header: &[u8],
    key: &[u8],
    fingerprint: &str,
    plaintext: &[u8],
) -> Result<Vec<u8>> {
    let cipher = aes_gcm::Aes256Gcm::new_from_slice(key)
        .map_err(|_| Error::Crypto("bad wrapping key".into()))?;
    let mut nonce = [0u8; 12];
    OsRng.fill_bytes(&mut nonce);
    let aad = aad(mode, fingerprint);
    let sealed = cipher
        .encrypt(
            aes_gcm::Nonce::from_slice(&nonce),
            Payload {
                msg: plaintext,
                aad: &aad,
            },
        )
        .map_err(|_| Error::Crypto("sealing the private key failed".into()))?;
    let mut out = MAGIC.to_vec();
    out.push(mode);
    out.extend_from_slice(header);
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&sealed);
    Ok(out)
}

/// Opens a sealed blob whose mode header is `header_len` bytes long.
fn open(
    mode: u8,
    key: &[u8],
    fingerprint: &str,
    data: &[u8],
    header_len: usize,
) -> Result<Zeroizing<Vec<u8>>> {
    let start = MAGIC.len() + 1 + header_len;
    if data.len() < start + 12 + 16 || &data[..4] != MAGIC || data[4] != mode {
        return Err(Error::malformed("not a Nail private-key blob"));
    }
    let cipher = aes_gcm::Aes256Gcm::new_from_slice(key)
        .map_err(|_| Error::Crypto("bad wrapping key".into()))?;
    let aad = aad(mode, fingerprint);
    cipher
        .decrypt(
            aes_gcm::Nonce::from_slice(&data[start..start + 12]),
            Payload {
                msg: &data[start + 12..],
                aad: &aad,
            },
        )
        .map(Zeroizing::new)
        .map_err(|_| Error::WrongPassphrase)
}

fn open_passphrase(
    secret: &SecretKeyData,
    fingerprint: &str,
    passphrase: &str,
) -> Result<Zeroizing<Vec<u8>>> {
    let data = secret
        .data
        .as_deref()
        .ok_or_else(|| Error::malformed("passphrase key without data"))?;
    if data.len() < 5 + 4 + 16 {
        return Err(Error::malformed("not a Nail private-key blob"));
    }
    let iterations = u32::from_be_bytes([data[5], data[6], data[7], data[8]]);
    if iterations == 0 || iterations > 100_000_000 {
        return Err(Error::malformed("implausible PBKDF2 iteration count"));
    }
    let key = derive(passphrase, &data[9..25], iterations);
    open(MODE_PASSPHRASE, key.as_ref(), fingerprint, data, 4 + 16)
}

#[cfg(windows)]
fn windows_key(secret: &SecretKeyData) -> Result<Arc<dyn PrivateKeyOps>> {
    let thumbprint = secret
        .reference
        .as_deref()
        .ok_or_else(|| Error::malformed("Windows key without a thumbprint"))?;
    Ok(Arc::new(crate::windows::WindowsKey::personal(thumbprint)?))
}

#[cfg(not(windows))]
fn windows_key(_secret: &SecretKeyData) -> Result<Arc<dyn PrivateKeyOps>> {
    Err(Error::Unsupported("the Windows certificate store".into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{AT, P12_PASSWORD, fixture, fixture_cert, test_anchors};
    use nail_api::engine::MemorySecretStore;
    use nail_api::security::{DecryptionState, SignatureState};

    struct Env {
        store: Store,
        secrets: MemorySecretStore,
        anchors: chain::FixedTrustAnchors,
        cache: UnlockCache,
    }

    impl Env {
        fn new() -> Self {
            Self {
                store: Store::open_in_memory().unwrap(),
                secrets: MemorySecretStore::new(),
                anchors: test_anchors(),
                cache: UnlockCache::new(),
            }
        }

        fn keyring(&self) -> Keyring<'_> {
            Keyring::new(&self.store, &self.secrets, &self.anchors, &self.cache)
                .with_pbkdf2_iterations(1_000)
                .at(AT)
        }
    }

    fn passphrase(p: &str) -> SecretProtection {
        SecretProtection::Passphrase {
            passphrase: p.into(),
        }
    }

    #[test]
    fn pkcs12_with_keychain_protection() {
        let env = Env::new();
        let keyring = env.keyring();
        let result = keyring
            .import_pkcs12(
                &fixture("alice.p12"),
                P12_PASSWORD,
                &SecretProtection::Keychain,
            )
            .unwrap();
        assert_eq!(result.created, 2); // alice + the intermediate
        let alice = &result.keys[0];
        assert!(alice.has_secret);
        assert_eq!(alice.secret_storage, Some(SecretStorage::Keychain));
        assert_eq!(alice.trust, TrustLevel::Own);
        assert_eq!(alice.source, KeySource::Pkcs12);
        assert_eq!(alice.emails, ["alice@example.org"]);
        assert_eq!(alice.user_ids, ["CN=Alice Example,O=Nail Test"]);
        assert_eq!(
            alice.issuer.as_deref(),
            Some("CN=Nail Test Email CA,O=Nail Test")
        );
        assert_eq!(alice.serial.as_deref(), Some("03E9"));
        assert_eq!(alice.chain.as_ref().unwrap().state, ChainState::Valid);
        assert!(alice.can_sign && alice.can_encrypt && !alice.expired);
        assert_eq!(env.secrets.len(), 1);

        // The stored secret is encrypted, never the plain PKCS#8.
        let stored = env.store.get_key(&alice.id).unwrap();
        let blob = stored.data.secret.as_ref().unwrap().data.clone().unwrap();
        assert!(blob.starts_with(MAGIC));
        let plain = crate::pkcs12::parse(&fixture("alice.p12"), P12_PASSWORD).unwrap();
        assert!(
            !blob
                .windows(32)
                .any(|w| plain.pkcs8.windows(32).any(|p| p == w))
        );

        // A fresh cache (a restart) can still sign without a prompt.
        let cache = UnlockCache::new();
        let keyring = Keyring::new(&env.store, &env.secrets, &env.anchors, &cache).at(AT);
        let signer = keyring.signer("ALICE@example.org", None).unwrap();
        assert_eq!(signer.chain, vec![fixture_cert("intermediate-ca.crt")]);
        let signed = mime::sign_entity(
            b"Content-Type: text/plain\r\n\r\nx\r\n",
            &signer.signer(),
            mime::SignMode::Detached,
            AT,
        )
        .unwrap();
        let outcome = keyring
            .read_message(&signed, Some("alice@example.org"))
            .unwrap()
            .unwrap();
        let sig = &outcome.status.signatures[0];
        assert_eq!(sig.state, SignatureState::Valid);
        assert_eq!(sig.key_id.as_ref(), Some(&alice.id));
        assert_eq!(sig.trust, TrustLevel::Own);
    }

    #[test]
    fn keychain_entry_loss_and_tampering() {
        let env = Env::new();
        let keyring = env.keyring();
        let alice = keyring
            .import_pkcs12(
                &fixture("alice.p12"),
                P12_PASSWORD,
                &SecretProtection::Keychain,
            )
            .unwrap()
            .keys
            .remove(0);
        let stored = env.store.get_key(&alice.id).unwrap();
        // Another certificate's fingerprint (the AAD) does not open it.
        let secret = stored.data.secret.clone().unwrap();
        let wrapping = env
            .secrets
            .get(&SecretRef::parse(secret.reference.as_deref().unwrap()).unwrap())
            .unwrap()
            .unwrap();
        let wrapping = base64::engine::general_purpose::STANDARD
            .decode(wrapping)
            .unwrap();
        let data = secret.data.clone().unwrap();
        assert!(open(MODE_KEYCHAIN, &wrapping, &stored.data.fingerprint, &data, 0).is_ok());
        assert!(open(MODE_KEYCHAIN, &wrapping, "00", &data, 0).is_err());
        // Losing the keychain entry means no key, not a crash.
        env.secrets
            .delete(&SecretRef::parse(secret.reference.as_deref().unwrap()).unwrap())
            .unwrap();
        let cache = UnlockCache::new();
        let keyring = Keyring::new(&env.store, &env.secrets, &env.anchors, &cache).at(AT);
        assert!(matches!(
            keyring.private_key(&stored),
            Err(Error::Keychain(_))
        ));
    }

    #[test]
    fn passphrase_protection_locks_and_unlocks() {
        let env = Env::new();
        let keyring = env.keyring();
        let alice = keyring
            .import_pkcs12(
                &fixture("alice.p12"),
                P12_PASSWORD,
                &passphrase("open sesame"),
            )
            .unwrap()
            .keys
            .remove(0);
        assert_eq!(alice.secret_storage, Some(SecretStorage::Passphrase));
        assert!(env.secrets.is_empty());
        // Usable right after import.
        keyring.signer("alice@example.org", None).unwrap();
        // After a restart it is locked.
        env.cache.lock_all();
        assert!(matches!(
            keyring.signer("alice@example.org", None),
            Err(Error::Locked(_))
        ));
        let message = fixture("alice-enc-gcm.eml");
        let outcome = keyring.read_message(&message, None).unwrap().unwrap();
        let enc = outcome.status.encryption.as_ref().unwrap();
        assert_eq!(enc.state, DecryptionState::Locked);
        assert_eq!(enc.key_id.as_ref(), Some(&alice.id));
        // Wrong passphrase.
        assert!(matches!(
            keyring.unlock(&alice.id, "nope", None),
            Err(Error::WrongPassphrase)
        ));
        // Right passphrase, for five minutes.
        keyring.unlock(&alice.id, "open sesame", Some(5)).unwrap();
        let outcome = keyring.read_message(&message, None).unwrap().unwrap();
        assert_eq!(
            outcome.status.encryption.as_ref().unwrap().state,
            DecryptionState::Decrypted
        );
        assert_eq!(outcome.content, fixture("content.eml"));
        // Six minutes later it is locked again.
        let later = Keyring::new(&env.store, &env.secrets, &env.anchors, &env.cache).at(AT + 360);
        assert!(matches!(
            later.signer("alice@example.org", None),
            Err(Error::Locked(_))
        ));
        assert!(matches!(
            keyring.import_pkcs12(&fixture("alice.p12"), P12_PASSWORD, &passphrase("")),
            Err(Error::Crypto(_))
        ));
    }

    #[test]
    fn wrong_pkcs12_password() {
        let env = Env::new();
        assert!(matches!(
            env.keyring().import_pkcs12(
                &fixture("alice.p12"),
                "wrong",
                &SecretProtection::Keychain
            ),
            Err(Error::WrongPassphrase)
        ));
        assert!(env.store.list_keys(None, false).unwrap().is_empty());
    }

    #[test]
    fn certificates_import_merge_list_and_export() {
        let env = Env::new();
        let keyring = env.keyring();
        let result = keyring
            .import_certificates(&fixture("alice-chain-p7c.der"), KeySource::Imported)
            .unwrap();
        assert_eq!((result.created, result.updated), (2, 0));
        let again = keyring
            .import_certificates(&fixture("alice.crt"), KeySource::Message)
            .unwrap();
        assert_eq!((again.created, again.updated), (0, 1));
        // The original source is kept.
        assert_eq!(again.keys[0].source, KeySource::Imported);
        // A PKCS#12 import of the same certificate adds the secret part.
        keyring
            .import_pkcs12(
                &fixture("alice.p12"),
                P12_PASSWORD,
                &SecretProtection::Keychain,
            )
            .unwrap();
        let own = keyring
            .list(&KeyFilter {
                own_only: true,
                ..KeyFilter::default()
            })
            .unwrap();
        assert_eq!(own.len(), 1);
        assert!(own[0].has_secret);

        let all = keyring.list(&KeyFilter::default()).unwrap();
        assert_eq!(all.len(), 2);
        assert!(all[0].has_secret, "own keys first");
        let found = keyring
            .list(&KeyFilter {
                text: Some("EMAIL CA".into()),
                ..KeyFilter::default()
            })
            .unwrap();
        assert_eq!(found.len(), 1);
        assert!(!found[0].can_sign);
        let none = keyring
            .list(&KeyFilter {
                technology: Some(CryptoTechnology::OpenPgp),
                ..KeyFilter::default()
            })
            .unwrap();
        assert!(none.is_empty());

        let pem = keyring
            .export_public(&own[0].id, KeyEncoding::Text)
            .unwrap();
        assert_eq!(pem.filename, "alice@example.org.pem");
        assert_eq!(
            Cert::parse_many(pem.data.as_bytes()).unwrap()[0],
            fixture_cert("alice.crt")
        );
        let der = keyring
            .export_public(&own[0].id, KeyEncoding::Base64)
            .unwrap();
        assert_eq!(der.filename, "alice@example.org.cer");
        assert_eq!(der.mime, "application/pkix-cert");
        assert_eq!(
            base64::engine::general_purpose::STANDARD
                .decode(der.data)
                .unwrap(),
            fixture_cert("alice.crt").der()
        );
        assert_eq!(keyring.get(&own[0].id).unwrap().id, own[0].id);
        assert!(keyring.get(&KeyId("key-999".into())).is_err());
        assert!(
            keyring
                .import_certificates(b"junk", KeySource::Imported)
                .is_err()
        );
    }

    #[test]
    fn recipient_states_and_encryption_certificates() {
        let env = Env::new();
        let keyring = env.keyring();
        for file in [
            "intermediate-ca.crt",
            "bob.crt",
            "carol.crt",
            "dave.crt",
            "mallory.crt",
        ] {
            keyring
                .import_certificates(&fixture(file), KeySource::Imported)
                .unwrap();
        }
        let state = |email: &str| keyring.recipient_key(email).unwrap().map(|k| k.state);
        assert_eq!(state("bob@example.net"), Some(RecipientKeyState::Usable));
        assert_eq!(state("carol@example.org"), Some(RecipientKeyState::Expired));
        assert_eq!(
            state("dave@example.org"),
            Some(RecipientKeyState::CannotEncrypt)
        );
        assert_eq!(
            state("mallory@example.org"),
            Some(RecipientKeyState::InvalidChain)
        );
        assert_eq!(state("nobody@example.org"), None);

        // A verified decision accepts the untrusted chain; a rejection wins.
        let mallory = fixture_cert("mallory.crt");
        env.store
            .set_trust_decision(
                CryptoTechnology::Smime,
                mallory.fingerprint(),
                TrustDecision::Verified,
            )
            .unwrap();
        assert_eq!(
            state("mallory@example.org"),
            Some(RecipientKeyState::Usable)
        );
        let bob = fixture_cert("bob.crt");
        env.store
            .set_trust_decision(
                CryptoTechnology::Smime,
                bob.fingerprint(),
                TrustDecision::Rejected,
            )
            .unwrap();
        let rejected = keyring.recipient_key("bob@example.net").unwrap().unwrap();
        assert_eq!(rejected.state, RecipientKeyState::Rejected);
        assert_eq!(rejected.trust, TrustLevel::Rejected);

        let certs = keyring
            .encryption_certificates(&["MALLORY@example.org"])
            .unwrap();
        assert_eq!(certs, vec![mallory]);
        let err = keyring
            .encryption_certificates(&["mallory@example.org", "bob@example.net", "x@example.org"])
            .unwrap_err();
        assert!(
            matches!(&err, Error::NoKey(m) if m.contains("bob@example.net") && m.contains("x@example.org"))
        );
        assert_eq!(err.code(), nail_api::ErrorCode::NoKey);
    }

    #[test]
    fn signers_need_an_own_usable_certificate() {
        let env = Env::new();
        let keyring = env.keyring();
        keyring
            .import_certificates(&fixture("alice.crt"), KeySource::Imported)
            .unwrap();
        // alice's certificate without a key cannot sign.
        assert!(matches!(
            keyring.signer("alice@example.org", None),
            Err(Error::NoKey(_))
        ));
        keyring
            .import_pkcs12(
                &fixture("bob.p12"),
                P12_PASSWORD,
                &SecretProtection::Keychain,
            )
            .unwrap();
        let bob = keyring.signer("bob@example.net", None).unwrap();
        assert_eq!(bob.cert, fixture_cert("bob.crt"));
        let by_fp = keyring
            .signer(
                "other@example.org",
                Some(&bob.cert.fingerprint().to_lowercase()),
            )
            .unwrap();
        assert_eq!(by_fp.key_id, bob.key_id);
        // Expired by the keyring's clock.
        let late =
            Keyring::new(&env.store, &env.secrets, &env.anchors, &env.cache).at(5_000_000_000);
        assert!(matches!(
            late.signer("bob@example.net", None),
            Err(Error::NoKey(_))
        ));
    }

    #[test]
    fn reading_fills_keys_and_remembers_signers() {
        let env = Env::new();
        let keyring = env.keyring();
        keyring
            .import_pkcs12(
                &fixture("bob.p12"),
                P12_PASSWORD,
                &SecretProtection::Keychain,
            )
            .unwrap();
        let bob_key = keyring.list(&KeyFilter::default()).unwrap()[0].id.clone();
        let outcome = keyring
            .read_message(
                &fixture("alice-signed-enc-bob.eml"),
                Some("alice@example.org"),
            )
            .unwrap()
            .unwrap();
        let enc = outcome.status.encryption.as_ref().unwrap();
        assert_eq!(enc.state, DecryptionState::Decrypted);
        assert_eq!(enc.key_id.as_ref(), Some(&bob_key));
        let sig = &outcome.status.signatures[0];
        assert_eq!(sig.state, SignatureState::Valid);
        assert!(sig.key_id.is_none(), "alice is not in the keyring yet");
        assert_eq!(sig.trust, TrustLevel::Unknown);

        let new = keyring.remember_certificates(&outcome).unwrap();
        assert_eq!(new.len(), 1);
        let alice = keyring.get(&new[0]).unwrap();
        assert_eq!(alice.source, KeySource::Message);
        assert_eq!(
            keyring
                .recipient_key("alice@example.org")
                .unwrap()
                .unwrap()
                .state,
            RecipientKeyState::Usable
        );
        // Remembering twice adds nothing; the next read knows the key.
        assert!(keyring.remember_certificates(&outcome).unwrap().is_empty());
        let again = keyring
            .read_message(
                &fixture("alice-signed-detached.eml"),
                Some("alice@example.org"),
            )
            .unwrap()
            .unwrap();
        assert_eq!(again.status.signatures[0].key_id.as_ref(), Some(&new[0]));

        // Invalid signatures are not remembered.
        let mut tampered = fixture("carol-signed-expired.eml");
        let pos = tampered.windows(9).position(|w| w == b"Hello Bob").unwrap();
        tampered[pos] = b'J';
        let bad = keyring.read_message(&tampered, None).unwrap().unwrap();
        assert!(keyring.remember_certificates(&bad).unwrap().is_empty());
        assert!(
            keyring
                .read_message(b"Content-Type: text/plain\r\n\r\nx", None)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn deleting_an_own_key_removes_its_keychain_entry() {
        let env = Env::new();
        let keyring = env.keyring();
        let alice = keyring
            .import_pkcs12(
                &fixture("alice.p12"),
                P12_PASSWORD,
                &SecretProtection::Keychain,
            )
            .unwrap()
            .keys
            .remove(0);
        assert_eq!(env.secrets.len(), 1);
        assert_eq!(
            keyring.delete(&alice.id, false).unwrap_err().code(),
            nail_api::ErrorCode::Conflict
        );
        assert!(keyring.delete(&alice.id, true).unwrap());
        assert!(env.secrets.is_empty());
        assert!(!keyring.delete(&alice.id, true).unwrap());
        // Public certificates need no flag.
        let inter = keyring.list(&KeyFilter::default()).unwrap()[0].id.clone();
        assert!(keyring.delete(&inter, false).unwrap());
        assert!(keyring.list(&KeyFilter::default()).unwrap().is_empty());
    }

    #[test]
    fn unlocking_keys_that_need_no_passphrase() {
        let env = Env::new();
        let keyring = env.keyring();
        let alice = keyring
            .import_pkcs12(
                &fixture("alice.p12"),
                P12_PASSWORD,
                &SecretProtection::Keychain,
            )
            .unwrap()
            .keys
            .remove(0);
        keyring.unlock(&alice.id, "anything", None).unwrap();
        let intermediate = keyring.list(&KeyFilter::default()).unwrap()[1].id.clone();
        assert!(matches!(
            keyring.unlock(&intermediate, "x", None),
            Err(Error::NoKey(_))
        ));
        assert!(alice.chain.is_some());
    }
}
