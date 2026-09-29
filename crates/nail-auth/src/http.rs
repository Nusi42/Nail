//! The HTTP client for OAuth2 and discovery requests, and the bridge from
//! [`oauth2`]'s request type to [`reqwest`].

use std::sync::Arc;

use tokio_rustls::rustls;
use tokio_rustls::rustls::pki_types::CertificateDer;

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

/// A rustls client configuration trusting only `roots` (DER), for tests
/// against local TLS servers.
pub fn tls_config_with_roots(roots: &[Vec<u8>]) -> Result<rustls::ClientConfig, rustls::Error> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let mut store = rustls::RootCertStore::empty();
    for der in roots {
        store.add(CertificateDer::from(der.clone()))?;
    }
    Ok(rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()?
        .with_root_certificates(store)
        .with_no_client_auth())
}

/// Error building an HTTP client.
pub type BuildError = Box<dyn std::error::Error + Send + Sync>;

/// A client builder with Nail's defaults: the given TLS configuration, no
/// redirects (OAuth2 token endpoints must not redirect; discovery follows
/// redirects itself where the protocol allows), 30 s timeout.
pub fn http_client_builder(tls: rustls::ClientConfig) -> reqwest::ClientBuilder {
    reqwest::Client::builder()
        .tls_backend_preconfigured(tls)
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(30))
        .user_agent(concat!("Nail/", env!("CARGO_PKG_VERSION")))
}

/// An HTTP client for OAuth2 and discovery requests: rustls with the
/// platform verifier, no redirects.
pub fn http_client() -> Result<reqwest::Client, BuildError> {
    Ok(http_client_builder(tls_config()?).build()?)
}

/// The error of an HTTP call made on behalf of [`oauth2`].
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct HttpError(pub String);

/// [`oauth2`]'s HTTP client trait on [`reqwest`], with a named `Send`
/// future (closures trip up `Send` inference across the token request).
#[derive(Debug, Clone)]
pub(crate) struct Fetcher(pub(crate) reqwest::Client);

impl<'c> oauth2::AsyncHttpClient<'c> for Fetcher {
    type Error = HttpError;
    type Future = std::pin::Pin<
        Box<dyn Future<Output = Result<oauth2::HttpResponse, HttpError>> + Send + 'c>,
    >;

    fn call(&'c self, request: oauth2::HttpRequest) -> Self::Future {
        Box::pin(execute(&self.0, request))
    }
}

/// Performs an [`oauth2::HttpRequest`] with `client`.
pub(crate) async fn execute(
    client: &reqwest::Client,
    request: oauth2::HttpRequest,
) -> Result<oauth2::HttpResponse, HttpError> {
    let (parts, body) = request.into_parts();
    let mut builder = client
        .request(parts.method, parts.uri.to_string())
        .body(body);
    for (name, value) in &parts.headers {
        builder = builder.header(name, value);
    }
    let response = builder
        .send()
        .await
        .map_err(|e| HttpError(without_url(&e)))?;
    let mut out = oauth2::http::Response::builder().status(response.status());
    for (name, value) in response.headers() {
        out = out.header(name, value);
    }
    let body = response
        .bytes()
        .await
        .map_err(|e| HttpError(without_url(&e)))?;
    out.body(body.to_vec())
        .map_err(|e| HttpError(e.to_string()))
}

/// The error text without the URL (token URLs are harmless, but query
/// strings of other requests might carry codes).
fn without_url(err: &reqwest::Error) -> String {
    let mut text = err.to_string();
    if let Some(url) = err.url() {
        text = text.replace(url.as_str(), "<url>");
    }
    if err.is_timeout() {
        format!("timeout: {text}")
    } else {
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_clients_without_a_process_wide_crypto_provider() {
        http_client().unwrap();
        http_client_builder(tls_config_with_roots(&[]).unwrap())
            .build()
            .unwrap();
        assert!(tls_config_with_roots(&[vec![1, 2, 3]]).is_err());
    }
}
