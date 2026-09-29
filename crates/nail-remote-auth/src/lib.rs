//! Remote-UI authentication (plan M2b WP41; contract ADR 0036, package
//! ADR 0037).
//!
//! A library over `nail-store` (schema v6) and the contract in
//! [`nail_api::remote`]; it does not depend on `nail-core`. `nail-server`
//! mounts its axum pieces and the core calls it for the admin requests
//! that need hashing (WP45).
//!
//! * **Credentials** — passwords as Argon2id PHC strings with OWASP
//!   parameters and a length policy ([`password`]); TOTP (RFC 6238,
//!   SHA-1 / 6 digits / 30 s, ±1 step) with the secret in the OS keychain
//!   and every step accepted once ([`totp`]); passkeys with `webauthn_rp`
//!   (host-name relying-party id, never an IP address; user verification
//!   required; signature counter checked; ceremonies in memory, single
//!   use, short-lived) ([`passkey`]).
//! * **Pairing** — one-time codes created (and so approved) on the
//!   desktop, `XXXX-XXXX` from an unambiguous alphabet, stored hashed,
//!   single use, 10 minutes by default ([`pairing`]). Redeeming one
//!   creates the pending device and its first session.
//! * **Sessions** — random tokens in `HttpOnly; Secure; SameSite=Strict`
//!   `__Host-` cookies, bound to the device cookie, idle and absolute
//!   expiry from the settings, rotated while in use with a short grace
//!   period, revocable ([`session`], [`RemoteAuth::resolve`]).
//! * **Re-authentication** — admin and sensitive requests need a login or
//!   re-authentication within `reauth_minutes` ([`access`]).
//! * **Brute force** — per-address, per-account and global (pairing)
//!   counters with exponential backoff and lockouts that raise
//!   `RemoteSecurityAlert` ([`throttle`]).
//! * **Authorization** — [`required_access`]: an exhaustive map from
//!   every [`nail_api::Request`] to the scope or `admin` it needs;
//!   [`RemoteAuth::authorize`] checks it and audits.
//! * **axum** — [`http::router`] (`POST /api/auth`),
//!   [`http::require_session`] middleware and the [`RemoteSessionCtx`]
//!   extractor.
//! * **Audit** — every login, failed login, pairing, lockout, credential
//!   change, logout and denied request.
//!
//! Secrets in memory are zeroized ([`zeroize::Zeroizing`]); comparisons
//! of secrets are constant-time (CSRF tokens, TOTP codes, password
//! hashes); stored secrets are hashes only.

pub mod access;
pub mod admin;
pub mod clock;
pub mod error;
pub mod events;
pub mod http;
pub mod login;
pub mod pairing;
pub mod passkey;
pub mod password;
pub mod secret;
pub mod service;
pub mod session;
pub mod throttle;
pub mod totp;

pub use access::{Access, Own, RequestAccess, required_access};
pub use clock::{Clock, ManualClock, SystemClock};
pub use error::{AuthError, Result};
pub use events::{EventSink, NoEvents, RecordingSink};
pub use login::AuthReply;
pub use passkey::RelyingParty;
pub use service::{AuthConfig, AuthDeps, AuthPolicy, NoSession, RemoteAuth, Resolved};
pub use session::{ClientMeta, Cookies, RemoteSessionCtx};
