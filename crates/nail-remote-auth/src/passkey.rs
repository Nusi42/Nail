//! Passkeys (WebAuthn relying party) on `webauthn_rp` (ADR 0036: the
//! plan's `webauthn-rs` needs OpenSSL). This module is the only place that
//! touches the library, so it can be swapped.
//!
//! * The relying-party id is the configured **host name**; IP addresses
//!   are refused ([`RelyingParty::new`]), because browsers refuse them.
//! * Registration creates discoverable credentials with user verification
//!   required, excluding the user's existing credentials. `credProtect`
//!   (user verification required) is requested but not enforced by the
//!   browser: many platform authenticators do not implement the extension,
//!   and an enforced one makes the browser refuse to create the passkey.
//!   Every login requires user verification anyway (ADR 0041).
//! * Login uses discoverable requests (no user name needed) with user
//!   verification required; the signature counter must grow (a cloned
//!   authenticator is refused).
//! * Ceremony state stays in memory (never serialized), keyed by a random
//!   challenge id, single use, with a short expiry — replaying an answer
//!   finds no ceremony.
//! * The stored credential ([`StoredCredential`]) holds only public data:
//!   user handle, transports, public key and extensions, and the dynamic
//!   state (signature counter, backup flags).

use std::net::IpAddr;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::{Deserialize, Serialize};
use webauthn_rp::bin::{Decode, Encode};
use webauthn_rp::request::auth::AuthenticationVerificationOptions;
use webauthn_rp::request::register::CredProtect;
use webauthn_rp::request::register::{
    Nickname, PublicKeyCredentialUserEntity, RegistrationVerificationOptions, UserHandle64,
    Username,
};
use webauthn_rp::request::{
    AsciiDomain, DomainOrigin, ExtensionInfo, PublicKeyCredentialDescriptor, RpId,
};
use webauthn_rp::response::register::{CompressedPubKey, DynamicState, StaticState};
use webauthn_rp::response::{AuthTransports, CredentialId};
use webauthn_rp::{
    AuthenticatedCredential, DiscoverableAuthentication64, DiscoverableAuthenticationServerState,
    DiscoverableCredentialRequestOptions, PublicKeyCredentialCreationOptions, Registration,
    RegistrationServerState,
};

use crate::error::{AuthError, Result};

/// Length of our user handles.
const USER_HANDLE_LEN: usize = 64;

/// The public key type of decoded credentials.
type StoredKey = CompressedPubKey<[u8; 32], [u8; 32], [u8; 48], Vec<u8>>;

/// The relying party: its id (a host name) and the origins pages may be
/// served from.
#[derive(Debug, Clone)]
pub struct RelyingParty {
    rp_id: RpId,
    origins: Vec<String>,
}

/// Whether `host` is usable as a WebAuthn relying-party id: a DNS name,
/// never an IP address.
pub fn check_rp_host(host: &str) -> Result<String> {
    let host = host.trim().trim_end_matches('.').to_ascii_lowercase();
    let bare = host.trim_start_matches('[').trim_end_matches(']');
    let refuse = |why: &str| {
        Err(AuthError::Invalid(format!(
            "relying-party id {host:?}: {why}"
        )))
    };
    if bare.parse::<IpAddr>().is_ok() {
        return refuse("an IP address cannot be a relying-party id");
    }
    let labels: Vec<&str> = host.split('.').collect();
    let label_ok = |l: &&str| {
        !l.is_empty()
            && l.len() <= 63
            && !l.starts_with('-')
            && !l.ends_with('-')
            && l.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
    };
    if host.is_empty() || host.len() > 253 || !labels.iter().all(label_ok) {
        return refuse("not a DNS name");
    }
    if labels
        .last()
        .is_some_and(|l| l.bytes().all(|b| b.is_ascii_digit()))
    {
        return refuse("the last label is numeric");
    }
    Ok(host)
}

