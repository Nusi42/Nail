//! Remote-UI authentication (plan M2b WP41; contract ADR 0036).
//!
//! **Stub (WP40).** This crate is WP41's slot. It compiles, declares its
//! dependencies and checks them with smoke tests (`tests/libraries.rs`)
//! so the package starts from a verified base.
//!
//! # Expected shape
//!
//! A library over `nail-store` (schema v6) and the contract in
//! [`nail_api::remote`]; it does **not** depend on `nail-core`. WP45 mounts
//! it into `nail-server` and calls it from `nail-core` for the admin
//! requests that need hashing (passwords, TOTP, pairing codes).
//!
//! * **Credentials.** Passwords as Argon2id PHC strings (`argon2`, OWASP
//!   parameters, a minimum length policy); TOTP (RFC 6238, `totp-rs`,
//!   SHA-1 / 6 digits / 30 s, ±1 step) with the secret in the OS keychain
//!   through [`nail_api::engine::SecretStore`] (the store keeps only the
//!   `keychain:` reference) and each step accepted once
//!   (`Store::accept_totp_step`); passkeys with `webauthn_rp` (relying-party
//!   id = the configured host name, user verification required, sign-count
//!   checks; ceremonies kept in memory with a short expiry).
//! * **Pairing.** Codes created on the desktop (random, unambiguous
//!   alphabet, shown as `XXXX-XXXX`, normalized before hashing, single use,
//!   10 minutes by default); redeeming one pairs the browser (device
//!   cookie [`nail_api::remote::DEVICE_COOKIE`] with a random secret, stored
//!   hashed) and starts a session.
//! * **Sessions.** Random tokens in [`nail_api::remote::SESSION_COOKIE`]
//!   (`HttpOnly; Secure; SameSite=Strict; Path=/`), bound to the device
//!   cookie, idle and absolute expiry from
//!   [`nail_api::remote::RemoteUiSettings`], rotated while in use (the old
//!   token stays valid for a short grace period), revocable; the CSRF token
//!   of [`nail_api::remote::AuthStatus`] checked on every state-changing
//!   request ([`nail_api::remote::CSRF_HEADER`]).
//! * **Re-authentication** for sensitive actions (grants, key export,
//!   remote-UI settings): `reauth_required` unless the session
//!   re-authenticated within `reauth_minutes`.
//! * **Brute force.** Per-IP and per-login-name counters in the store
//!   (`remote_throttle`), exponential backoff (`rate_limited` with
//!   `retry_after_secs`) and a lockout that raises
//!   [`nail_api::remote::RemoteAlert::Lockout`]; a new device raises
//!   `NewDevice`.
//! * **Authorization.** `required_access(&Request)`: an exhaustive match
//!   from every [`nail_api::Request`] to the scope (or `admin`) a remote
//!   session needs, so a new request cannot be forgotten.
//! * **axum.** A handler for `POST` [`nail_api::remote::AUTH_PATH`]
//!   ([`nail_api::remote::AuthRequest`] → [`nail_api::remote::AuthResponse`])
//!   and a middleware / extractor that resolves the session for the
//!   WebSocket upgrade, the blob route and the proxy route.
//! * **Audit.** Every login, failed login, pairing, credential change and
//!   denied request is written with `Store::append_remote_audit`.
//!
//! Tests include brute-force and replay attempts (TOTP codes, pairing
//! codes, rotated session tokens, WebAuthn challenges).
