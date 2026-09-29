//! Cookies, the resolved session of a request, and small helpers.
//!
//! Both cookies are `HttpOnly; Secure; SameSite=Strict; Path=/` with the
//! `__Host-` prefix (no `Domain`), so scripts cannot read them, they never
//! travel over plain HTTP (browsers treat `http://localhost` as secure) or
//! cross-site, and they are pinned to the exact origin.
//!
//! * [`SESSION_COOKIE`]: a random token, rotated while in use, living at
//!   most until the session's absolute expiry.
//! * [`DEVICE_COOKIE`]: the paired browser's random secret. A session is
//!   only valid together with the device cookie of the device it was
//!   started on, so a stolen session cookie alone is useless.

use std::net::IpAddr;

use axum::http::HeaderMap;
use axum::http::header::COOKIE;
use cookie::{Cookie, SameSite};
use nail_api::remote::{DEVICE_COOKIE, RemoteDevice, RemoteGrant, RemoteSession, SESSION_COOKIE};
use zeroize::Zeroizing;

use crate::error::{AuthError, Result};

/// Longest name of a device or passkey (characters).
pub const MAX_LABEL_CHARS: usize = 64;

/// A device or passkey name: trimmed, without control characters, at most
/// [`MAX_LABEL_CHARS`] characters.
pub fn clean_label(name: &str) -> Result<String> {
    let name = name.trim();
    if name.is_empty() || name.chars().any(char::is_control) {
        return Err(AuthError::Invalid("a name must be visible text".into()));
    }
    if name.chars().count() > MAX_LABEL_CHARS {
        return Err(AuthError::Invalid(format!(
            "a name has at most {MAX_LABEL_CHARS} characters"
        )));
    }
    Ok(name.to_owned())
}

/// Who sent a request.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ClientMeta {
    /// The peer address.
    pub ip: Option<IpAddr>,
    /// The `User-Agent` header.
    pub user_agent: Option<String>,
}

impl ClientMeta {
    /// The address as stored in audit entries and sessions.
    pub fn ip_string(&self) -> Option<String> {
        self.ip.map(|ip| match ip {
            IpAddr::V6(v6) => v6
                .to_ipv4_mapped()
                .map_or_else(|| v6.to_string(), |v4| v4.to_string()),
            IpAddr::V4(v4) => v4.to_string(),
        })
    }

    /// The user agent, shortened for storage.
    pub fn user_agent(&self) -> Option<String> {
        self.user_agent
            .as_deref()
            .map(|ua| ua.chars().filter(|c| !c.is_control()).take(256).collect())
    }
}

/// The two cookies of a request (values zeroized on drop).
#[derive(Debug, Default)]
pub struct Cookies {
    /// The session token.
    pub session: Option<Zeroizing<String>>,
    /// The device secret.
    pub device: Option<Zeroizing<String>>,
}

impl Cookies {
    /// Reads the cookies from `Cookie` headers. A cookie sent twice (an
    /// attempt to shadow ours) makes that cookie count as absent.
    pub fn from_headers(headers: &HeaderMap) -> Self {
        let mut session: Vec<Zeroizing<String>> = Vec::new();
        let mut device: Vec<Zeroizing<String>> = Vec::new();
        for header in headers.get_all(COOKIE) {
            let Ok(text) = header.to_str() else { continue };
            for cookie in Cookie::split_parse(text).flatten() {
                match cookie.name() {
                    SESSION_COOKIE => session.push(Zeroizing::new(cookie.value().to_owned())),
                    DEVICE_COOKIE => device.push(Zeroizing::new(cookie.value().to_owned())),
                    _ => {}
                }
            }
        }
        let single =
            |mut values: Vec<Zeroizing<String>>| (values.len() == 1).then(|| values.remove(0));
        Self {
            session: single(session),
            device: single(device),
        }
    }
}

fn build(name: &'static str, value: &str, max_age_secs: i64) -> String {
    Cookie::build((name, value.to_owned()))
        .http_only(true)
        .secure(true)
        .same_site(SameSite::Strict)
        .path("/")
        .max_age(cookie::time::Duration::seconds(max_age_secs.max(0)))
        .build()
        .to_string()
}