impl RelyingParty {
    /// A relying party for `host` (a DNS name; IP addresses are refused)
    /// whose pages are served from `origins` (e.g.
    /// `https://nail.local:4780`). With no origins, `https://<host>` is the
    /// only one.
    pub fn new(host: &str, origins: Vec<String>) -> Result<Self> {
        let host = check_rp_host(host)?;
        let domain = AsciiDomain::try_from(host.clone())
            .map_err(|e| AuthError::Invalid(format!("relying-party id {host:?}: {e}")))?;
        let origins = if origins.is_empty() {
            vec![format!("https://{host}")]
        } else {
            origins
        };
        for origin in &origins {
            if DomainOrigin::try_from(origin.as_str()).is_err() {
                return Err(AuthError::Invalid(format!("not an origin: {origin:?}")));
            }
        }
        Ok(Self {
            rp_id: RpId::Domain(domain),
            origins,
        })
    }

    /// A relying party for `host` served on `https://host:port`.
    pub fn for_port(host: &str, port: u16) -> Result<Self> {
        let host = check_rp_host(host)?;
        let origin = if port == 443 {
            format!("https://{host}")
        } else {
            format!("https://{host}:{port}")
        };
        Self::new(&host, vec![origin])
    }

    /// The relying-party id.
    pub fn id(&self) -> &str {
        self.rp_id.as_ref()
    }

    /// The accepted origins.
    pub fn origins(&self) -> &[String] {
        &self.origins
    }

    fn domain_origins(&self) -> Vec<DomainOrigin<'_, '_>> {
        self.origins
            .iter()
            .filter_map(|o| DomainOrigin::try_from(o.as_str()).ok())
            .collect()
    }
}

/// Why a passkey step failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PasskeyError {
    /// The browser's answer is not a WebAuthn credential.
    #[error("malformed credential: {0}")]
    Malformed(String),
    /// No stored credential has this id.
    #[error("unknown credential")]
    UnknownCredential,
    /// The stored credential cannot be decoded.
    #[error("stored credential unreadable: {0}")]
    Stored(String),
    /// The ceremony failed (signature, origin, challenge, user
    /// verification, signature counter, …).
    #[error("ceremony failed: {0}")]
    Ceremony(String),
}

/// A credential as stored in `remote_passkeys.credential` (JSON; public
/// data only).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredCredential {
    /// Format version.
    pub v: u32,
    /// User handle (base64url).
    pub user_handle: String,
    /// Transports (bit set).
    pub transports: u8,
    /// Public key and extensions (base64url of the library's encoding).
    pub static_state: String,
    /// Signature counter and flags (base64url of the library's encoding).
    pub dynamic_state: String,
}

impl StoredCredential {
    /// Parses the stored JSON.
    pub fn parse(text: &str) -> Result<Self, PasskeyError> {
        serde_json::from_str(text).map_err(|e| PasskeyError::Stored(e.to_string()))
    }

    /// The JSON to store.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    /// The user handle.
    pub fn user_handle(&self) -> Result<UserHandle64, PasskeyError> {
        let bytes = URL_SAFE_NO_PAD
            .decode(&self.user_handle)
            .map_err(|e| PasskeyError::Stored(e.to_string()))?;
        let array: [u8; USER_HANDLE_LEN] = bytes
            .try_into()
            .map_err(|_| PasskeyError::Stored("user handle length".into()))?;
        UserHandle64::decode(array).map_err(|_| PasskeyError::Stored("user handle".into()))
    }

    fn transports(&self) -> Option<AuthTransports> {
        transports(self.transports)
    }

    fn static_state(&self) -> Result<StaticState<StoredKey>, PasskeyError> {
        let bytes = URL_SAFE_NO_PAD
            .decode(&self.static_state)
            .map_err(|e| PasskeyError::Stored(e.to_string()))?;
        StaticState::<StoredKey>::decode(bytes.as_slice())
            .map_err(|e| PasskeyError::Stored(e.to_string()))
    }

