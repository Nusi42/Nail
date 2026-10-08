//! Test harness: an in-memory store, memory keychain, manual clock,
//! recorded events, a cookie-keeping [`Browser`] and a software passkey
//! [`Authenticator`] (Ed25519, "none" attestation).

use std::sync::Arc;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ed25519_dalek::{Signer as _, SigningKey};
use nail_api::engine::MemorySecretStore;
use nail_api::remote::{
    AuthPurpose, AuthRequest, AuthResponse, DEVICE_COOKIE, RemoteAccessPreset, RemoteGrant,
    RemoteSession, RemoteUser, SESSION_COOKIE,
};
use nail_api::{Event, McpScope};
use nail_remote_auth::password::{Argon2Cost, PasswordPolicy};
use nail_remote_auth::{
    AuthConfig, AuthDeps, AuthPolicy, AuthReply, ClientMeta, Clock, Cookies, ManualClock,
    RecordingSink, RelyingParty, RemoteAuth, RemoteSessionCtx,
};
use nail_store::Store;
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

/// Start of the test clock (2026-09-29).
pub(crate) use nail_api::testing::T0;
pub(crate) const HOST: &str = "nail.local";
pub(crate) const ORIGIN: &str = "https://nail.local:4780";
pub(crate) const PASSWORD: &str = "correct horse battery";

pub(crate) struct Harness {
    pub auth: RemoteAuth,
    pub store: Store,
    pub clock: ManualClock,
    pub events: Arc<RecordingSink>,
    pub secrets: Arc<MemorySecretStore>,
}

pub(crate) fn fast_policy() -> AuthPolicy {
    AuthPolicy {
        password: PasswordPolicy {
            cost: Argon2Cost {
                memory_kib: 1024,
                iterations: 1,
                parallelism: 1,
            },
            ..PasswordPolicy::default()
        },
        ..AuthPolicy::default()
    }
}

pub(crate) fn config() -> AuthConfig {
    AuthConfig {
        relying_party: Some(RelyingParty::for_port(HOST, 4780).unwrap()),
        origins: vec![ORIGIN.to_owned()],
        certificate_fingerprint: Some("AB".repeat(32)),
    }
}

impl Harness {
    pub(crate) fn new() -> Self {
        Self::with_policy(fast_policy())
    }

    pub(crate) fn with_policy(policy: AuthPolicy) -> Self {
        let store = Store::open_in_memory().unwrap();
        let clock = ManualClock::new(T0);
        let events = Arc::new(RecordingSink::new());
        let secrets = Arc::new(MemorySecretStore::new());
        let auth = RemoteAuth::new(
            AuthDeps {
                store: store.clone(),
                secrets: secrets.clone(),
                clock: Arc::new(clock.clone()),
                events: events.clone(),
            },
            config(),
            policy,
        );
        Self {
            auth,
            store,
            clock,
            events,
            secrets,
        }
    }

    pub(crate) fn now(&self) -> i64 {
        self.clock.now()
    }

    /// A user with the full preset (optionally admin) and a password.
    pub(crate) async fn user(&self, name: &str, admin: bool) -> RemoteUser {
        let mut grant = RemoteAccessPreset::Full.grant();
        grant.admin = admin;
        let user = self
            .store
            .create_remote_user(name, None, &grant, self.now())
            .unwrap();
        self.auth
            .set_password(&user.id, Some(PASSWORD), None)
            .await
            .unwrap()
    }

    /// A pairing code for `user`.
    pub(crate) fn code(&self, user: &RemoteUser, grant: Option<&RemoteGrant>) -> String {
        match self
            .auth
            .create_pairing_code(&user.id, None, grant, None, None)
            .unwrap()
        {
            nail_api::Response::PairingCodeCreated { code, .. } => code,
            other => panic!("{other:?}"),
        }
    }

    /// A browser paired to `user` (logged in by the pairing).
    pub(crate) async fn paired(&self, user: &RemoteUser, ip: &str) -> Browser {
        let mut browser = Browser::new(ip);
        let code = self.code(user, None);
        let reply = browser.pair(self, &code, "Phone").await;
        assert!(
            matches!(reply.response, AuthResponse::LoggedIn { .. }),
            "{reply:?}"
        );
        browser
    }

