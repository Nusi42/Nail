//! ACME certificates (Let's Encrypt) for internet mode.
//!
//! **Locked.** Internet mode stays locked until the penetration-test gate
//! is recorded ([`nail_api::remote::internet_mode_lock`]); every entry
//! point here checks the lock first and answers
//! [`AcmeError::InternetModeLocked`] without touching the network. The
//! issuing code (HTTP-01 through [`AcmeChallenges`], served at
//! [`CHALLENGE_PATH`] by the app router) is wired so that lifting the lock
//! is the only change needed — plus port 80 reaching Nail, which ACME's
//! HTTP-01 validation requires.

use std::collections::HashMap;
use std::sync::{Arc, PoisonError, RwLock};

use chrono::Utc;
use instant_acme::{
    Account, AuthorizationStatus, ChallengeType, Identifier, NewAccount, NewOrder, OrderStatus,
    RetryPolicy,
};
use nail_api::remote::{
    CertificateSource, InternetModeLockReason, RemoteTlsMode, RemoteUiSettings, internet_mode_lock,
};
use rustls::pki_types::CertificateDer;
use rustls::pki_types::pem::PemObject;

use crate::cert::{CertError, ServerIdentity};

/// Route prefix of HTTP-01 challenge answers.
pub const CHALLENGE_PATH: &str = "/.well-known/acme-challenge/";

/// Why no ACME certificate was issued.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum AcmeError {
    /// Internet mode (and with it ACME) is locked in this build.
    #[error("internet mode is locked ({0:?})")]
    InternetModeLocked(InternetModeLockReason),
    /// The settings do not ask for ACME, or lack domains / accepted terms,
    /// or name a non-https directory.
    #[error("the ACME settings are incomplete")]
    Settings,
    /// The CA refused or the protocol failed.
    #[error("ACME: {0}")]
    Protocol(String),
    /// The issued certificate is unusable.
    #[error(transparent)]
    Certificate(#[from] CertError),
}

/// Pending HTTP-01 challenges: token → key authorization. Shared between
/// the issuing task and the app router's challenge route.
#[derive(Debug, Clone, Default)]
pub struct AcmeChallenges(Arc<RwLock<HashMap<String, String>>>);

impl AcmeChallenges {
    /// No pending challenges.
    pub fn new() -> Self {
        Self::default()
    }

    /// The key authorization for `token`, if pending.
    pub fn get(&self, token: &str) -> Option<String> {
        self.0
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .get(token)
            .cloned()
    }

    fn insert(&self, token: String, key_authorization: String) {
        self.0
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(token, key_authorization);
    }

    fn clear(&self) {
        self.0
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .clear();
    }

    /// **Tests only** (feature-free, crate-private use): registers a token.
    #[cfg(test)]
    pub(crate) fn insert_for_tests(&self, token: &str, key_authorization: &str) {
        self.insert(token.to_owned(), key_authorization.to_owned());
    }
}

/// The checks before any ACME traffic, against a given lock state.
fn preflight(
    settings: &RemoteUiSettings,
    lock: Option<InternetModeLockReason>,
) -> Result<(), AcmeError> {
    if let Some(reason) = lock {
        return Err(AcmeError::InternetModeLocked(reason));
    }
    let acme = &settings.acme;
    let directory_ok = url::Url::parse(&acme.directory_url).is_ok_and(|u| u.scheme() == "https");
    if !settings.internet_mode
        || settings.tls != RemoteTlsMode::Acme
        || acme.domains.is_empty()
        || !acme.accept_terms
        || !directory_ok
    {
        return Err(AcmeError::Settings);
    }
    Ok(())
}

/// Whether ACME may be used by this build with these settings (the lock
/// first). Never touches the network.
pub fn check_available(settings: &RemoteUiSettings) -> Result<(), AcmeError> {
    preflight(settings, internet_mode_lock())
}

/// Issues a certificate for `settings.acme.domains` with HTTP-01.
/// Refuses with [`AcmeError::InternetModeLocked`] while internet mode is
/// locked — always in this build.
pub async fn issue(
    settings: &RemoteUiSettings,
    challenges: &AcmeChallenges,
) -> Result<ServerIdentity, AcmeError> {
    check_available(settings)?;
    let result = issue_unchecked(settings, challenges).await;
    challenges.clear();
    result
}

fn protocol(err: impl std::fmt::Display) -> AcmeError {
    AcmeError::Protocol(err.to_string())
}

