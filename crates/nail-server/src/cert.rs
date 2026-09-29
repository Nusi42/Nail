//! The remote UI's certificate: a self-signed one made by Nail (the LAN
//! default, pinned by fingerprint at pairing) or one from ACME
//! ([`crate::acme`]).
//!
//! Persistence (ADR 0036): the record — certificate chain, SHA-256
//! fingerprint, names, validity and the `keychain:` reference of the
//! private key, never the key itself — is stored in the core's settings
//! under [`nail_store::REMOTE_UI_CERTIFICATE_KEY`]; the key (PKCS#8 PEM) is
//! in the OS keychain ([`CERTIFICATE_KEY_REF`]). A self-signed certificate
//! is reused across restarts (so pinned fingerprints stay valid) until its
//! names change, it nears expiry or the key does not match; then a new one
//! is made.

use std::collections::BTreeSet;
use std::net::IpAddr;
use std::sync::Arc;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use chrono::{DateTime, Datelike, Days, NaiveTime, Utc};
use nail_api::SecretRef;
use nail_api::engine::SecretStore;
use nail_api::remote::{CertificateSource, RemoteCertificateInfo, RemoteUiSettings};
use nail_core::Service;
use rustls::ServerConfig;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::tls::{self, TlsConfigError};

/// Keychain entry of the certificate's private key.
pub const CERTIFICATE_KEY_REF: &str = "keychain:nail/remote-ui/certificate-key";

/// Validity of a self-signed certificate (within the 398 days browsers
/// accept for TLS server certificates).
pub const SELF_SIGNED_DAYS: u64 = 397;

/// A stored self-signed certificate is replaced this many days before it
/// expires.
pub const RENEW_BEFORE_DAYS: u64 = 30;

/// Errors making, loading or storing a certificate.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum CertError {
    /// Certificate generation failed.
    #[error("certificate generation: {0}")]
    Generate(#[from] rcgen::Error),
    /// No usable subject name.
    #[error("a certificate needs at least one host name or address")]
    NoNames,
    /// The TLS configuration could not be built from it.
    #[error(transparent)]
    Tls(#[from] TlsConfigError),
    /// Malformed PEM / DER.
    #[error("malformed certificate or key")]
    Malformed,
    /// The store failed.
    #[error("store: {0}")]
    Store(String),
    /// The keychain failed.
    #[error("keychain: {0}")]
    Keychain(String),
}

/// A certificate chain with its key, ready to serve.
#[derive(Clone)]
pub struct ServerIdentity {
    chain: Vec<CertificateDer<'static>>,
    key_pem: String,
    info: RemoteCertificateInfo,
}

impl std::fmt::Debug for ServerIdentity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ServerIdentity")
            .field("info", &self.info)
            .finish_non_exhaustive()
    }
}

impl ServerIdentity {
    /// A chain (leaf first) and its PKCS#8 PEM key, checked to match.
    pub fn new(
        source: CertificateSource,
        chain: Vec<CertificateDer<'static>>,
        key_pem: String,
        subject_names: Vec<String>,
        not_before: String,
        not_after: String,
    ) -> Result<Self, CertError> {
        let leaf = chain.first().ok_or(CertError::Malformed)?;
        let info = RemoteCertificateInfo {
            source,
            fingerprint_sha256: fingerprint(leaf),
            subject_names,
            not_before,
            not_after,
        };
        let identity = Self {
            chain,
            key_pem,
            info,
        };
        identity.check_key()?;
        Ok(identity)
    }

