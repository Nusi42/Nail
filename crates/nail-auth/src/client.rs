//! [`OAuthClient`]: one provider's endpoints and client registration, with
//! authorization URLs, code exchange and refresh on top of [`oauth2`] and
//! Nail's [`reqwest`] client.

use std::fmt;
use std::time::{Duration, Instant};

use oauth2::basic::{BasicClient, BasicErrorResponseType, BasicTokenResponse};
use oauth2::{
    AuthType, AuthUrl, AuthorizationCode, ClientId, ClientSecret, CsrfToken, EndpointNotSet,
    EndpointSet, PkceCodeChallenge, PkceCodeVerifier, RedirectUrl, RefreshToken, RequestTokenError,
    Scope, TokenResponse, TokenUrl,
};

use crate::error::AuthError;
use crate::http::{Fetcher, HttpError};
use crate::providers::ProviderSpec;

type ConfiguredClient =
    BasicClient<EndpointSet, EndpointNotSet, EndpointNotSet, EndpointNotSet, EndpointSet>;

/// Tokens from the token endpoint. `Debug` never prints them.
#[derive(Clone, PartialEq, Eq)]
pub struct TokenSet {
    /// The access token for XOAUTH2 / OAUTHBEARER.
    pub access_token: String,
    /// The refresh token, if the server issued (or rotated) one.
    pub refresh_token: Option<String>,
    /// When the access token expires (from `expires_in`; 1 hour when the
    /// server does not say).
    pub expires_at: Instant,
    /// Granted scopes, if the server listed them.
    pub scopes: Vec<String>,
}

impl fmt::Debug for TokenSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TokenSet")
            .field("access_token", &"<redacted>")
            .field(
                "refresh_token",
                &self.refresh_token.as_ref().map(|_| "<redacted>"),
            )
            .field("expires_at", &self.expires_at)
            .field("scopes", &self.scopes)
            .finish()
    }
}

impl TokenSet {
    /// Lifetime assumed when the server gives no `expires_in`.
    pub const DEFAULT_LIFETIME: Duration = Duration::from_hours(1);

    fn from_response(response: &BasicTokenResponse) -> Self {
        Self {
            access_token: response.access_token().secret().clone(),
            refresh_token: response.refresh_token().map(|t| t.secret().clone()),
            expires_at: Instant::now() + response.expires_in().unwrap_or(Self::DEFAULT_LIFETIME),
            scopes: response
                .scopes()
                .map(|s| s.iter().map(|s| s.to_string()).collect())
                .unwrap_or_default(),
        }
    }

    /// Whether the access token is still valid for at least `margin`.
    pub fn is_fresh(&self, margin: Duration) -> bool {
        Instant::now() + margin < self.expires_at
    }
}

/// A usable provider: endpoints, client id and (resolved) client secret.
#[derive(Clone)]
pub struct OAuthClient {
    spec: ProviderSpec,
    client_id: String,
    client_secret: Option<String>,
    http: reqwest::Client,
    /// The connection log of token requests (ADR 0190).
    tap: nail_connlog::HttpTap,
}

impl fmt::Debug for OAuthClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OAuthClient")
            .field("provider", &self.spec.id)
            .field("client_id", &self.client_id)
            .field(
                "client_secret",
                &self.client_secret.as_ref().map(|_| "<redacted>"),
            )
            .finish_non_exhaustive()
    }
}

fn map_request_error(
    err: RequestTokenError<HttpError, oauth2::basic::BasicErrorResponse>,
) -> AuthError {
    match err {
        RequestTokenError::ServerResponse(response) => {
            let description = response
                .error_description()
                .map(|d| format!(": {d}"))
                .unwrap_or_default();
            match response.error() {
                BasicErrorResponseType::InvalidGrant => {
                    AuthError::InvalidGrant(format!("invalid_grant{description}"))
                }
                other => AuthError::Refused(format!("{}{description}", other.as_ref())),
            }
        }
        RequestTokenError::Request(e) => AuthError::Network(e.0),
        RequestTokenError::Parse(e, _body) => {
            AuthError::Protocol(format!("unparsable token response: {e}"))
        }
        RequestTokenError::Other(message) => AuthError::Protocol(message),
    }
}

