//! Signed, expiring proxy tokens.
//!
//! A token is `base64url(body) "." base64url(tag)` where
//!
//! ```text
//! body = version (1) | purpose (1) | expires, Unix seconds (8, big endian)
//!      | message id length (2, big endian) | message id | payload
//! tag  = HMAC-SHA-256(per-install key, "nail-proxy\0" | body)
//! ```
//!
//! The payload is the original resource URL for [`Purpose::Resource`] and
//! empty for [`Purpose::Render`] (the sandbox origin's mail documents).
//! Only the holder of the key — the core — can mint a token, and it mints
//! them only for URLs found in a message it renders, so the proxy never
//! fetches a URL a client chose: it is not an open proxy. Tags are compared
//! in constant time.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;
use url::Url;

use crate::key::ProxyKey;
use crate::policy::{self, UrlRefusal};

type HmacSha256 = Hmac<Sha256>;

const VERSION: u8 = 1;
const DOMAIN: &[u8] = b"nail-proxy\0";
const TAG_LEN: usize = 32;
/// Longest message id a token carries.
pub const MAX_MESSAGE_ID_LEN: usize = 512;
/// Longest token accepted (URL + message id + overhead, base64).
pub const MAX_TOKEN_LEN: usize = 6 * 1024;

/// Default lifetime of resource tokens: a rendered message keeps working
/// for a working day, then the reader asks the core again.
pub const DEFAULT_RESOURCE_TTL: Duration = Duration::from_secs(12 * 60 * 60);
/// Default lifetime of render tokens (sandbox documents are loaded right
/// after the core hands out the URL).
pub const DEFAULT_RENDER_TTL: Duration = Duration::from_secs(60 * 60);

/// What a token lets its holder do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Purpose {
    /// Fetch one remote resource of one message through the proxy.
    Resource,
    /// Load one message's sanitized HTML on the sandbox origin.
    Render,
}

impl Purpose {
    fn byte(self) -> u8 {
        match self {
            Self::Resource => 1,
            Self::Render => 2,
        }
    }

    fn from_byte(byte: u8) -> Option<Self> {
        match byte {
            1 => Some(Self::Resource),
            2 => Some(Self::Render),
            _ => None,
        }
    }
}

/// Why a token was not signed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum SignError {
    /// The URL breaks the proxy's URL rules ([`crate::policy`]).
    #[error("{0}")]
    Url(#[from] UrlRefusal),
    /// Empty or overlong message id.
    #[error("invalid message id")]
    MessageId,
}

/// Why a token was refused. Deliberately coarse towards clients (the
/// server answers `404` for all of them).
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum TokenError {
    /// Not a token of ours, tampered with, or signed with another key.
    #[error("invalid token")]
    Invalid,
    /// Signed for a different purpose.
    #[error("token for another purpose")]
    WrongPurpose,
    /// Past its expiry.
    #[error("token expired")]
    Expired,
}

/// A verified token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grant {
    /// What it is for.
    pub purpose: Purpose,
    /// The message it belongs to.
    pub message_id: String,
    /// The remote resource ([`Purpose::Resource`] only), already checked
    /// against the URL rules.
    pub url: Option<Url>,
    /// Expiry (Unix seconds).
    pub expires: u64,
}

/// Mints and checks tokens with the per-install [`ProxyKey`].
#[derive(Debug, Clone)]
pub struct Signer {
    key: ProxyKey,
    resource_ttl: Duration,
    render_ttl: Duration,
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

impl Signer {
    /// A signer with the default lifetimes.
    pub fn new(key: ProxyKey) -> Self {
        Self {
            key,
            resource_ttl: DEFAULT_RESOURCE_TTL,
            render_ttl: DEFAULT_RENDER_TTL,
        }
    }

    /// Changes the lifetime of resource tokens.
    #[must_use]
    pub fn with_resource_ttl(mut self, ttl: Duration) -> Self {
        self.resource_ttl = ttl;
        self
    }