    /// A harness with the user `ada` and a browser paired to her.
    pub(crate) async fn with_ada() -> (Self, RemoteUser, Browser) {
        let h = Self::new();
        let ada = h.user("ada", false).await;
        let browser = h.paired(&ada, "192.0.2.10").await;
        (h, ada, browser)
    }

    /// Two browsers paired to `user` (a phone and a laptop).
    pub(crate) async fn two_browsers(&self, user: &RemoteUser) -> (Browser, Browser) {
        (
            self.paired(user, "192.0.2.10").await,
            self.paired(user, "192.0.2.11").await,
        )
    }

    /// [`Harness::two_browsers`] with each browser's resolved session.
    pub(crate) async fn two_sessions(
        &self,
        user: &RemoteUser,
    ) -> ((Browser, RemoteSessionCtx), (Browser, RemoteSessionCtx)) {
        let (mut phone, mut laptop) = self.two_browsers(user).await;
        let phone_ctx = phone.resolve(self).unwrap();
        let laptop_ctx = laptop.resolve(self).unwrap();
        ((phone, phone_ctx), (laptop, laptop_ctx))
    }

    /// The reply refused the credentials, and the audit says why.
    pub(crate) fn assert_refused(&self, reply: &AuthReply, detail: &str) {
        assert_eq!(
            failure(reply).0,
            nail_api::remote::AuthFailure::InvalidCredentials
        );
        let audit = self.store.list_remote_audit(None, None, 1).unwrap();
        assert_eq!(audit[0].detail.as_deref(), Some(detail));
    }

    pub(crate) fn events(&self) -> Vec<Event> {
        self.events.take()
    }

    pub(crate) fn audit_kinds(
        &self,
    ) -> Vec<(
        nail_api::remote::RemoteAuditKind,
        nail_api::remote::RemoteAuditOutcome,
    )> {
        self.store
            .list_remote_audit(None, None, 1000)
            .unwrap()
            .into_iter()
            .rev()
            .map(|e| (e.kind, e.outcome))
            .collect()
    }
}

impl Default for Harness {
    fn default() -> Self {
        Self::new()
    }
}

/// A browser: keeps its cookies and CSRF token like a real one would.
#[derive(Debug, Clone)]
pub(crate) struct Browser {
    pub session: Option<String>,
    pub device: Option<String>,
    pub csrf: Option<String>,
    pub client: ClientMeta,
}

impl Browser {
    pub(crate) fn new(ip: &str) -> Self {
        Self {
            session: None,
            device: None,
            csrf: None,
            client: ClientMeta {
                ip: Some(ip.parse().unwrap()),
                user_agent: Some("TestBrowser/1.0".into()),
            },
        }
    }

    pub(crate) fn cookies(&self) -> Cookies {
        Cookies {
            session: self.session.clone().map(Zeroizing::new),
            device: self.device.clone().map(Zeroizing::new),
        }
    }

    pub(crate) fn apply(&mut self, set_cookies: &[String]) {
        for header in set_cookies {
            let cookie = cookie::Cookie::parse(header.clone()).unwrap();
            let removed = cookie
                .max_age()
                .is_some_and(cookie::time::SignedDuration::is_zero);
            let value = (!removed).then(|| cookie.value().to_owned());
            match cookie.name() {
                SESSION_COOKIE => self.session = value,
                DEVICE_COOKIE => self.device = value,
                other => panic!("unexpected cookie {other}"),
            }
        }
    }

    /// Sends a login-protocol request (fetching a CSRF token first when
    /// needed) and keeps the cookies.
    pub(crate) async fn send(&mut self, h: &Harness, request: AuthRequest) -> AuthReply {
        if self.csrf.is_none() && !matches!(request, AuthRequest::Status) {
            self.status(h).await;
        }
        let reply = h
            .auth
            .handle_auth(request, &self.cookies(), &self.client, self.csrf.as_deref())
            .await;
        self.apply(&reply.cookies);
        if let Some(token) = &reply.csrf_token {
            self.csrf = Some(token.clone());
        }
        reply
    }