    fn dynamic_state(&self) -> Result<DynamicState, PasskeyError> {
        let bytes = URL_SAFE_NO_PAD
            .decode(&self.dynamic_state)
            .map_err(|e| PasskeyError::Stored(e.to_string()))?;
        let array: [u8; 7] = bytes
            .try_into()
            .map_err(|_| PasskeyError::Stored("dynamic state length".into()))?;
        DynamicState::decode(array).map_err(|e| PasskeyError::Stored(e.to_string()))
    }

    fn with_dynamic_state(&self, state: DynamicState) -> Self {
        let encoded = state.encode().map(|b| URL_SAFE_NO_PAD.encode(b));
        Self {
            dynamic_state: encoded.unwrap_or_else(|_| self.dynamic_state.clone()),
            ..self.clone()
        }
    }

    /// The signature counter (tests, diagnostics).
    pub fn sign_count(&self) -> Option<u32> {
        self.dynamic_state().ok().map(|d| d.sign_count)
    }
}

/// The transport set of stored bits (unknown bits: none).
fn transports(bits: u8) -> Option<AuthTransports> {
    AuthTransports::decode(bits)
        .or_else(|_| AuthTransports::decode(0))
        .ok()
}

/// A new user handle (random, 64 bytes), base64url.
pub fn new_user_handle() -> String {
    let handle = UserHandle64::new();
    URL_SAFE_NO_PAD.encode(handle.as_ref())
}

/// A registered credential of the user (for `excludeCredentials`).
#[derive(Debug, Clone, Copy)]
pub struct StoredCredentialRef<'a> {
    /// The credential id.
    pub credential_id: &'a [u8],
    /// Its stored data.
    pub stored: &'a StoredCredential,
}

/// Pending registration ceremony.
#[derive(Debug)]
pub struct RegistrationCeremony(RegistrationServerState<USER_HANDLE_LEN>);

/// Pending login ceremony.
#[derive(Debug)]
pub struct LoginCeremony(DiscoverableAuthenticationServerState);

/// Starts registering a passkey for a user identified by `user_handle`
/// (base64url, from [`new_user_handle`] or the user's other passkeys),
/// shown to authenticators as `name` / `display_name`, excluding the
/// `existing` credentials of the user. Returns the ceremony and the
/// `PublicKeyCredentialCreationOptionsJSON` for the browser.
pub fn start_registration(
    rp: &RelyingParty,
    user_handle: &str,
    name: &str,
    display_name: Option<&str>,
    existing: &[StoredCredentialRef<'_>],
) -> Result<(RegistrationCeremony, serde_json::Value)> {
    let stored = StoredCredential {
        v: 1,
        user_handle: user_handle.to_owned(),
        transports: 0,
        static_state: String::new(),
        dynamic_state: String::new(),
    };
    let handle = stored
        .user_handle()
        .map_err(|e| AuthError::Internal(e.to_string()))?;
    // Names that do not fit the WebAuthn username profile fall back to
    // the handle-free placeholder the library uses.
    let fallback = PublicKeyCredentialUserEntity::from(&handle);
    let user = PublicKeyCredentialUserEntity {
        name: Username::try_from(name).unwrap_or(fallback.name),
        id: &handle,
        display_name: display_name.and_then(|d| Nickname::try_from(d).ok()),
    };
    let exclude = existing
        .iter()
        .filter_map(|existing| {
            Some(PublicKeyCredentialDescriptor {
                id: CredentialId::<Vec<u8>>::decode(existing.credential_id.to_vec()).ok()?,
                transports: existing.stored.transports()?,
            })
        })
        .collect();
    let mut creation = PublicKeyCredentialCreationOptions::passkey(&rp.rp_id, user, exclude);
    creation.extensions.cred_protect =
        CredProtect::UserVerificationRequired(ExtensionInfo::AllowDontEnforceValue);
    let (server, client) = creation
        .start_ceremony()
        .map_err(|e| AuthError::Internal(format!("webauthn: {e}")))?;
    let options =
        serde_json::to_value(&client).map_err(|e| AuthError::Internal(format!("webauthn: {e}")))?;
    Ok((RegistrationCeremony(server), options))
}

/// A verified new credential.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewCredential {
    /// The credential id.
    pub credential_id: Vec<u8>,
    /// What to store.
    pub stored: StoredCredential,
}

