//! Security response headers of the app origin.
//!
//! Every response (static files, API, errors) gets:
//!
//! * a strict Content Security Policy: scripts only from the bundle (no
//!   inline script, no `eval`), no plug-ins, no `<base>`, no form posts,
//!   never framed (`frame-ancestors 'none'`); frames only from the app and
//!   the sandbox origin. Inline *styles* stay allowed: the compiled theme
//!   is a generated `<style>` element;
//! * Trusted Types (`require-trusted-types-for 'script'`, policies `nail`
//!   and `nail-reader` only): enforced in the [`HeaderProfile::Strict`]
//!   profile (the remote UI), reported only in [`HeaderProfile::Compat`]
//!   (loopback `nail serve`, until the reader has moved to the sandbox
//!   origin);
//! * `Cross-Origin-Opener-Policy: same-origin`,
//!   `Cross-Origin-Embedder-Policy: require-corp`,
//!   `Cross-Origin-Resource-Policy: same-origin` (unless a route set its
//!   own), `X-Frame-Options: DENY`, `X-Content-Type-Options: nosniff`,
//!   `Referrer-Policy: no-referrer`, `Origin-Agent-Cluster`, a restrictive
//!   `Permissions-Policy`;
//! * with TLS: `Strict-Transport-Security` and `upgrade-insecure-requests`.

use std::sync::Arc;

use axum::extract::{Request, State};
use axum::http::{HeaderName, HeaderValue, header};
use axum::middleware::Next;
use axum::response::Response;

/// Which header profile a server uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum HeaderProfile {
    /// Loopback `nail serve` today: Trusted Types report-only.
    #[default]
    Compat,
    /// The remote UI: Trusted Types enforced. Always used for
    /// non-loopback addresses.
    Strict,
}

/// Trusted Types directives (policy names the UI may create).
pub const TRUSTED_TYPES: &str =
    "require-trusted-types-for 'script'; trusted-types nail nail-reader";

/// HSTS value: one year; no `includeSubDomains` (a LAN host name may share
/// its parent with other devices).
pub const HSTS: &str = "max-age=31536000";

/// Features the app never uses.
pub const PERMISSIONS_POLICY: &str = "camera=(), microphone=(), geolocation=(), payment=(), \
     usb=(), serial=(), hid=(), bluetooth=(), midi=(), display-capture=(), \
     publickey-credentials-get=(self), publickey-credentials-create=(self)";

/// The app's Content Security Policy. `sandbox_origins` may frame mail;
/// `strict` enforces Trusted Types; `tls` upgrades mixed content.
pub fn app_csp(sandbox_origins: &[String], strict: bool, tls: bool) -> String {
    let mut frame_src = "'self' blob: data:".to_owned();
    for origin in sandbox_origins {
        frame_src.push(' ');
        frame_src.push_str(origin);
    }
    let mut csp = format!(
        "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; \
         img-src 'self' data: blob:; font-src 'self' data:; connect-src 'self'; \
         frame-src {frame_src}; worker-src 'self'; manifest-src 'self'; media-src 'self' blob:; \
         object-src 'none'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'"
    );
    if strict {
        csp.push_str("; ");
        csp.push_str(TRUSTED_TYPES);
    }
    if tls {
        csp.push_str("; upgrade-insecure-requests");
    }
    csp
}

/// The headers one router adds.
#[derive(Debug, Clone)]
pub(crate) struct HeaderSet {
    /// Always set (replacing a handler's value).
    overriding: Vec<(HeaderName, HeaderValue)>,
    /// Set unless the handler already did.
    defaults: Vec<(HeaderName, HeaderValue)>,
}

fn value(text: &str) -> HeaderValue {
    HeaderValue::from_str(text).unwrap_or_else(|_| HeaderValue::from_static(""))
}