/// `Set-Cookie` value of a session token living `max_age_secs`.
pub fn session_cookie(token: &str, max_age_secs: i64) -> String {
    build(SESSION_COOKIE, token, max_age_secs)
}

/// `Set-Cookie` value of a device secret living `max_age_secs`.
pub fn device_cookie(secret: &str, max_age_secs: i64) -> String {
    build(DEVICE_COOKIE, secret, max_age_secs)
}

/// `Set-Cookie` value that removes a cookie.
pub fn clear_cookie(name: &'static str) -> String {
    build(name, "", 0)
}

/// An authenticated remote session, as attached to a request by the
/// middleware ([`crate::http::require_session`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteSessionCtx {
    /// The session.
    pub session: RemoteSession,
    /// Its device.
    pub device: RemoteDevice,
    /// Who sent the request.
    pub client: ClientMeta,
    /// Until when the session counts as freshly authenticated (Unix
    /// seconds): the later of its login and its last re-authentication,
    /// plus `reauth_minutes`.
    pub reauth_until: i64,
}

impl RemoteSessionCtx {
    /// What the session may do.
    pub fn grant(&self) -> &RemoteGrant {
        &self.session.grant
    }

    /// Whether the session authenticated recently enough for a sensitive
    /// action at `now`.
    pub fn is_fresh(&self, now: i64) -> bool {
        now < self.reauth_until
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    #[test]
    fn labels() {
        assert_eq!(clean_label("  Phone ").unwrap(), "Phone");
        assert!(clean_label("   ").is_err());
        assert!(clean_label("a\u{7}b").is_err());
        assert!(clean_label(&"x".repeat(65)).is_err());
        assert_eq!(clean_label(&"ü".repeat(64)).unwrap().chars().count(), 64);
    }

    #[test]
    fn cookie_attributes() {
        let cookie = session_cookie("tok", 3600);
        for part in [
            "__Host-nail-session=tok",
            "HttpOnly",
            "Secure",
            "SameSite=Strict",
            "Path=/",
            "Max-Age=3600",
        ] {
            assert!(cookie.contains(part), "{cookie}");
        }
        assert!(!cookie.contains("Domain"), "{cookie}");
        let device = device_cookie("sec", 10);
        assert!(device.starts_with("__Host-nail-device=sec;"), "{device}");
        let cleared = clear_cookie(SESSION_COOKIE);
        assert!(cleared.contains("Max-Age=0"), "{cleared}");
        assert!(session_cookie("t", -5).contains("Max-Age=0"));
    }

    #[test]
    fn cookies_from_headers() {
        let mut headers = HeaderMap::new();
        headers.append(
            COOKIE,
            HeaderValue::from_static("a=b; __Host-nail-session=s1; __Host-nail-device=d1"),
        );
        let cookies = Cookies::from_headers(&headers);
        assert_eq!(cookies.session.as_deref().map(String::as_str), Some("s1"));
        assert_eq!(cookies.device.as_deref().map(String::as_str), Some("d1"));

        // A second session cookie (e.g. planted by a sibling path) voids it.
        headers.append(COOKIE, HeaderValue::from_static("__Host-nail-session=s2"));
        let cookies = Cookies::from_headers(&headers);
        assert!(cookies.session.is_none());
        assert!(cookies.device.is_some());
        assert!(Cookies::from_headers(&HeaderMap::new()).session.is_none());
    }

    #[test]
    fn client_meta() {
        let meta = ClientMeta {
            ip: Some("::ffff:10.0.0.9".parse().unwrap()),
            user_agent: Some(format!("UA\n{}", "x".repeat(400))),
        };
        assert_eq!(meta.ip_string().as_deref(), Some("10.0.0.9"));
        let ua = meta.user_agent().unwrap();
        assert_eq!(ua.chars().count(), 256);
        assert!(!ua.contains('\n'));
        assert_eq!(ClientMeta::default().ip_string(), None);
    }
}