    pub(crate) async fn status(&mut self, h: &Harness) -> nail_api::remote::AuthStatus {
        let reply = h
            .auth
            .handle_auth(AuthRequest::Status, &self.cookies(), &self.client, None)
            .await;
        self.apply(&reply.cookies);
        match reply.response {
            AuthResponse::Status { status } => {
                self.csrf = Some(status.csrf_token.clone());
                status
            }
            other => panic!("{other:?}"),
        }
    }

    pub(crate) async fn pair(&mut self, h: &Harness, code: &str, name: &str) -> AuthReply {
        let reply = self
            .send(
                h,
                AuthRequest::Pair {
                    code: code.to_owned(),
                    device_name: name.to_owned(),
                },
            )
            .await;
        // The CSRF binding moves from anonymous to the device.
        self.status(h).await;
        reply
    }

    pub(crate) async fn password(
        &mut self,
        h: &Harness,
        user: &str,
        password: &str,
        totp: Option<&str>,
    ) -> AuthReply {
        self.send(
            h,
            AuthRequest::Password {
                user_name: user.to_owned(),
                password: password.to_owned(),
                totp_code: totp.map(str::to_owned),
                purpose: AuthPurpose::Login,
            },
        )
        .await
    }

    pub(crate) async fn reauth_password(
        &mut self,
        h: &Harness,
        user: &str,
        password: &str,
    ) -> AuthReply {
        self.send(
            h,
            AuthRequest::Password {
                user_name: user.to_owned(),
                password: password.to_owned(),
                totp_code: None,
                purpose: AuthPurpose::Reauth,
            },
        )
        .await
    }

    /// The session this browser holds, resolved like the middleware does.
    pub(crate) fn resolve(&mut self, h: &Harness) -> Option<RemoteSessionCtx> {
        let resolved = h.auth.resolve(&self.cookies(), &self.client).ok()?;
        self.apply(&resolved.set_cookie.into_iter().collect::<Vec<_>>());
        Some(resolved.ctx)
    }
}

pub(crate) fn logged_in(reply: &AuthReply) -> RemoteSession {
    match &reply.response {
        AuthResponse::LoggedIn { session } => session.clone(),
        other => panic!("not logged in: {other:?}"),
    }
}

pub(crate) fn failure(reply: &AuthReply) -> (nail_api::remote::AuthFailure, Option<u32>) {
    match &reply.response {
        AuthResponse::Failed {
            reason,
            retry_after_secs,
        } => (*reason, *retry_after_secs),
        other => panic!("did not fail: {other:?}"),
    }
}

pub(crate) fn read_only() -> RemoteGrant {
    RemoteAccessPreset::ReadOnly.grant()
}

pub(crate) fn has_scope(grant: &RemoteGrant, scope: McpScope) -> bool {
    grant.mail.has(scope)
}

// ---------------------------------------------------------------------------
// Software authenticator
// ---------------------------------------------------------------------------

fn b64(bytes: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(bytes)
}

fn cbor_head(major: u8, len: usize) -> Vec<u8> {
    let major = major << 5;
    match u8::try_from(len) {
        Ok(small @ 0..=23) => vec![major | small],
        Ok(byte) => vec![major | 0x18, byte],
        Err(_) => {
            let mut v = vec![major | 0x19];
            let len = u16::try_from(len).expect("CBOR test values stay below 64 KiB");
            v.extend_from_slice(&len.to_be_bytes());
            v
        }
    }
}

fn cbor_bytes(bytes: &[u8]) -> Vec<u8> {
    let mut v = cbor_head(2, bytes.len());
    v.extend_from_slice(bytes);
    v
}

fn cbor_text(text: &str) -> Vec<u8> {
    let mut v = cbor_head(3, text.len());
    v.extend_from_slice(text.as_bytes());
    v
}

/// A passkey authenticator with one Ed25519 credential.
pub(crate) struct Authenticator {
    key: SigningKey,
    pub credential_id: Vec<u8>,
    pub user_handle: Option<String>,
    pub counter: u32,
    pub origin: String,
}