/// Finishes a registration with the browser's `PublicKeyCredential.toJSON()`.
pub fn finish_registration(
    rp: &RelyingParty,
    ceremony: RegistrationCeremony,
    credential: &serde_json::Value,
) -> Result<NewCredential, PasskeyError> {
    let bytes =
        serde_json::to_vec(credential).map_err(|e| PasskeyError::Malformed(e.to_string()))?;
    let registration = Registration::from_json_relaxed(&bytes)
        .map_err(|e| PasskeyError::Malformed(e.to_string()))?;
    let origins = rp.domain_origins();
    let options = RegistrationVerificationOptions::<DomainOrigin<'_, '_>, DomainOrigin<'_, '_>> {
        allowed_origins: &origins,
        ..RegistrationVerificationOptions::default()
    };
    let registered = ceremony
        .0
        .verify(&rp.rp_id, &registration, &options)
        .map_err(|e| PasskeyError::Ceremony(e.to_string()))?;
    let (id, transports, user_id, static_state, dynamic_state, _metadata) = registered.into_parts();
    let encode_err = |e: std::convert::Infallible| PasskeyError::Stored(e.to_string());
    let stored = StoredCredential {
        v: 1,
        user_handle: URL_SAFE_NO_PAD.encode(user_id.encode().map_err(encode_err)?),
        transports: transports.encode().map_err(encode_err)?,
        static_state: URL_SAFE_NO_PAD.encode(static_state.encode().map_err(encode_err)?),
        dynamic_state: URL_SAFE_NO_PAD.encode(dynamic_state.encode().map_err(encode_err)?),
    };
    Ok(NewCredential {
        credential_id: id.as_ref().to_vec(),
        stored,
    })
}

/// Starts a passkey login (discoverable credential, user verification
/// required). Returns the ceremony and the
/// `PublicKeyCredentialRequestOptionsJSON` for the browser.
pub fn start_login(rp: &RelyingParty) -> Result<(LoginCeremony, serde_json::Value)> {
    let (server, client) = DiscoverableCredentialRequestOptions::passkey(&rp.rp_id)
        .start_ceremony()
        .map_err(|e| AuthError::Internal(format!("webauthn: {e}")))?;
    let options =
        serde_json::to_value(&client).map_err(|e| AuthError::Internal(format!("webauthn: {e}")))?;
    Ok((LoginCeremony(server), options))
}

/// The credential id of a login answer (to look the credential up).
pub fn login_credential_id(credential: &serde_json::Value) -> Result<Vec<u8>, PasskeyError> {
    let bytes =
        serde_json::to_vec(credential).map_err(|e| PasskeyError::Malformed(e.to_string()))?;
    let authentication = DiscoverableAuthentication64::from_json_relaxed(&bytes)
        .map_err(|e| PasskeyError::Malformed(e.to_string()))?;
    Ok(authentication.raw_id().as_ref().to_vec())
}

