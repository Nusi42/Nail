//! Authentication support for Nail.
//!
//! * [`KeychainSecretStore`]: [`nail_api::engine::SecretStore`] on the OS
//!   keychain via [`keyring`] (Windows Credential Manager, macOS Keychain,
//!   Secret Service), chunking values longer than one Windows credential
//!   blob. The in-memory store for tests is
//!   [`nail_api::engine::MemorySecretStore`].
//! * [`ProviderRegistry`]: built-in Google and Microsoft endpoints and mail
//!   scopes (IMAP / POP3 / SMTP), merged with the `oauth-provider` entries
//!   of `accounts.kdl` (client ids come only from there).
//! * [`start_flow`]: authorization code + PKCE with a loopback redirect on
//!   `127.0.0.1:<random port>` (RFC 8252), `state` check, code exchange.
//! * [`TokenManager`]: stores refresh tokens, caches and refreshes access
//!   tokens; the core's [`nail_api::engine::AccessTokenProvider`].
//!
//! HTTP goes through [`reqwest`] 0.13 built with `rustls-no-provider`:
//! [`http_client`] (ring provider + platform verifier, no redirects).
//! Secrets are never logged or included in errors and `Debug` output.
//! See `docs/adr/0017-wp12-outgoing-pop3-auth-discovery.md`.

#![warn(missing_docs)]

mod client;
mod error;
mod flow;
mod http;
mod keychain;
mod manager;
pub mod providers;

pub use client::{OAuthClient, TokenSet};
pub use error::AuthError;
pub use flow::{CallbackPages, FlowCanceller, FlowOptions, PendingFlow, start_flow};
pub use http::{
    BuildError, HttpError, http_client, http_client_builder, tls_config, tls_config_with_roots,
};
pub use keychain::{CHUNK_BYTES, KeychainBackend, KeychainSecretStore, OsKeychain};
pub use manager::{EXPIRY_MARGIN, TokenManager};
pub use providers::{GOOGLE, MICROSOFT, ProviderRegistry, ProviderSpec, provider_for_host};

/// The keychain service Nail's entries live under (see
/// [`nail_api::accounts::SecretRef`]).
pub const KEYCHAIN_SERVICE: &str = nail_api::accounts::SecretRef::SERVICE;