impl Authenticator {
    pub(crate) fn new(seed: u8) -> Self {
        Self {
            key: SigningKey::from_bytes(&[seed; 32]),
            credential_id: vec![seed; 16],
            user_handle: None,
            counter: 0,
            origin: ORIGIN.to_owned(),
        }
    }

    fn cose_key(&self) -> Vec<u8> {
        let mut key = vec![0xA4, 0x01, 0x01, 0x03, 0x27, 0x20, 0x06, 0x21];
        key.extend(cbor_bytes(self.key.verifying_key().as_bytes()));
        key
    }

    fn client_data(&self, kind: &str, challenge: &str) -> Vec<u8> {
        format!(
            r#"{{"type":"{kind}","challenge":"{challenge}","origin":"{}","crossOrigin":false}}"#,
            self.origin
        )
        .into_bytes()
    }

    /// `navigator.credentials.create()` for these creation options.
    pub(crate) fn create(&mut self, options: &serde_json::Value) -> serde_json::Value {
        let challenge = options["challenge"].as_str().unwrap();
        let rp_id = options["rp"]["id"].as_str().unwrap();
        self.user_handle = Some(options["user"]["id"].as_str().unwrap().to_owned());
        let mut auth_data = Sha256::digest(rp_id.as_bytes()).to_vec();
        auth_data.push(0x45); // UP | UV | AT
        auth_data.extend_from_slice(&self.counter.to_be_bytes());
        auth_data.extend_from_slice(&[0; 16]);
        let id_len = u16::try_from(self.credential_id.len()).expect("short credential id");
        auth_data.extend_from_slice(&id_len.to_be_bytes());
        auth_data.extend_from_slice(&self.credential_id);
        auth_data.extend(self.cose_key());
        let mut attestation = vec![0xA3];
        attestation.extend(cbor_text("fmt"));
        attestation.extend(cbor_text("none"));
        attestation.extend(cbor_text("attStmt"));
        attestation.push(0xA0);
        attestation.extend(cbor_text("authData"));
        attestation.extend(cbor_bytes(&auth_data));
        let client_data = self.client_data("webauthn.create", challenge);
        serde_json::json!({
            "id": b64(&self.credential_id),
            "rawId": b64(&self.credential_id),
            "type": "public-key",
            "response": {
                "clientDataJSON": b64(&client_data),
                "attestationObject": b64(&attestation),
                "transports": ["internal"],
            },
            "clientExtensionResults": {},
            "authenticatorAttachment": "platform",
        })
    }

    /// `navigator.credentials.get()` for these request options.
    pub(crate) fn get(&mut self, options: &serde_json::Value) -> serde_json::Value {
        self.counter += 1;
        self.assert_with(options, self.counter)
    }

    /// An assertion with an explicit signature counter.
    pub(crate) fn assert_with(
        &self,
        options: &serde_json::Value,
        counter: u32,
    ) -> serde_json::Value {
        let challenge = options["challenge"].as_str().unwrap();
        let rp_id = options["rpId"].as_str().unwrap();
        let mut auth_data = Sha256::digest(rp_id.as_bytes()).to_vec();
        auth_data.push(0x05); // UP | UV
        auth_data.extend_from_slice(&counter.to_be_bytes());
        let client_data = self.client_data("webauthn.get", challenge);
        let mut signed = auth_data.clone();
        signed.extend_from_slice(&Sha256::digest(&client_data));
        let signature = self.key.sign(&signed);
        serde_json::json!({
            "id": b64(&self.credential_id),
            "rawId": b64(&self.credential_id),
            "type": "public-key",
            "response": {
                "clientDataJSON": b64(&client_data),
                "authenticatorData": b64(&auth_data),
                "signature": b64(&signature.to_bytes()),
                "userHandle": self.user_handle.clone().unwrap(),
            },
            "clientExtensionResults": {},
            "authenticatorAttachment": "platform",
        })
    }
}

pub(crate) fn challenge(reply: &AuthReply) -> (String, serde_json::Value) {
    match &reply.response {
        AuthResponse::PasskeyChallenge {
            challenge_id,
            options,
        } => (challenge_id.clone(), options.clone()),
        other => panic!("no challenge: {other:?}"),
    }
}