    /// Changes the lifetime of render tokens.
    #[must_use]
    pub fn with_render_ttl(mut self, ttl: Duration) -> Self {
        self.render_ttl = ttl;
        self
    }

    /// A token for fetching `url` on behalf of `message_id`. The core calls
    /// this only for URLs it found in that message.
    pub fn sign_resource(&self, message_id: &str, url: &str) -> Result<String, SignError> {
        self.sign_resource_at(message_id, url, now_unix())
    }

    /// [`Self::sign_resource`] at a given time (tests).
    pub fn sign_resource_at(
        &self,
        message_id: &str,
        url: &str,
        now: u64,
    ) -> Result<String, SignError> {
        let url = policy::parse_target(url)?;
        let expires = now.saturating_add(self.resource_ttl.as_secs());
        self.sign(Purpose::Resource, message_id, url.as_str(), expires)
    }

    /// `nail-proxy:<token>`: the form the sanitized HTML carries
    /// ([`nail_api::remote_content::PROXY_URL_SCHEME`]).
    pub fn proxy_url(&self, message_id: &str, url: &str) -> Result<String, SignError> {
        let token = self.sign_resource(message_id, url)?;
        Ok(format!(
            "{}{token}",
            nail_api::remote_content::PROXY_URL_SCHEME
        ))
    }

    /// A rewriting function for one message, for the sanitizer hook in
    /// `nail-mime`: maps a remote URL to its `nail-proxy:` URL, `None` when
    /// the URL must stay blocked (not http(s), local, credentials, …).
    pub fn rewriter<'a>(&'a self, message_id: &'a str) -> impl Fn(&str) -> Option<String> + 'a {
        move |url| self.proxy_url(message_id, url).ok()
    }

    /// A token for loading `message_id`'s HTML on the sandbox origin.
    pub fn sign_render(&self, message_id: &str) -> Result<String, SignError> {
        self.sign_render_at(message_id, now_unix())
    }

    /// [`Self::sign_render`] at a given time (tests).
    pub fn sign_render_at(&self, message_id: &str, now: u64) -> Result<String, SignError> {
        let expires = now.saturating_add(self.render_ttl.as_secs());
        self.sign(Purpose::Render, message_id, "", expires)
    }

    fn sign(
        &self,
        purpose: Purpose,
        message_id: &str,
        payload: &str,
        expires: u64,
    ) -> Result<String, SignError> {
        if message_id.is_empty() || message_id.len() > MAX_MESSAGE_ID_LEN {
            return Err(SignError::MessageId);
        }
        let id_len = u16::try_from(message_id.len()).map_err(|_| SignError::MessageId)?;
        let mut body = Vec::with_capacity(12 + message_id.len() + payload.len());
        body.push(VERSION);
        body.push(purpose.byte());
        body.extend_from_slice(&expires.to_be_bytes());
        body.extend_from_slice(&id_len.to_be_bytes());
        body.extend_from_slice(message_id.as_bytes());
        body.extend_from_slice(payload.as_bytes());
        let tag = self.mac(&body).finalize().into_bytes();
        Ok(format!(
            "{}.{}",
            URL_SAFE_NO_PAD.encode(&body),
            URL_SAFE_NO_PAD.encode(tag)
        ))
    }

    fn mac(&self, body: &[u8]) -> HmacSha256 {
        let mut mac = HmacSha256::new_from_slice(self.key.as_bytes())
            .unwrap_or_else(|_| unreachable!("HMAC takes keys of any length"));
        mac.update(DOMAIN);
        mac.update(body);
        mac
    }

    /// Checks `token` for `purpose` now.
    pub fn verify(&self, token: &str, purpose: Purpose) -> Result<Grant, TokenError> {
        self.verify_at(token, purpose, now_unix())
    }