    fn key_der(&self) -> Result<PrivateKeyDer<'static>, CertError> {
        let pair = rcgen::KeyPair::from_pem(&self.key_pem).map_err(|_| CertError::Malformed)?;
        Ok(PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(
            pair.serialize_der(),
        )))
    }

    fn check_key(&self) -> Result<(), CertError> {
        let provider = tls::provider();
        let key = provider
            .key_provider
            .load_private_key(self.key_der()?)
            .map_err(|_| CertError::Malformed)?;
        rustls::sign::CertifiedKey::new(self.chain.clone(), key)
            .keys_match()
            .map_err(|_| CertError::Malformed)
    }

    /// What the status and the pairing screen show.
    pub fn info(&self) -> &RemoteCertificateInfo {
        &self.info
    }

    /// SHA-256 of the leaf certificate (uppercase hex), for pinning.
    pub fn fingerprint(&self) -> &str {
        &self.info.fingerprint_sha256
    }

    /// The certificate chain (leaf first).
    pub fn chain(&self) -> &[CertificateDer<'static>] {
        &self.chain
    }

    /// The rustls server configuration ([`tls::server_config`]).
    pub fn tls_config(&self) -> Result<Arc<ServerConfig>, CertError> {
        Ok(tls::server_config(self.chain.clone(), self.key_der()?)?)
    }
}

/// SHA-256 of a DER certificate, uppercase hex without separators (the
/// form of [`RemoteCertificateInfo::fingerprint_sha256`]).
pub fn fingerprint(der: &CertificateDer<'_>) -> String {
    hex::encode_upper(Sha256::digest(der.as_ref()))
}

/// The names a self-signed certificate should carry for `settings`: the
/// host name browsers use (or the computer's name), `localhost`, the
/// loopback addresses and the bind address when it is a specific one.
pub fn subject_names(settings: &RemoteUiSettings, computer_name: Option<&str>) -> Vec<String> {
    let mut names = Vec::new();
    let mut push = |name: String| {
        if !name.is_empty() && !names.contains(&name) {
            names.push(name);
        }
    };
    if let Some(host) = settings.host_name.as_deref() {
        push(host.trim_end_matches('.').to_ascii_lowercase());
    } else if let Some(computer) = computer_name {
        push(computer.to_ascii_lowercase());
    }
    push("localhost".to_owned());
    push("127.0.0.1".to_owned());
    push("::1".to_owned());
    if let Some(ip) = settings.bind_ip()
        && !ip.is_unspecified()
    {
        push(ip.to_string());
    }
    names
}

fn day_start(day: DateTime<Utc>) -> DateTime<Utc> {
    day.date_naive().and_time(NaiveTime::MIN).and_utc()
}

/// Makes a self-signed certificate (ECDSA P-256) for `names`, valid from
/// the start of `now`'s day for [`SELF_SIGNED_DAYS`].
pub fn generate_self_signed(
    names: &[String],
    now: DateTime<Utc>,
) -> Result<ServerIdentity, CertError> {
    if names.is_empty() {
        return Err(CertError::NoNames);
    }
    let not_before = day_start(now);
    let not_after = not_before
        .checked_add_days(Days::new(SELF_SIGNED_DAYS))
        .ok_or(CertError::Malformed)?;
    let mut params = rcgen::CertificateParams::new(names.to_vec())?;
    let ymd = |d: DateTime<Utc>| {
        rcgen::date_time_ymd(
            d.year(),
            u8::try_from(d.month()).unwrap_or(1),
            u8::try_from(d.day()).unwrap_or(1),
        )
    };
    params.not_before = ymd(not_before);
    params.not_after = ymd(not_after);
    let mut dn = rcgen::DistinguishedName::new();
    dn.push(rcgen::DnType::CommonName, names[0].clone());
    params.distinguished_name = dn;
    params.is_ca = rcgen::IsCa::ExplicitNoCa;
    params.key_usages = vec![rcgen::KeyUsagePurpose::DigitalSignature];
    params.extended_key_usages = vec![rcgen::ExtendedKeyUsagePurpose::ServerAuth];
    let key = rcgen::KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256)?;
    let cert = params.self_signed(&key)?;
    ServerIdentity::new(
        CertificateSource::SelfSigned,
        vec![cert.der().clone()],
        key.serialize_pem(),
        names.to_vec(),
        not_before.to_rfc3339(),
        not_after.to_rfc3339(),
    )
}