impl OAuthClient {
    /// A client for `spec` (which must have a client id) with the resolved
    /// client secret, sending requests through `http`.
    pub fn new(
        spec: ProviderSpec,
        client_secret: Option<String>,
        http: reqwest::Client,
    ) -> Result<Self, AuthError> {
        let client_id = spec
            .client_id
            .clone()
            .ok_or_else(|| AuthError::Config(format!("no client id for provider {:?}", spec.id)))?;
        let tap = nail_connlog::HttpTap::off();
        if let Some(secret) = &client_secret {
            tap.add_secret(secret);
        }
        let client = Self {
            spec,
            client_id,
            client_secret,
            http,
            tap,
        };
        client.oauth(None)?;
        Ok(client)
    }

    /// Logs token requests into `tap` (ADR 0190); the client secret is
    /// registered as a secret of it.
    #[must_use]
    pub fn with_tap(mut self, tap: nail_connlog::HttpTap) -> Self {
        if let Some(secret) = &self.client_secret {
            tap.add_secret(secret);
        }
        self.tap = tap;
        self
    }

    /// The provider.
    pub fn spec(&self) -> &ProviderSpec {
        &self.spec
    }

    fn oauth(&self, redirect: Option<&str>) -> Result<ConfiguredClient, AuthError> {
        let config =
            |e: url::ParseError| AuthError::Config(format!("provider {:?}: {e}", self.spec.id));
        let mut client = BasicClient::new(ClientId::new(self.client_id.clone()))
            .set_auth_uri(AuthUrl::new(self.spec.auth_url.clone()).map_err(config)?)
            .set_token_uri(TokenUrl::new(self.spec.token_url.clone()).map_err(config)?)
            // Client credentials in the body: required by public clients
            // (Microsoft) and accepted by Google.
            .set_auth_type(AuthType::RequestBody);
        if let Some(secret) = &self.client_secret {
            client = client.set_client_secret(ClientSecret::new(secret.clone()));
        }
        if let Some(redirect) = redirect {
            client =
                client.set_redirect_uri(RedirectUrl::new(redirect.to_owned()).map_err(config)?);
        }
        Ok(client)
    }

    /// The authorization URL for a PKCE (S256) flow redirecting to
    /// `redirect`. Returns the URL, the `state` value and the verifier.
    pub fn authorize_url(
        &self,
        redirect: &str,
        login_hint: Option<&str>,
    ) -> Result<(url::Url, CsrfToken, PkceCodeVerifier), AuthError> {
        self.authorize_url_with(redirect, login_hint, None)
    }

    /// [`Self::authorize_url`] asking for `scopes` instead of the
    /// provider's (`None` = the provider's).
    pub fn authorize_url_with(
        &self,
        redirect: &str,
        login_hint: Option<&str>,
        scopes: Option<&[String]>,
    ) -> Result<(url::Url, CsrfToken, PkceCodeVerifier), AuthError> {
        let (challenge, verifier) = PkceCodeChallenge::new_random_sha256();
        let client = self.oauth(Some(redirect))?;
        let scopes = scopes.unwrap_or(&self.spec.scopes);
        let mut request = client
            .authorize_url(CsrfToken::new_random)
            .set_pkce_challenge(challenge)
            .add_scopes(scopes.iter().cloned().map(Scope::new));
        for (name, value) in &self.spec.extra_auth_params {
            request = request.add_extra_param(name.clone(), value.clone());
        }
        if let Some(hint) = login_hint.filter(|h| !h.is_empty()) {
            request = request.add_extra_param("login_hint", hint.to_owned());
        }
        let (url, state) = request.url();
        Ok((url, state, verifier))
    }

