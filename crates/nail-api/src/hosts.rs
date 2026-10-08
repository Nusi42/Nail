//! Host names and URLs: the one definition of the DNS-name, loopback and
//! server-URL rules that accounts, plug-in permissions, remote access and
//! passkeys share, and of the well-known mail providers' domains that
//! discovery, OAuth2, sending and imports recognize (wave Q2, ADR 0171).

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

/// Gmail's consumer mail domains (addresses `@gmail.com`, servers
/// `imap.gmail.com`, …).
pub const GMAIL_DOMAINS: [&str; 2] = ["gmail.com", "googlemail.com"];

/// Google's own domain (MX hosts `aspmx.l.google.com`, …).
pub const GOOGLE_DOMAIN: &str = "google.com";

/// The domains of Exchange Online (`outlook.office365.com`,
/// `outlook.office.com`, `*.outlook.com`).
pub const EXCHANGE_ONLINE_DOMAINS: [&str; 3] =
    ["office365.com", "outlook.office.com", "outlook.com"];

/// Microsoft's consumer mail domains (Outlook.com: `@hotmail.com`,
/// `@live.com`; their MX hosts are under `outlook.com`).
pub const OUTLOOK_COM_DOMAINS: [&str; 3] = ["outlook.com", "hotmail.com", "live.com"];

/// Microsoft's own domain (Autodiscover answers may point there).
pub const MICROSOFT_DOMAIN: &str = "microsoft.com";

/// `host` without a trailing dot, in lower case (how host names compare).
#[must_use]
pub fn normalize_host(host: &str) -> String {
    host.trim_end_matches('.').to_ascii_lowercase()
}

/// Whether `host` is `domain` or a name below it (`imap.gmail.com` under
/// `gmail.com`; never `notgmail.com`). Both compare without a trailing dot
/// and case.
#[must_use]
pub fn is_under_domain(host: &str, domain: &str) -> bool {
    let (host, domain) = (normalize_host(host), normalize_host(domain));
    host == domain
        || host
            .strip_suffix(domain.as_str())
            .is_some_and(|rest| rest.ends_with('.'))
}

/// Whether `host` is under one of `domains` ([`is_under_domain`]).
#[must_use]
pub fn is_under_any(host: &str, domains: &[&str]) -> bool {
    domains.iter().any(|d| is_under_domain(host, d))
}

/// Whether `domain` (an address's domain) is one of Gmail's own
/// ([`GMAIL_DOMAINS`], exactly).
#[must_use]
pub fn is_gmail_domain(domain: &str) -> bool {
    GMAIL_DOMAINS.contains(&normalize_host(domain).as_str())
}

/// Whether `host` is one of Google's mail hosts (`imap.gmail.com`,
/// `aspmx.l.google.com`, …): Google's OAuth2 sign-in applies, and its
/// servers file the Sent copy themselves.
#[must_use]
pub fn is_google_mail_host(host: &str) -> bool {
    is_under_any(host, &GMAIL_DOMAINS) || is_under_domain(host, GOOGLE_DOMAIN)
}

/// Whether `host` is Exchange Online ([`EXCHANGE_ONLINE_DOMAINS`]).
#[must_use]
pub fn is_exchange_online_host(host: &str) -> bool {
    is_under_any(host, &EXCHANGE_ONLINE_DOMAINS)
}

/// Whether `host` is one of Microsoft's mail hosts: Exchange Online,
/// Outlook.com (servers and MX hosts like
/// `contoso-com.mail.protection.outlook.com`) or Microsoft Graph.
/// Microsoft's OAuth2 sign-in applies, and its servers file the Sent copy
/// themselves.
#[must_use]
pub fn is_microsoft_mail_host(host: &str) -> bool {
    is_exchange_online_host(host)
        || is_under_any(host, &OUTLOOK_COM_DOMAINS)
        || is_under_domain(host, crate::exchange::GRAPH_HOST)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn domains_need_a_label_boundary() {
        assert!(is_under_domain("imap.gmail.com", "gmail.com"));
        assert!(is_under_domain("GMAIL.com.", "gmail.com"));
        assert!(!is_under_domain("notgmail.com", "gmail.com"));
        assert!(!is_under_domain("gmail.com.evil.example", "gmail.com"));
        assert!(!is_under_domain("", "gmail.com"));
        assert!(is_under_any("smtp.office365.com", &EXCHANGE_ONLINE_DOMAINS));
    }

    #[test]
    fn providers() {
        assert!(is_gmail_domain("GoogleMail.com.") && !is_gmail_domain("mail.gmail.com"));
        assert!(
            is_google_mail_host("aspmx.l.google.com.") && is_google_mail_host("imap.gmail.com")
        );
        assert!(!is_google_mail_host("notgmail.com") && !is_google_mail_host("example.org"));
        assert!(is_exchange_online_host("Outlook.Office365.com."));
        assert!(is_exchange_online_host("outlook.office.com"));
        assert!(
            !is_exchange_online_host("notoffice365.com") && !is_exchange_online_host("hotmail.com")
        );
        for host in [
            "outlook.office365.com",
            "smtp-mail.outlook.com",
            "graph.microsoft.com",
            "contoso-com.mail.protection.outlook.com.",
            "eur.olc.protection.outlook.com",
            "hotmail.com",
            "live.com",
        ] {
            assert!(is_microsoft_mail_host(host), "{host}");
        }
        for host in ["notoutlook.com", "mx.example.org", "microsoft.com"] {
            assert!(!is_microsoft_mail_host(host), "{host}");
        }
    }

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
