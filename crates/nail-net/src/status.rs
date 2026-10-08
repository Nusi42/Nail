//! HTTP status codes the clients act on (only those that are not
//! self-explanatory in a `match`: redirects, conflicts, limits, gateway
//! errors), one definition for the API clients, discovery and the OpenPGP
//! key lookup.

/// `301 Moved Permanently`.
pub const MOVED_PERMANENTLY: u16 = 301;
/// `302 Found`.
pub const FOUND: u16 = 302;
/// `303 See Other`.
pub const SEE_OTHER: u16 = 303;
/// `307 Temporary Redirect`.
pub const TEMPORARY_REDIRECT: u16 = 307;
/// `308 Permanent Redirect`.
pub const PERMANENT_REDIRECT: u16 = 308;
/// `409 Conflict`.
pub const CONFLICT: u16 = 409;
/// `410 Gone`.
pub const GONE: u16 = 410;
/// `413 Content Too Large`.
pub const CONTENT_TOO_LARGE: u16 = 413;
/// `429 Too Many Requests`.
pub const TOO_MANY_REQUESTS: u16 = 429;
/// `504 Gateway Timeout`.
pub const GATEWAY_TIMEOUT: u16 = 504;

/// Whether `status` is a redirect a client may follow (`301`, `302`, `303`,
/// `307`, `308`).
#[must_use]
pub fn is_redirect(status: u16) -> bool {
    matches!(
        status,
        MOVED_PERMANENTLY | FOUND | SEE_OTHER | TEMPORARY_REDIRECT | PERMANENT_REDIRECT
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redirect_statuses() {
        for s in [301, 302, 303, 307, 308] {
            assert!(is_redirect(s), "{s}");
        }
        for s in [200, 304, 305, 400] {
            assert!(!is_redirect(s), "{s}");
        }
    }
}
