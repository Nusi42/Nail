//! A fake OAuth2 access-token source for the engine tests: hands out a
//! stale token until a refresh is forced, counts the calls, and can stand
//! for a revoked sign-in.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use nail_api::ErrorCode;
use nail_api::accounts::SecretRef;
use nail_api::engine::{AccessTokenProvider, BoxFuture, EngineError};

/// The token handed out before a refresh.
pub const STALE_TOKEN: &str = "stale";

/// A fake [`AccessTokenProvider`] for one provider id.
#[derive(Debug)]
pub struct FakeTokens {
    provider: &'static str,
    fresh: &'static str,
    /// Refreshes keep the fresh token for later calls too.
    sticky: bool,
    /// The sign-in is revoked: every call fails with this code.
    revoked: Option<ErrorCode>,
    refreshed: AtomicBool,
    calls: AtomicU32,
    forced: AtomicU32,
}

impl FakeTokens {
    /// [`STALE_TOKEN`], and `"fresh"` on a forced refresh, for `provider`.
    pub fn new(provider: &'static str) -> Self {
        Self {
            provider,
            fresh: "fresh",
            sticky: false,
            revoked: None,
            refreshed: AtomicBool::new(false),
            calls: AtomicU32::new(0),
            forced: AtomicU32::new(0),
        }
    }

    /// Fails every call with `code` ("revoked").
    pub fn revoked(provider: &'static str, code: ErrorCode) -> Self {
        Self {
            revoked: Some(code),
            ..Self::new(provider)
        }
    }

    /// Hands out `fresh` from the first forced refresh on.
    pub fn refreshing_to(provider: &'static str, fresh: &'static str) -> Self {
        Self {
            fresh,
            sticky: true,
            ..Self::new(provider)
        }
    }
}

impl FakeTokens {
    /// Calls so far.
    pub fn calls(&self) -> u32 {
        self.calls.load(Ordering::SeqCst)
    }

    /// Calls that forced a refresh.
    pub fn forced(&self) -> u32 {
        self.forced.load(Ordering::SeqCst)
    }
}

impl AccessTokenProvider for FakeTokens {
    fn access_token<'a>(
        &'a self,
        provider: &'a str,
        _secret: &'a SecretRef,
        force_refresh: bool,
    ) -> BoxFuture<'a, Result<String, EngineError>> {
        Box::pin(async move {
            assert_eq!(provider, self.provider);
            self.calls.fetch_add(1, Ordering::SeqCst);
            if let Some(code) = self.revoked {
                return Err(EngineError::permanent(code, "revoked"));
            }
            if force_refresh {
                self.forced.fetch_add(1, Ordering::SeqCst);
                self.refreshed.store(true, Ordering::SeqCst);
            }
            let fresh = force_refresh || (self.sticky && self.refreshed.load(Ordering::SeqCst));
            Ok(if fresh { self.fresh } else { STALE_TOKEN }.to_owned())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn token(tokens: &FakeTokens, force: bool) -> Result<String, EngineError> {
        tokens
            .access_token("p", &SecretRef::parse("keychain:x/y").unwrap(), force)
            .await
    }

    #[tokio::test]
    async fn stale_until_forced() {
        let tokens = FakeTokens::new("p");
        assert_eq!(token(&tokens, false).await.unwrap(), STALE_TOKEN);
        assert_eq!(token(&tokens, true).await.unwrap(), "fresh");
        assert_eq!(token(&tokens, false).await.unwrap(), STALE_TOKEN);
        assert_eq!(tokens.calls(), 3);
        assert_eq!(tokens.forced(), 1);

        let sticky = FakeTokens::refreshing_to("p", "tok");
        token(&sticky, true).await.unwrap();
        assert_eq!(token(&sticky, false).await.unwrap(), "tok");

        let revoked = FakeTokens::revoked("p", ErrorCode::AuthFailed);
        assert_eq!(
            token(&revoked, false).await.unwrap_err().code,
            ErrorCode::AuthFailed
        );
    }
}