async fn issue_unchecked(
    settings: &RemoteUiSettings,
    challenges: &AcmeChallenges,
) -> Result<ServerIdentity, AcmeError> {
    let acme = &settings.acme;
    let contact: Vec<String> = acme
        .contact_email
        .iter()
        .map(|mail| format!("mailto:{mail}"))
        .collect();
    let contact: Vec<&str> = contact.iter().map(String::as_str).collect();
    let (account, _credentials) = Account::builder()
        .map_err(protocol)?
        .create(
            &NewAccount {
                contact: &contact,
                terms_of_service_agreed: acme.accept_terms,
                only_return_existing: false,
            },
            acme.directory_url.clone(),
            None,
        )
        .await
        .map_err(protocol)?;
    let identifiers: Vec<Identifier> = acme
        .domains
        .iter()
        .map(|d| Identifier::Dns(d.clone()))
        .collect();
    let mut order = account
        .new_order(&NewOrder::new(&identifiers))
        .await
        .map_err(protocol)?;

    let mut authorizations = order.authorizations();
    while let Some(authz) = authorizations.next().await {
        let mut authz = authz.map_err(protocol)?;
        match authz.status {
            AuthorizationStatus::Pending => {}
            AuthorizationStatus::Valid => continue,
            other => return Err(AcmeError::Protocol(format!("authorization {other:?}"))),
        }
        let mut challenge = authz
            .challenge(ChallengeType::Http01)
            .ok_or_else(|| AcmeError::Protocol("no http-01 challenge".into()))?;
        challenges.insert(
            challenge.token.clone(),
            challenge.key_authorization().as_str().to_owned(),
        );
        challenge.set_ready().await.map_err(protocol)?;
    }
    let status = order
        .poll_ready(&RetryPolicy::new())
        .await
        .map_err(protocol)?;
    if status != OrderStatus::Ready {
        return Err(AcmeError::Protocol(format!("order {status:?}")));
    }

    let mut params =
        rcgen::CertificateParams::new(acme.domains.clone()).map_err(CertError::from)?;
    params.distinguished_name = rcgen::DistinguishedName::new();
    let key =
        rcgen::KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256).map_err(CertError::from)?;
    let csr = params.serialize_request(&key).map_err(CertError::from)?;
    order.finalize_csr(csr.der()).await.map_err(protocol)?;
    let chain_pem = order
        .poll_certificate(&RetryPolicy::new())
        .await
        .map_err(protocol)?;
    let chain = CertificateDer::pem_slice_iter(chain_pem.as_bytes())
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| CertError::Malformed)?;
    let now = Utc::now();
    let not_after = now.checked_add_days(chrono::Days::new(90)).unwrap_or(now);
    Ok(ServerIdentity::new(
        CertificateSource::Acme,
        chain,
        key.serialize_pem(),
        acme.domains.clone(),
        now.to_rfc3339(),
        not_after.to_rfc3339(),
    )?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nail_api::remote::AcmeSettings;

    fn acme_settings() -> RemoteUiSettings {
        RemoteUiSettings {
            enabled: true,
            bind_address: "0.0.0.0".into(),
            tls: RemoteTlsMode::Acme,
            internet_mode: true,
            acme: AcmeSettings {
                domains: vec!["mail.example.org".into()],
                contact_email: Some("admin@example.org".into()),
                accept_terms: true,
                ..AcmeSettings::default()
            },
            ..RemoteUiSettings::default()
        }
    }

    #[test]
    fn acme_is_locked_in_this_build() {
        assert!(internet_mode_lock().is_some());
        assert!(matches!(
            check_available(&acme_settings()),
            Err(AcmeError::InternetModeLocked(
                InternetModeLockReason::PentestPending
            ))
        ));
    }

    #[tokio::test]
    async fn issuing_refuses_without_network_access() {
        let challenges = AcmeChallenges::new();
        // Even complete settings with internet mode on are refused before
        // any request (there is no network in tests: a request would fail
        // differently).
        let err = issue(&acme_settings(), &challenges).await.unwrap_err();
        assert!(matches!(err, AcmeError::InternetModeLocked(_)), "{err:?}");
    }

    #[test]
    fn preflight_checks_the_settings_once_unlocked() {
        // With a hypothetical unlocked build, the settings decide.
        assert!(preflight(&acme_settings(), None).is_ok());
        let mut s = acme_settings();
        s.internet_mode = false;
        assert!(matches!(preflight(&s, None), Err(AcmeError::Settings)));
        let mut s = acme_settings();
        s.tls = RemoteTlsMode::SelfSigned;
        assert!(matches!(preflight(&s, None), Err(AcmeError::Settings)));
        let mut s = acme_settings();
        s.acme.domains.clear();
        assert!(matches!(preflight(&s, None), Err(AcmeError::Settings)));
        let mut s = acme_settings();
        s.acme.accept_terms = false;
        assert!(matches!(preflight(&s, None), Err(AcmeError::Settings)));
        let mut s = acme_settings();
        s.acme.directory_url = "http://acme.example.org/dir".into();
        assert!(matches!(preflight(&s, None), Err(AcmeError::Settings)));
        // The lock wins over everything.
        assert!(matches!(
            preflight(
                &acme_settings(),
                Some(InternetModeLockReason::PentestPending)
            ),
            Err(AcmeError::InternetModeLocked(_))
        ));
    }

    #[test]
    fn challenges_are_looked_up_by_token() {
        let c = AcmeChallenges::new();
        assert_eq!(c.get("t1"), None);
        c.insert_for_tests("t1", "t1.thumb");
        assert_eq!(c.get("t1").as_deref(), Some("t1.thumb"));
        c.clear();
        assert_eq!(c.get("t1"), None);
    }
}