impl HeaderSet {
    /// The app origin's headers.
    pub(crate) fn app(profile: HeaderProfile, tls: bool, sandbox_origins: &[String]) -> Self {
        let strict = profile == HeaderProfile::Strict;
        let mut overriding = common(tls);
        overriding.push((header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY")));
        overriding.push((
            HeaderName::from_static("permissions-policy"),
            HeaderValue::from_static(PERMISSIONS_POLICY),
        ));
        let mut defaults = vec![
            (
                header::CONTENT_SECURITY_POLICY,
                value(&app_csp(sandbox_origins, strict, tls)),
            ),
            (
                HeaderName::from_static("cross-origin-resource-policy"),
                HeaderValue::from_static("same-origin"),
            ),
        ];
        if !strict {
            defaults.push((
                header::CONTENT_SECURITY_POLICY_REPORT_ONLY,
                HeaderValue::from_static(TRUSTED_TYPES),
            ));
        }
        Self {
            overriding,
            defaults,
        }
    }

    /// The sandbox origin's baseline (its routes set CSP and CORP).
    pub(crate) fn sandbox(tls: bool) -> Self {
        Self {
            overriding: common(tls),
            defaults: vec![
                (
                    header::CONTENT_SECURITY_POLICY,
                    HeaderValue::from_static("default-src 'none'; sandbox"),
                ),
                (
                    HeaderName::from_static("cross-origin-resource-policy"),
                    HeaderValue::from_static("same-origin"),
                ),
                (header::CACHE_CONTROL, HeaderValue::from_static("no-store")),
            ],
        }
    }
}

/// Headers both origins always send.
fn common(tls: bool) -> Vec<(HeaderName, HeaderValue)> {
    let mut headers = vec![
        (
            header::X_CONTENT_TYPE_OPTIONS,
            HeaderValue::from_static("nosniff"),
        ),
        (
            header::REFERRER_POLICY,
            HeaderValue::from_static("no-referrer"),
        ),
        (
            HeaderName::from_static("cross-origin-opener-policy"),
            HeaderValue::from_static("same-origin"),
        ),
        (
            HeaderName::from_static("cross-origin-embedder-policy"),
            HeaderValue::from_static("require-corp"),
        ),
        (
            HeaderName::from_static("origin-agent-cluster"),
            HeaderValue::from_static("?1"),
        ),
    ];
    if tls {
        headers.push((
            header::STRICT_TRANSPORT_SECURITY,
            HeaderValue::from_static(HSTS),
        ));
    }
    headers
}

/// Middleware adding a [`HeaderSet`].
pub(crate) async fn apply(
    State(set): State<Arc<HeaderSet>>,
    request: Request,
    next: Next,
) -> Response {
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    for (name, value) in &set.overriding {
        headers.insert(name.clone(), value.clone());
    }
    for (name, value) in &set.defaults {
        if !headers.contains_key(name) {
            headers.insert(name.clone(), value.clone());
        }
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_csp_has_no_inline_script_and_is_never_framed() {
        for strict in [false, true] {
            let csp = app_csp(&["https://nail.example:4781".to_owned()], strict, true);
            assert!(csp.contains("script-src 'self';"));
            assert!(!csp.contains("unsafe-eval"));
            let script = csp
                .split(';')
                .find(|d| d.trim().starts_with("script-src"))
                .unwrap();
            assert!(!script.contains("unsafe-inline"));
            assert!(csp.contains("frame-ancestors 'none'"));
            assert!(csp.contains("object-src 'none'"));
            assert!(csp.contains("base-uri 'none'"));
            assert!(csp.contains("frame-src 'self' blob: data: https://nail.example:4781;"));
            assert!(csp.ends_with("upgrade-insecure-requests"));
            assert_eq!(csp.contains("require-trusted-types-for 'script'"), strict);
        }
        assert!(!app_csp(&[], false, false).contains("upgrade-insecure-requests"));
    }

    #[test]
    fn hsts_only_with_tls() {
        let has_hsts = |set: &HeaderSet| {
            set.overriding
                .iter()
                .any(|(n, _)| n == header::STRICT_TRANSPORT_SECURITY)
        };
        assert!(has_hsts(&HeaderSet::app(HeaderProfile::Strict, true, &[])));
        assert!(!has_hsts(&HeaderSet::app(
            HeaderProfile::Strict,
            false,
            &[]
        )));
        assert!(has_hsts(&HeaderSet::sandbox(true)));
        assert!(!has_hsts(&HeaderSet::sandbox(false)));
    }

    #[test]
    fn trusted_types_are_reported_in_compat_mode() {
        let compat = HeaderSet::app(HeaderProfile::Compat, false, &[]);
        assert!(
            compat.defaults.iter().any(
                |(n, v)| n == header::CONTENT_SECURITY_POLICY_REPORT_ONLY && v == TRUSTED_TYPES
            )
        );
        let strict = HeaderSet::app(HeaderProfile::Strict, false, &[]);
        assert!(
            !strict
                .defaults
                .iter()
                .any(|(n, _)| n == header::CONTENT_SECURITY_POLICY_REPORT_ONLY)
        );
    }
}