/// Finishes a login: verifies the browser's answer against the stored
/// credential and returns the credential with its updated dynamic state
/// (store it: it carries the new signature counter).
pub fn finish_login(
    rp: &RelyingParty,
    ceremony: LoginCeremony,
    credential: &serde_json::Value,
    credential_id: &[u8],
    stored: &StoredCredential,
) -> Result<StoredCredential, PasskeyError> {
    let bytes =
        serde_json::to_vec(credential).map_err(|e| PasskeyError::Malformed(e.to_string()))?;
    let authentication = DiscoverableAuthentication64::from_json_relaxed(&bytes)
        .map_err(|e| PasskeyError::Malformed(e.to_string()))?;
    let handle = stored.user_handle()?;
    let id = CredentialId::<&[u8]>::decode(credential_id)
        .map_err(|e| PasskeyError::Stored(e.to_string()))?;
    let mut authenticated =
        AuthenticatedCredential::new(id, &handle, stored.static_state()?, stored.dynamic_state()?)
            .map_err(|e| PasskeyError::Stored(e.to_string()))?;
    let origins = rp.domain_origins();
    let options = AuthenticationVerificationOptions::<DomainOrigin<'_, '_>, DomainOrigin<'_, '_>> {
        allowed_origins: &origins,
        ..AuthenticationVerificationOptions::default()
    };
    ceremony
        .0
        .verify(&rp.rp_id, &authentication, &mut authenticated, &options)
        .map_err(|e| PasskeyError::Ceremony(e.to_string()))?;
    Ok(stored.with_dynamic_state(authenticated.dynamic_state()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relying_party_ids_must_be_host_names() {
        for bad in [
            "192.168.1.2",
            "[::1]",
            "::1",
            "fe80::1",
            "127.0.0.1",
            "host.123",
            "",
            "bad_name.local",
            "-x.local",
        ] {
            assert!(
                matches!(check_rp_host(bad), Err(AuthError::Invalid(_))),
                "{bad:?} accepted"
            );
            assert!(RelyingParty::for_port(bad, 4780).is_err(), "{bad:?}");
        }
        assert_eq!(check_rp_host("Nail.Local.").unwrap(), "nail.local");
        let rp = RelyingParty::for_port("nail.local", 4780).unwrap();
        assert_eq!(rp.id(), "nail.local");
        assert_eq!(rp.origins(), ["https://nail.local:4780"]);
        let rp = RelyingParty::for_port("desktop-7", 443).unwrap();
        assert_eq!(rp.origins(), ["https://desktop-7"]);
        assert!(RelyingParty::new("nail.local", vec!["https://nail.local:x".into()]).is_err());
    }

    #[test]
    fn options_carry_challenge_rp_and_user_verification() {
        let rp = RelyingParty::for_port("nail.local", 4780).unwrap();
        let (_ceremony, options) = start_login(&rp).unwrap();
        assert_eq!(options["rpId"], "nail.local");
        assert_eq!(options["userVerification"], "required");
        assert!(options["challenge"].as_str().is_some_and(|c| !c.is_empty()));

        let handle = new_user_handle();
        let other = StoredCredential {
            v: 1,
            user_handle: handle.clone(),
            transports: 0,
            static_state: String::new(),
            dynamic_state: String::new(),
        };
        let existing = [StoredCredentialRef {
            credential_id: &[7; 16],
            stored: &other,
        }];
        let (_ceremony, options) =
            start_registration(&rp, &handle, "ada", Some("Ada"), &existing).unwrap();
        assert_eq!(options["rp"]["id"], "nail.local");
        assert_eq!(options["user"]["name"], "ada");
        assert_eq!(options["user"]["id"], handle.as_str());
        assert_eq!(
            options["authenticatorSelection"]["userVerification"],
            "required"
        );
        assert_eq!(options["authenticatorSelection"]["residentKey"], "required");
        assert_eq!(options["excludeCredentials"].as_array().unwrap().len(), 1);
        // credProtect is asked for, not enforced by the browser (authenticators
        // without the extension could not create a passkey otherwise).
        assert_eq!(
            options["extensions"]["credentialProtectionPolicy"],
            "userVerificationRequired"
        );
        assert_eq!(
            options["extensions"]["enforceCredentialProtectionPolicy"],
            false
        );
    }

    #[test]
    fn stored_credentials_reject_garbage() {
        assert!(StoredCredential::parse("{}").is_err());
        let stored = StoredCredential {
            v: 1,
            user_handle: "short".into(),
            transports: 0,
            static_state: "!!".into(),
            dynamic_state: "AAAA".into(),
        };
        assert!(stored.user_handle().is_err());
        assert!(stored.static_state().is_err());
        assert!(stored.dynamic_state().is_err());
        assert_eq!(stored.sign_count(), None);
        assert_eq!(StoredCredential::parse(&stored.to_json()).unwrap(), stored);
    }
}
