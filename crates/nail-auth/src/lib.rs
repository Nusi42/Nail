//! Authentication support for Nail.
//!
//! **Stub (M1a WP10).** Work package WP12 fills this crate in:
//!
//! * a keychain-backed [`nail_api::engine::SecretStore`] on [`keyring`]
//!   (Windows Credential Manager, macOS Keychain, Secret Service); the
//!   in-memory store for tests is [`nail_api::engine::MemorySecretStore`];
//! * OAuth2 authorization code + PKCE with a loopback redirect
//!   (`http://127.0.0.1:<port>/`) using [`oauth2`], token refresh, and an
//!   [`nail_api::engine::AccessTokenProvider`];
//! * the provider registry: `google` and `microsoft` built in, client ids
//!   from `accounts.kdl` ([`nail_api::accounts::OAuthProviderConfig`]).
//!
//! HTTP goes through [`reqwest`] 0.13 built with `rustls-no-provider`: build
//! the client with [`http_client`] (ring provider + platform verifier), or
//! hand it a preconfigured rustls config with custom roots in tests.
//! Tests use the mock OAuth server of `nail-testkit-net`.
//! Expected shape: `docs/adr/0012-m1a-contract.md`.

#![warn(missing_docs)]

use std::sync::Arc;

use tokio_rustls::rustls;

/// The keychain service Nail's entries live under (see
/// [`nail_api::accounts::SecretRef`]).
pub const KEYCHAIN_SERVICE: &str = nail_api::accounts::SecretRef::SERVICE;

/// A rustls client configuration with the `ring` provider and the
/// platform's certificate verifier, as the rest of Nail uses.
pub fn tls_config() -> Result<rustls::ClientConfig, rustls::Error> {
    use rustls_platform_verifier::BuilderVerifierExt as _;
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    Ok(rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()?
        .with_platform_verifier()?
        .with_no_client_auth())
}

/// An HTTP client for OAuth2 and discovery requests: rustls with the
/// platform verifier, no redirects (OAuth2 token endpoints must not
/// redirect).
pub fn http_client() -> Result<reqwest::Client, Box<dyn std::error::Error + Send + Sync>> {
    Ok(reqwest::Client::builder()
        .tls_backend_preconfigured(tls_config()?)
        .redirect(reqwest::redirect::Policy::none())
        .build()?)
}

/// The type the keychain backend wraps (kept referenced so the dependency
/// is compiled and checked on every platform).
pub type KeychainEntry = keyring::Entry;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_challenge_is_s256() {
        let (challenge, verifier) = oauth2::PkceCodeChallenge::new_random_sha256();
        assert_eq!(challenge.method().as_str(), "S256");
        assert!(verifier.secret().len() >= 43);
        let url = url::Url::parse("http://127.0.0.1:1234/callback").unwrap();
        assert_eq!(url.port(), Some(1234));
        assert_eq!(KEYCHAIN_SERVICE, "nail");
    }

    #[test]
    fn http_client_builds_without_a_process_wide_crypto_provider() {
        http_client().unwrap();
    }
}
