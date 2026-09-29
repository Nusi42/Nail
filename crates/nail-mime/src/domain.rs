//! Domain helpers shared by `auth_results` (DMARC alignment) and `links`
//! (display vs target comparison).
//!
//! Nail ships no Public Suffix List, so [`org_domain`] approximates the
//! organizational domain: the last two labels, or the last three when the
//! second-level label is a well-known registry label under a two-letter
//! country code (`example.co.uk`, `example.com.au`). Known limitation:
//! private suffixes such as `github.io` are treated as organizations, so
//! `a.github.io` and `b.github.io` count as aligned (DMARC) and a link
//! between them is not flagged. The receiving server's own DMARC verdict is
//! unaffected; `aligned` is informational.

/// Second-level labels that registries use under country codes.
const REGISTRY_LABELS: &[&str] = &[
    "ac", "co", "com", "edu", "gov", "govt", "gv", "ltd", "me", "mil", "ne", "net", "nic", "nom",
    "or", "org", "plc", "sch",
];

/// Lowercases, trims a trailing dot and converts to ASCII (punycode).
/// `None` for something that is not a host name.
pub(crate) fn normalize_host(host: &str) -> Option<String> {
    let host = host.trim().trim_end_matches('.');
    if host.is_empty() || host.contains(|c: char| c.is_whitespace() || c == '/' || c == '@') {
        return None;
    }
    idna::domain_to_ascii(host)
        .ok()
        .map(|h| h.to_ascii_lowercase())
        .filter(|h| !h.is_empty())
}

/// The organizational domain (see the module docs).
pub(crate) fn org_domain(host: &str) -> String {
    let host = normalize_host(host).unwrap_or_else(|| host.to_ascii_lowercase());
    let labels: Vec<&str> = host.split('.').filter(|l| !l.is_empty()).collect();
    let n = labels.len();
    if n <= 2 {
        return labels.join(".");
    }
    let tld = labels[n - 1];
    let second = labels[n - 2];
    let keep = if tld.len() == 2 && REGISTRY_LABELS.contains(&second) {
        3
    } else {
        2
    };
    labels[n - keep..].join(".")
}

/// Whether two hosts share the organizational domain (DMARC relaxed
/// alignment).
pub(crate) fn aligned(a: &str, b: &str) -> bool {
    let (a, b) = (org_domain(a), org_domain(b));
    !a.is_empty() && a == b
}

/// The domain of an address or a bare domain (`user@example.org`,
/// `@example.org`, `example.org`).
pub(crate) fn domain_of(value: &str) -> Option<String> {
    let value = value.trim().trim_matches(|c| c == '<' || c == '>');
    let domain = value.rsplit_once('@').map_or(value, |(_, d)| d);
    normalize_host(domain)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn organizational_domains() {
        assert_eq!(org_domain("mail.example.org"), "example.org");
        assert_eq!(org_domain("Example.ORG."), "example.org");
        assert_eq!(org_domain("a.b.example.co.uk"), "example.co.uk");
        assert_eq!(org_domain("shop.example.com.au"), "example.com.au");
        assert_eq!(org_domain("example.de"), "example.de");
        assert_eq!(org_domain("localhost"), "localhost");
        // Unknown suffixes: two labels.
        assert_eq!(org_domain("x.example.github.io"), "github.io");
    }

    #[test]
    fn alignment_and_domains() {
        assert!(aligned("bounce.example.org", "example.org"));
        assert!(!aligned("example.org", "example.net"));
        assert!(!aligned("", ""));
        assert_eq!(domain_of("Ada@Example.org").as_deref(), Some("example.org"));
        assert_eq!(domain_of("@example.org").as_deref(), Some("example.org"));
        assert_eq!(
            domain_of("<x@mail.example.org>").as_deref(),
            Some("mail.example.org")
        );
        assert_eq!(domain_of("example.org").as_deref(), Some("example.org"));
        assert_eq!(
            normalize_host("münchen.de").as_deref(),
            Some("xn--mnchen-3ya.de")
        );
        assert_eq!(normalize_host("a b"), None);
        assert_eq!(domain_of(""), None);
    }
}