/// The stored record (settings key [`nail_store::REMOTE_UI_CERTIFICATE_KEY`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CertificateRecord {
    /// Self-signed or ACME.
    pub source: CertificateSource,
    /// DER certificates, leaf first, standard base64.
    pub chain: Vec<String>,
    /// SHA-256 of the leaf (uppercase hex).
    pub fingerprint_sha256: String,
    /// Names in the certificate.
    pub subject_names: Vec<String>,
    /// Valid from (RFC 3339).
    pub not_before: String,
    /// Valid until (RFC 3339).
    pub not_after: String,
    /// Keychain reference of the private key.
    pub key_ref: String,
}

impl CertificateRecord {
    fn of(identity: &ServerIdentity) -> Self {
        Self {
            source: identity.info.source,
            chain: identity.chain.iter().map(|c| STANDARD.encode(c)).collect(),
            fingerprint_sha256: identity.info.fingerprint_sha256.clone(),
            subject_names: identity.info.subject_names.clone(),
            not_before: identity.info.not_before.clone(),
            not_after: identity.info.not_after.clone(),
            key_ref: CERTIFICATE_KEY_REF.to_owned(),
        }
    }

    fn restore(&self, secrets: &dyn SecretStore) -> Result<ServerIdentity, CertError> {
        let chain = self
            .chain
            .iter()
            .map(|c| STANDARD.decode(c).map(CertificateDer::from))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| CertError::Malformed)?;
        let key_ref = SecretRef::parse(&self.key_ref).map_err(|_| CertError::Malformed)?;
        let key_pem = secrets
            .get(&key_ref)
            .map_err(|e| CertError::Keychain(e.message))?
            .ok_or(CertError::Malformed)?;
        let identity = ServerIdentity::new(
            self.source,
            chain,
            key_pem,
            self.subject_names.clone(),
            self.not_before.clone(),
            self.not_after.clone(),
        )?;
        if identity.info.fingerprint_sha256 != self.fingerprint_sha256 {
            return Err(CertError::Malformed);
        }
        Ok(identity)
    }
}

/// Where certificates are kept: the core's settings and keychain.
pub trait CertificateStore {
    /// The stored record, if any.
    fn load_record(&self) -> Result<Option<CertificateRecord>, CertError>;
    /// Replaces the stored record.
    fn save_record(&self, record: &CertificateRecord) -> Result<(), CertError>;
    /// The keychain.
    fn secrets(&self) -> &dyn SecretStore;
}

impl CertificateStore for Service {
    fn load_record(&self) -> Result<Option<CertificateRecord>, CertError> {
        self.store()
            .setting::<CertificateRecord>(nail_store::REMOTE_UI_CERTIFICATE_KEY)
            .map_err(|e| CertError::Store(e.to_string()))
    }

    fn save_record(&self, record: &CertificateRecord) -> Result<(), CertError> {
        self.store()
            .set_setting(nail_store::REMOTE_UI_CERTIFICATE_KEY, record)
            .map_err(|e| CertError::Store(e.to_string()))
    }

    fn secrets(&self) -> &dyn SecretStore {
        Service::secrets(self).as_ref()
    }
}

/// Stores `identity` (key first, then the record that references it).
pub fn save(store: &dyn CertificateStore, identity: &ServerIdentity) -> Result<(), CertError> {
    let key_ref = SecretRef(CERTIFICATE_KEY_REF.to_owned());
    store
        .secrets()
        .set(&key_ref, &identity.key_pem)
        .map_err(|e| CertError::Keychain(e.message))?;
    store.save_record(&CertificateRecord::of(identity))
}