    /// [`Self::verify`] at a given time (tests).
    pub fn verify_at(&self, token: &str, purpose: Purpose, now: u64) -> Result<Grant, TokenError> {
        let token = token
            .strip_prefix(nail_api::remote_content::PROXY_URL_SCHEME)
            .unwrap_or(token);
        if token.len() > MAX_TOKEN_LEN {
            return Err(TokenError::Invalid);
        }
        let (body, tag) = token.split_once('.').ok_or(TokenError::Invalid)?;
        let body = URL_SAFE_NO_PAD
            .decode(body)
            .map_err(|_| TokenError::Invalid)?;
        let tag = URL_SAFE_NO_PAD
            .decode(tag)
            .map_err(|_| TokenError::Invalid)?;
        if tag.len() != TAG_LEN {
            return Err(TokenError::Invalid);
        }
        // Authenticity first: nothing below looks at unauthenticated data.
        self.mac(&body)
            .verify_slice(&tag)
            .map_err(|_| TokenError::Invalid)?;

        let parsed = parse_body(&body).ok_or(TokenError::Invalid)?;
        if parsed.purpose != purpose {
            return Err(TokenError::WrongPurpose);
        }
        if parsed.expires <= now {
            return Err(TokenError::Expired);
        }
        let url = match purpose {
            Purpose::Resource => {
                // Re-checked: the rules may be stricter than when it was signed.
                Some(policy::parse_target(&parsed.payload).map_err(|_| TokenError::Invalid)?)
            }
            Purpose::Render => {
                if !parsed.payload.is_empty() {
                    return Err(TokenError::Invalid);
                }
                None
            }
        };
        Ok(Grant {
            purpose,
            message_id: parsed.message_id,
            url,
            expires: parsed.expires,
        })
    }
}

struct Body {
    purpose: Purpose,
    expires: u64,
    message_id: String,
    payload: String,
}

fn parse_body(body: &[u8]) -> Option<Body> {
    let (&version, rest) = body.split_first()?;
    if version != VERSION {
        return None;
    }
    let (&purpose, rest) = rest.split_first()?;
    let purpose = Purpose::from_byte(purpose)?;
    let (expires, rest) = rest.split_first_chunk::<8>()?;
    let (id_len, rest) = rest.split_first_chunk::<2>()?;
    let id_len = usize::from(u16::from_be_bytes(*id_len));
    if id_len == 0 || id_len > MAX_MESSAGE_ID_LEN || rest.len() < id_len {
        return None;
    }
    let (message_id, payload) = rest.split_at(id_len);
    Some(Body {
        purpose,
        expires: u64::from_be_bytes(*expires),
        message_id: String::from_utf8(message_id.to_vec()).ok()?,
        payload: String::from_utf8(payload.to_vec()).ok()?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    const NOW: u64 = 1_790_000_000;

    fn signer() -> Signer {
        Signer::new(ProxyKey::from_bytes([7; 32]))
    }

    #[test]
    fn a_signed_resource_round_trips() {
        let s = signer();
        let token = s
            .sign_resource_at("m-42", "https://img.example.org/a.png?x=1&y=2", NOW)
            .unwrap();
        assert!(!token.contains('/') && !token.contains('+') && !token.contains('='));
        let grant = s.verify_at(&token, Purpose::Resource, NOW + 10).unwrap();
        assert_eq!(grant.message_id, "m-42");
        assert_eq!(
            grant.url.unwrap().as_str(),
            "https://img.example.org/a.png?x=1&y=2"
        );
        assert_eq!(grant.expires, NOW + DEFAULT_RESOURCE_TTL.as_secs());

        let url = s
            .proxy_url("m-42", "https://img.example.org/a.png")
            .unwrap();
        assert!(url.starts_with("nail-proxy:"));
        // The scheme prefix is accepted by `verify`.
        assert!(s.verify(&url, Purpose::Resource).is_ok());
    }

    #[test]
    fn tokens_expire() {
        let s = signer().with_resource_ttl(Duration::from_secs(60));
        let token = s
            .sign_resource_at("m-1", "https://img.example.org/a.png", NOW)
            .unwrap();
        assert!(s.verify_at(&token, Purpose::Resource, NOW + 59).is_ok());
        assert_eq!(
            s.verify_at(&token, Purpose::Resource, NOW + 60),
            Err(TokenError::Expired)
        );
    }

    #[test]
    fn forged_and_tampered_tokens_are_refused() {
        let s = signer();
        let token = s
            .sign_resource_at("m-1", "https://img.example.org/a.png", NOW)
            .unwrap();
        let (body, tag) = token.split_once('.').unwrap();

        // Another key.
        let other = Signer::new(ProxyKey::from_bytes([8; 32]));
        assert_eq!(
            other.verify_at(&token, Purpose::Resource, NOW),
            Err(TokenError::Invalid)
        );

        // The attacker swaps in their own URL but keeps the tag.
        let mut raw = URL_SAFE_NO_PAD.decode(body).unwrap();
        let url_start = raw.len() - "https://img.example.org/a.png".len();
        raw.truncate(url_start);
        raw.extend_from_slice(b"http://127.0.0.1:4780/api/blob/x");
        let swapped = format!("{}.{tag}", URL_SAFE_NO_PAD.encode(&raw));
        assert_eq!(
            s.verify_at(&swapped, Purpose::Resource, NOW),
            Err(TokenError::Invalid)
        );

        // Extending the expiry breaks the tag as well.
        let mut raw = URL_SAFE_NO_PAD.decode(body).unwrap();
        raw[2] = 0xff;
        let extended = format!("{}.{tag}", URL_SAFE_NO_PAD.encode(&raw));
        assert_eq!(
            s.verify_at(&extended, Purpose::Resource, NOW),
            Err(TokenError::Invalid)
        );

        for junk in [
            "",
            ".",
            "abc",
            "abc.def",
            &format!("{body}."),
            &format!("{body}.{}", &tag[..10]),
            &"A".repeat(MAX_TOKEN_LEN + 1),
            "https://img.example.org/a.png",
        ] {
            assert_eq!(
                s.verify_at(junk, Purpose::Resource, NOW),
                Err(TokenError::Invalid),
                "{junk:?}"
            );
        }
    }

    #[test]
    fn purposes_do_not_mix() {
        let s = signer();
        let render = s.sign_render_at("m-1", NOW).unwrap();
        let resource = s
            .sign_resource_at("m-1", "https://img.example.org/a.png", NOW)
            .unwrap();
        assert_eq!(
            s.verify_at(&render, Purpose::Resource, NOW),
            Err(TokenError::WrongPurpose)
        );
        assert_eq!(
            s.verify_at(&resource, Purpose::Render, NOW),
            Err(TokenError::WrongPurpose)
        );
        let grant = s.verify_at(&render, Purpose::Render, NOW).unwrap();
        assert_eq!((grant.message_id.as_str(), grant.url), ("m-1", None));
    }

    #[test]
    fn only_fetchable_urls_are_signed() {
        let s = signer();
        for url in [
            "http://127.0.0.1/",
            "http://localhost/a.png",
            "http://[::1]/",
            "http://10.0.0.1/",
            "file:///C:/Windows/win.ini",
            "cid:part1@example",
            "https://user:pw@img.example.org/",
            "http://img.example.org:25/",
        ] {
            assert!(
                matches!(s.sign_resource("m-1", url), Err(SignError::Url(_))),
                "{url}"
            );
        }
        assert_eq!(
            s.sign_resource("", "https://img.example.org/"),
            Err(SignError::MessageId)
        );
        let long_id = "m".repeat(MAX_MESSAGE_ID_LEN + 1);
        assert_eq!(
            s.sign_resource(&long_id, "https://img.example.org/"),
            Err(SignError::MessageId)
        );
    }

    #[test]
    fn the_rewriter_maps_or_blocks() {
        let s = signer();
        let rewrite = s.rewriter("m-9");
        let mapped = rewrite("https://img.example.org/logo.png").unwrap();
        let grant = s.verify(&mapped, Purpose::Resource).unwrap();
        assert_eq!(grant.message_id, "m-9");
        assert_eq!(rewrite("http://192.168.0.1/track.gif"), None);
        assert_eq!(rewrite("javascript:alert(1)"), None);
    }
}