    /// Exchanges an authorization code (with its PKCE verifier) for tokens.
    pub async fn exchange_code(
        &self,
        code: String,
        verifier: PkceCodeVerifier,
        redirect: &str,
    ) -> Result<TokenSet, AuthError> {
        let client = self.oauth(Some(redirect))?;
        let fetch = Fetcher(self.http.clone(), self.tap.clone());
        let response = client
            .exchange_code(AuthorizationCode::new(code))
            .set_pkce_verifier(verifier)
            .request_async(&fetch)
            .await
            .map_err(map_request_error)?;
        Ok(TokenSet::from_response(&response))
    }

    /// Gets a new access token with a refresh token. The result carries a
    /// new refresh token only when the server rotated it.
    pub async fn refresh(&self, refresh_token: &str) -> Result<TokenSet, AuthError> {
        let client = self.oauth(None)?;
        self.tap.add_secret(refresh_token);
        let fetch = Fetcher(self.http.clone(), self.tap.clone());
        let token = RefreshToken::new(refresh_token.to_owned());
        let response = client
            .exchange_refresh_token(&token)
            .request_async(&fetch)
            .await
            .map_err(map_request_error)?;
        Ok(TokenSet::from_response(&response))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::{GOOGLE, builtin};

    fn google() -> OAuthClient {
        let spec = ProviderSpec {
            client_id: Some("cid.apps".into()),
            ..builtin(GOOGLE).unwrap()
        };
        OAuthClient::new(spec, Some("s3cret".into()), crate::http_client().unwrap()).unwrap()
    }

    #[test]
    fn authorization_url_has_pkce_scopes_and_hint() {
        let (url, state, verifier) = google()
            .authorize_url("http://127.0.0.1:5000/", Some("ada@gmail.com"))
            .unwrap();
        let query: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
        assert_eq!(url.host_str(), Some("accounts.google.com"));
        assert_eq!(query["response_type"], "code");
        assert_eq!(query["client_id"], "cid.apps");
        assert_eq!(query["redirect_uri"], "http://127.0.0.1:5000/");
        assert_eq!(query["code_challenge_method"], "S256");
        assert_eq!(query["scope"], "https://mail.google.com/");
        assert_eq!(query["login_hint"], "ada@gmail.com");
        assert_eq!(query["access_type"], "offline");
        assert_eq!(&query["state"], state.secret());
        assert!(!query.contains_key("client_secret"));
        assert!(verifier.secret().len() >= 43);
    }

    #[test]
    fn a_flow_can_ask_for_other_scopes() {
        let scopes = vec!["Mail.ReadWrite".to_owned(), "offline_access".to_owned()];
        let (url, _, _) = google()
            .authorize_url_with("http://127.0.0.1:5000/", None, Some(&scopes))
            .unwrap();
        let query: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
        assert_eq!(query["scope"], "Mail.ReadWrite offline_access");
        assert!(!query.contains_key("login_hint"));
    }

    #[test]
    fn debug_is_redacted() {
        let text = format!("{:?}", google());
        assert!(!text.contains("s3cret"));
        let tokens = TokenSet {
            access_token: "ya29.abc".into(),
            refresh_token: Some("1//refresh".into()),
            expires_at: Instant::now() + Duration::from_secs(10),
            scopes: vec![],
        };
        let text = format!("{tokens:?}");
        assert!(!text.contains("ya29") && !text.contains("1//refresh"));
        assert!(tokens.is_fresh(Duration::from_secs(1)));
        assert!(!tokens.is_fresh(Duration::from_mins(1)));
    }

    #[test]
    fn needs_a_client_id() {
        let err = OAuthClient::new(
            builtin(GOOGLE).unwrap(),
            None,
            crate::http_client().unwrap(),
        )
        .unwrap_err();
        assert!(matches!(err, AuthError::Config(_)));
    }
}