/// Whether a stored self-signed certificate can be served for `names` at
/// `now`.
fn reusable(identity: &ServerIdentity, names: &[String], now: DateTime<Utc>) -> bool {
    let wanted: BTreeSet<String> = names.iter().map(|n| normalize_name(n)).collect();
    let have: BTreeSet<String> = identity
        .info
        .subject_names
        .iter()
        .map(|n| normalize_name(n))
        .collect();
    let dates = DateTime::parse_from_rfc3339(&identity.info.not_before)
        .ok()
        .zip(DateTime::parse_from_rfc3339(&identity.info.not_after).ok());
    let Some((not_before, not_after)) = dates else {
        return false;
    };
    let renew_at = not_after
        .with_timezone(&Utc)
        .checked_sub_days(Days::new(RENEW_BEFORE_DAYS));
    identity.info.source == CertificateSource::SelfSigned
        && wanted == have
        && not_before.with_timezone(&Utc) <= now
        && renew_at.is_some_and(|renew| now < renew)
}

fn normalize_name(name: &str) -> String {
    match name.parse::<IpAddr>() {
        Ok(ip) => ip.to_string(),
        Err(_) => name.trim_end_matches('.').to_ascii_lowercase(),
    }
}

/// The self-signed certificate for `names`: the stored one while it is
/// still good (same names, not near expiry, key present and matching),
/// else a new one, which is stored. Blocking (store and keychain).
pub fn load_or_create_self_signed(
    store: &dyn CertificateStore,
    names: &[String],
    now: DateTime<Utc>,
) -> Result<ServerIdentity, CertError> {
    if let Some(record) = store.load_record()? {
        match record.restore(store.secrets()) {
            Ok(identity) if reusable(&identity, names, now) => return Ok(identity),
            Ok(_) => tracing::info!("replacing the remote UI certificate (names or validity)"),
            Err(err) => tracing::warn!(%err, "the stored remote UI certificate is unusable"),
        }
    }
    regenerate_self_signed(store, names, now)
}

/// A new self-signed certificate for `names`, stored (replacing the old
/// one). Paired browsers see a new fingerprint.
pub fn regenerate_self_signed(
    store: &dyn CertificateStore,
    names: &[String],
    now: DateTime<Utc>,
) -> Result<ServerIdentity, CertError> {
    let identity = generate_self_signed(names, now)?;
    save(store, &identity)?;
    Ok(identity)
}

