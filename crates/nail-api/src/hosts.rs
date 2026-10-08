//! Host names and URLs: the one definition of the DNS-name, loopback and
//! server-URL rules that accounts, plug-in permissions, remote access and
//! passkeys share (wave Q2, ADR 0171).

/// The standard HTTPS port.
pub const HTTPS_PORT: u16 = 443;

/// The standard HTTP port.
pub const HTTP_PORT: u16 = 80;

/// The standard web port: HTTPS with `tls`, else HTTP.
pub const fn web_port(tls: bool) -> u16 {
    if tls { HTTPS_PORT } else { HTTP_PORT }
}

/// Longest DNS name (RFC 1035, without the trailing dot).
pub const MAX_DNS_NAME_LEN: usize = 253;

/// Longest DNS label (RFC 1035).
pub const MAX_DNS_LABEL_LEN: usize = 63;

/// Whether `label` is an LDH label: 1–63 ASCII letters, digits and `-`, not
/// starting or ending with `-` (either case).
pub fn is_dns_label(label: &str) -> bool {
    !label.is_empty()
        && label.len() <= MAX_DNS_LABEL_LEN
        && !label.starts_with('-')
        && !label.ends_with('-')
        && label
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

/// Whether the last label of a dotted name is all digits (an IPv4 address
/// rather than a host name).
pub fn has_numeric_last_label(name: &str) -> bool {
    name.rsplit('.')
        .next()
        .is_some_and(|l| l.bytes().all(|b| b.is_ascii_digit()))
}

/// A conservative DNS host name check: at most 253 characters of LDH
/// labels ([`is_dns_label`]), the last one not all digits (so IPv4
/// addresses are no host names). No trailing dot.
pub fn is_dns_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_DNS_NAME_LEN
        && name.split('.').all(is_dns_label)
        && !has_numeric_last_label(name)
}

/// Whether `host` is a loopback name or address (`localhost`, `127.0.0.0/8`,
/// `::1`, with or without brackets; test servers).
pub fn is_loopback_host(host: &str) -> bool {
    let host = host.trim_start_matches('[').trim_end_matches(']');
    host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
}

/// The host of an `authority` (`host`, `host:port`, `[v6]:port`): a trailing
/// numeric port is dropped, brackets stay.
pub fn authority_host(authority: &str) -> &str {
    match authority.rsplit_once(':') {
        Some((host, port)) if port.chars().all(|c| c.is_ascii_digit()) => host,
        _ => authority,
    }
}

/// The host of an absolute URL (`scheme://[user@]host[:port]/…`), without
/// the port; an IPv6 literal keeps its brackets. `None` when there is none.
pub fn url_host(url: &str) -> Option<String> {
    let rest = url.split_once("://")?.1;
    let authority = rest.split(['/', '?', '#']).next()?;
    let host = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    let host = if let Some(v6) = host.strip_prefix('[') {
        format!("[{}]", v6.split(']').next()?)
    } else {
        host.split(':').next()?.to_owned()
    };
    (!host.is_empty()).then_some(host)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dns_names() {
        assert!(is_dns_name("mail.example.org") && is_dns_name("Nail.Local"));
        assert!(is_dns_name(&format!("{}.org", "a".repeat(63))));
        assert!(!is_dns_name(&format!("{}.org", "a".repeat(64))));
        assert!(!is_dns_name(&"a.".repeat(127)));
        for bad in [
            "", "-a.org", "a-.org", "a..org", "a_b.org", "10.0.0.1", "::1", "a.org.",
        ] {
            assert!(!is_dns_name(bad), "{bad}");
        }
        assert!(has_numeric_last_label("10.0.0.1") && !has_numeric_last_label("a.b"));
    }

    #[test]
    fn loopback_hosts() {
        for host in [
            "localhost",
            "LOCALHOST",
            "127.0.0.1",
            "127.1.2.3",
            "::1",
            "[::1]",
        ] {
            assert!(is_loopback_host(host), "{host}");
        }
        for host in ["10.0.0.1", "example.org", "[::2]", ""] {
            assert!(!is_loopback_host(host), "{host}");
        }
    }

    #[test]
    fn url_hosts() {
        assert_eq!(
            url_host("https://ex.example/EWS/x").as_deref(),
            Some("ex.example")
        );
        assert_eq!(
            url_host("https://u@ex.example:8443/").as_deref(),
            Some("ex.example")
        );
        assert_eq!(url_host("http://[::1]:8080/a").as_deref(), Some("[::1]"));
        assert_eq!(url_host("https:///path"), None);
        assert_eq!(url_host("ex.example"), None);
    }

    #[test]
    fn authority_hosts() {
        assert_eq!(authority_host("example.org:8080"), "example.org");
        assert_eq!(authority_host("example.org"), "example.org");
        assert_eq!(authority_host("[::1]:80"), "[::1]");
        assert_eq!(authority_host("[::1]"), "[::1]");
        assert_eq!(authority_host("host:abc"), "host:abc");
    }
}