/// The stored certificate of any source (e.g. an ACME one), if it restores.
pub fn load(store: &dyn CertificateStore) -> Result<Option<ServerIdentity>, CertError> {
    match store.load_record()? {
        Some(record) => record.restore(store.secrets()).map(Some),
        None => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nail_api::engine::MemorySecretStore;
    use std::sync::Mutex;

    #[derive(Default)]
    struct MemoryStore {
        record: Mutex<Option<CertificateRecord>>,
        secrets: MemorySecretStore,
    }

    impl CertificateStore for MemoryStore {
        fn load_record(&self) -> Result<Option<CertificateRecord>, CertError> {
            Ok(self.record.lock().unwrap().clone())
        }
        fn save_record(&self, record: &CertificateRecord) -> Result<(), CertError> {
            *self.record.lock().unwrap() = Some(record.clone());
            Ok(())
        }
        fn secrets(&self) -> &dyn SecretStore {
            &self.secrets
        }
    }

    fn now() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-09-29T12:34:56Z")
            .unwrap()
            .with_timezone(&Utc)
    }

    fn names() -> Vec<String> {
        vec![
            "nail.lan.example".into(),
            "localhost".into(),
            "127.0.0.1".into(),
        ]
    }

    #[test]
    fn a_self_signed_certificate_has_the_names_and_a_fingerprint() {
        let id = generate_self_signed(&names(), now()).unwrap();
        let info = id.info();
        assert_eq!(info.source, CertificateSource::SelfSigned);
        assert_eq!(info.fingerprint_sha256.len(), 64);
        assert!(
            info.fingerprint_sha256
                .chars()
                .all(|c| c.is_ascii_digit() || ('A'..='F').contains(&c))
        );
        assert_eq!(info.fingerprint_sha256, fingerprint(&id.chain()[0]));
        assert_eq!(info.subject_names, names());
        assert_eq!(info.not_before, "2026-09-29T00:00:00+00:00");
        assert_eq!(info.not_after, "2027-10-31T00:00:00+00:00");
        assert!(id.tls_config().is_ok());
        assert!(format!("{id:?}").contains("fingerprint"));
        assert!(!format!("{id:?}").contains("PRIVATE KEY"));
        assert!(matches!(
            generate_self_signed(&[], now()),
            Err(CertError::NoNames)
        ));
    }

    #[test]
    fn the_stored_certificate_is_reused_until_it_must_change() {
        let store = MemoryStore::default();
        let first = load_or_create_self_signed(&store, &names(), now()).unwrap();
        // Only a reference to the key is in the record.
        let record = store.load_record().unwrap().unwrap();
        assert_eq!(record.key_ref, CERTIFICATE_KEY_REF);
        assert!(!serde_json::to_string(&record).unwrap().contains("PRIVATE"));
        assert_eq!(store.secrets.len(), 1);

        // Restart: same certificate (pinned fingerprints stay valid).
        let again = load_or_create_self_signed(&store, &names(), now()).unwrap();
        assert_eq!(again.fingerprint(), first.fingerprint());

        // Near expiry: replaced.
        let late = now()
            .checked_add_days(Days::new(SELF_SIGNED_DAYS - 10))
            .unwrap();
        let renewed = load_or_create_self_signed(&store, &names(), late).unwrap();
        assert_ne!(renewed.fingerprint(), first.fingerprint());

        // Other names: replaced.
        let other = vec!["other.lan.example".to_owned()];
        let renamed = load_or_create_self_signed(&store, &other, late).unwrap();
        assert_ne!(renamed.fingerprint(), renewed.fingerprint());
        assert_eq!(renamed.info().subject_names, other);

        // Explicit regeneration.
        let fresh = regenerate_self_signed(&store, &other, late).unwrap();
        assert_ne!(fresh.fingerprint(), renamed.fingerprint());
        assert_eq!(
            load(&store).unwrap().unwrap().fingerprint(),
            fresh.fingerprint()
        );
    }

    #[test]
    fn a_damaged_record_or_missing_key_is_replaced() {
        let store = MemoryStore::default();
        let first = load_or_create_self_signed(&store, &names(), now()).unwrap();
        // The key vanished from the keychain.
        store
            .secrets
            .delete(&SecretRef(CERTIFICATE_KEY_REF.to_owned()))
            .unwrap();
        let second = load_or_create_self_signed(&store, &names(), now()).unwrap();
        assert_ne!(second.fingerprint(), first.fingerprint());

        // A key that belongs to another certificate.
        let other = generate_self_signed(&names(), now()).unwrap();
        store
            .secrets
            .set(&SecretRef(CERTIFICATE_KEY_REF.to_owned()), &other.key_pem)
            .unwrap();
        assert!(matches!(load(&store), Err(CertError::Malformed)));
        let third = load_or_create_self_signed(&store, &names(), now()).unwrap();
        assert_ne!(third.fingerprint(), second.fingerprint());

        // A tampered fingerprint.
        let mut record = store.load_record().unwrap().unwrap();
        record.fingerprint_sha256 = "00".repeat(32);
        store.save_record(&record).unwrap();
        assert!(matches!(load(&store), Err(CertError::Malformed)));
    }

    #[test]
    fn subject_names_follow_the_settings() {
        let mut settings = RemoteUiSettings {
            bind_address: "192.168.1.5".into(),
            ..RemoteUiSettings::default()
        };
        assert_eq!(
            subject_names(&settings, Some("DESKTOP-1")),
            ["desktop-1", "localhost", "127.0.0.1", "::1", "192.168.1.5"]
        );
        settings.host_name = Some("Nail.Home.Example.".into());
        settings.bind_address = "0.0.0.0".into();
        assert_eq!(
            subject_names(&settings, Some("DESKTOP-1")),
            ["nail.home.example", "localhost", "127.0.0.1", "::1"]
        );
    }
}
