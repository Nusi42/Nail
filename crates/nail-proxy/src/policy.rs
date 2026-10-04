//! What the proxy may fetch: URL shape and destination addresses.
//!
//! Two layers, both applied to the first URL **and every redirect**:
//!
//! 1. [`check_url`]: `http` / `https` only, no credentials in the URL, a
//!    host name that is not local by construction (`localhost`, single
//!    labels, `.local`, `.internal`, `.home.arpa`), no port from the
//!    browsers' "bad ports" list (mail, SSH, …), bounded length.
//! 2. [`is_public_ip`]: after DNS resolution every answer must be a public
//!    unicast address. Loopback, private (RFC 1918 / ULA), CGNAT,
//!    link-local, multicast, broadcast, unspecified, documentation,
//!    benchmarking and other special-purpose ranges are refused, and so are
//!    IPv6 forms that embed an IPv4 address of those (IPv4-mapped, NAT64,
//!    6to4) and tunnels whose target cannot be checked (Teredo).

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use ipnet::{Ipv4Net, Ipv6Net};
use url::{Host, Url};

/// Longest URL the proxy signs or fetches.
pub const MAX_URL_LEN: usize = 4096;

/// Why a URL is refused before any network access.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum UrlRefusal {
    /// Not a valid absolute URL, or too long.
    #[error("not a valid URL")]
    Invalid,
    /// Not `http` / `https`.
    #[error("only http and https are fetched")]
    Scheme,
    /// The URL carries a user name or password.
    #[error("URLs with credentials are refused")]
    Credentials,
    /// A local-only host name (`localhost`, a single label, `.local`, …).
    #[error("local host names are refused")]
    LocalName,
    /// A port browsers refuse to fetch from (SMTP, IMAP, SSH, …).
    #[error("port {0} is refused")]
    Port(u16),
    /// A literal IP address that is not public.
    #[error("non-public address {0}")]
    Address(IpAddr),
}

/// Ports the Fetch standard blocks ("bad ports"); a mail image has no
/// business talking to an SMTP, IMAP or SSH server.
const BAD_PORTS: &[u16] = &[
    0, 1, 7, 9, 11, 13, 15, 17, 19, 20, 21, 22, 23, 25, 37, 42, 43, 53, 69, 77, 79, 87, 95, 101,
    102, 103, 104, 109, 110, 111, 113, 115, 117, 119, 123, 135, 137, 139, 143, 161, 179, 389, 427,
    465, 512, 513, 514, 515, 526, 530, 531, 532, 540, 548, 554, 556, 563, 587, 601, 636, 989, 990,
    993, 995, 1719, 1720, 1723, 2049, 3659, 4045, 4190, 5060, 5061, 6000, 6566, 6665, 6666, 6667,
    6668, 6669, 6679, 6697, 10080,
];

/// Name suffixes that only ever resolve inside a local network.
const LOCAL_SUFFIXES: &[&str] = &[
    ".localhost",
    ".local",
    ".localdomain",
    ".internal",
    ".home.arpa",
    ".lan",
    ".intranet",
    ".corp",
];

/// Parses `text` and applies the URL rules (see the module docs).
pub fn parse_target(text: &str) -> Result<Url, UrlRefusal> {
    if text.len() > MAX_URL_LEN {
        return Err(UrlRefusal::Invalid);
    }
    let url = Url::parse(text).map_err(|_| UrlRefusal::Invalid)?;
    check_url(&url)?;
    Ok(url)
}

/// The URL rules (see the module docs) for an already parsed URL.
pub fn check_url(url: &Url) -> Result<(), UrlRefusal> {
    if url.as_str().len() > MAX_URL_LEN {
        return Err(UrlRefusal::Invalid);
    }
    if !matches!(url.scheme(), "http" | "https") {
        return Err(UrlRefusal::Scheme);
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(UrlRefusal::Credentials);
    }
    let port = url.port_or_known_default().ok_or(UrlRefusal::Invalid)?;
    if BAD_PORTS.contains(&port) {
        return Err(UrlRefusal::Port(port));
    }
    match url.host().ok_or(UrlRefusal::Invalid)? {
        Host::Domain(name) => {
            if is_local_name(name) {
                return Err(UrlRefusal::LocalName);
            }
        }
        Host::Ipv4(ip) => check_literal(IpAddr::V4(ip))?,
        Host::Ipv6(ip) => check_literal(IpAddr::V6(ip))?,
    }
    Ok(())
}

fn check_literal(ip: IpAddr) -> Result<(), UrlRefusal> {
    if is_public_ip(ip) {
        Ok(())
    } else {
        Err(UrlRefusal::Address(ip))
    }
}

/// Whether a host name can only be local: `localhost`, a single label
/// (intranet names, search-domain expansion) or a local-only suffix.
pub fn is_local_name(name: &str) -> bool {
    let name = name.trim_end_matches('.').to_ascii_lowercase();
    name.is_empty()
        || name == "localhost"
        || !name.contains('.')
        || LOCAL_SUFFIXES.iter().any(|suffix| name.ends_with(suffix))
}

/// IPv4 ranges that are never fetched.
const BLOCKED_V4: &[(Ipv4Addr, u8)] = &[
    (Ipv4Addr::UNSPECIFIED, 8),           // "this network", unspecified
    (Ipv4Addr::new(10, 0, 0, 0), 8),      // private
    (Ipv4Addr::new(100, 64, 0, 0), 10),   // CGNAT
    (Ipv4Addr::new(127, 0, 0, 0), 8),     // loopback
    (Ipv4Addr::new(169, 254, 0, 0), 16),  // link-local (cloud metadata)
    (Ipv4Addr::new(172, 16, 0, 0), 12),   // private
    (Ipv4Addr::new(192, 0, 0, 0), 24),    // IETF protocol assignments
    (Ipv4Addr::new(192, 0, 2, 0), 24),    // documentation
    (Ipv4Addr::new(192, 31, 196, 0), 24), // AS112
    (Ipv4Addr::new(192, 52, 193, 0), 24), // AMT
    (Ipv4Addr::new(192, 88, 99, 0), 24),  // 6to4 relay anycast
    (Ipv4Addr::new(192, 168, 0, 0), 16),  // private
    (Ipv4Addr::new(192, 175, 48, 0), 24), // AS112
    (Ipv4Addr::new(198, 18, 0, 0), 15),   // benchmarking
    (Ipv4Addr::new(198, 51, 100, 0), 24), // documentation
    (Ipv4Addr::new(203, 0, 113, 0), 24),  // documentation
    (Ipv4Addr::new(224, 0, 0, 0), 4),     // multicast
    (Ipv4Addr::new(240, 0, 0, 0), 4),     // reserved, broadcast
];

/// Whether `ip` is a public unicast address the proxy may connect to.
pub fn is_public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_public_v4(v4),
        IpAddr::V6(v6) => is_public_v6(v6),
    }
}

fn is_public_v4(ip: Ipv4Addr) -> bool {
    !BLOCKED_V4
        .iter()
        .any(|&(net, prefix)| Ipv4Net::new(net, prefix).map_or(true, |net| net.contains(&ip)))
}

fn v6_net(text: &str) -> Ipv6Net {
    text.parse()
        .unwrap_or_else(|_| unreachable!("constant network {text}"))
}

fn is_public_v6(ip: Ipv6Addr) -> bool {
    // IPv4-mapped (::ffff:a.b.c.d): the IPv4 rules decide.
    if let Some(v4) = ip.to_ipv4_mapped() {
        return is_public_v4(v4);
    }
    let segments = ip.segments();
    // NAT64 well-known prefix 64:ff9b::/96 embeds the IPv4 target.
    if v6_net("64:ff9b::/96").contains(&ip) {
        let o = ip.octets();
        return is_public_v4(Ipv4Addr::new(o[12], o[13], o[14], o[15]));
    }
    // 6to4 (2002::/16) embeds the IPv4 relay/target in bits 16..48.
    if segments[0] == 0x2002 {
        let o = ip.octets();
        return is_public_v4(Ipv4Addr::new(o[2], o[3], o[4], o[5]));
    }
    // Only global unicast (2000::/3) is public …
    if !v6_net("2000::/3").contains(&ip) {
        return false; // ::, ::1, IPv4-compatible, fc00::/7, fe80::/10, ff00::/8, …
    }
    // … minus special-purpose blocks inside it.
    let blocked = [
        "2001::/23",     // IETF protocol assignments, incl. Teredo 2001::/32, ORCHID
        "2001:db8::/32", // documentation
        "3fff::/20",     // documentation (RFC 9637)
    ];
    !blocked.iter().any(|net| v6_net(net).contains(&ip))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn public(ip: &str) -> bool {
        is_public_ip(ip.parse().unwrap())
    }

    #[test]
    fn private_and_special_ipv4_ranges_are_blocked() {
        for ip in [
            "0.0.0.0",
            "0.1.2.3",
            "10.0.0.1",
            "10.255.255.255",
            "100.64.0.1",
            "100.127.255.254",
            "127.0.0.1",
            "127.1.2.3",
            "169.254.169.254",
            "172.16.0.1",
            "172.31.255.255",
            "192.0.0.8",
            "192.0.2.1",
            "192.88.99.1",
            "192.168.1.1",
            "198.18.0.1",
            "198.19.255.255",
            "198.51.100.7",
            "203.0.113.9",
            "224.0.0.1",
            "239.255.255.250",
            "240.0.0.1",
            "255.255.255.255",
        ] {
            assert!(!public(ip), "{ip} must be blocked");
        }
    }

    #[test]
    fn public_ipv4_addresses_pass() {
        for ip in [
            "93.184.216.34",
            "1.1.1.1",
            "8.8.8.8",
            "172.15.255.255",
            "172.32.0.0",
            "100.63.255.255",
            "100.128.0.0",
            "192.169.0.1",
        ] {
            assert!(public(ip), "{ip} is public");
        }
    }

    #[test]
    fn ipv6_special_ranges_and_embedded_ipv4_are_checked() {
        for ip in [
            "::",
            "::1",
            "::127.0.0.1",      // IPv4-compatible (deprecated)
            "::ffff:127.0.0.1", // IPv4-mapped loopback
            "::ffff:10.0.0.1",
            "::ffff:169.254.169.254",
            "64:ff9b::127.0.0.1", // NAT64 → loopback
            "64:ff9b::10.1.1.1",
            "64:ff9b:1::1",
            "2002:7f00:1::1",   // 6to4 of 127.0.0.1
            "2002:c0a8:101::1", // 6to4 of 192.168.1.1
            "2001::1",          // Teredo
            "2001:db8::1",
            "3fff::1",
            "fc00::1",
            "fd12:3456::1",
            "fe80::1",
            "fec0::1",
            "ff02::1",
            "100::1",
        ] {
            assert!(!public(ip), "{ip} must be blocked");
        }
        for ip in [
            "2606:2800:220:1::1",
            "2a00:1450:4001:80e::200e",
            "::ffff:93.184.216.34",
            "64:ff9b::93.184.216.34",
            "2002:5db8:d822::1",
        ] {
            assert!(public(ip), "{ip} is public");
        }
    }

    #[test]
    fn url_rules() {
        use UrlRefusal as R;
        let refused = |url: &str| parse_target(url).unwrap_err();
        assert_eq!(refused("ftp://img.example.org/a.png"), R::Scheme);
        assert_eq!(refused("file:///etc/passwd"), R::Scheme);
        assert_eq!(refused("javascript:alert(1)"), R::Scheme);
        assert_eq!(refused("data:image/png;base64,AAAA"), R::Scheme);
        assert_eq!(refused("https://user:pw@img.example.org/"), R::Credentials);
        assert_eq!(refused("https://user@img.example.org/"), R::Credentials);
        assert_eq!(refused("http://localhost/"), R::LocalName);
        assert_eq!(refused("http://LOCALHOST./"), R::LocalName);
        assert_eq!(refused("http://api.localhost/"), R::LocalName);
        assert_eq!(refused("http://printer/"), R::LocalName);
        assert_eq!(refused("http://nas.local/x.png"), R::LocalName);
        assert_eq!(refused("http://router.home.arpa/"), R::LocalName);
        assert_eq!(refused("http://img.example.org:25/"), R::Port(25));
        assert_eq!(refused("http://img.example.org:22/"), R::Port(22));
        assert_eq!(refused("https://img.example.org:993/"), R::Port(993));
        assert_eq!(refused("not a url"), R::Invalid);
        let long = format!("https://img.example.org/{}", "a".repeat(MAX_URL_LEN));
        assert_eq!(refused(&long), R::Invalid);

        assert!(parse_target("https://img.example.org/a.png?x=1").is_ok());
        assert!(parse_target("http://img.example.org:8080/a.png").is_ok());
        assert!(parse_target("http://93.184.216.34/a.png").is_ok());
        assert!(parse_target("http://[2606:2800:220:1::1]/a.png").is_ok());
    }

    #[test]
    fn literal_addresses_in_every_spelling_are_checked() {
        // The URL parser normalizes decimal, hex, octal and short forms the
        // way browsers do, so the check sees the real address.
        for url in [
            "http://127.0.0.1/",
            "http://2130706433/",
            "http://0x7f000001/",
            "http://0177.0.0.1/",
            "http://0x7f.1/",
            "http://127.1/",
            "http://0/",
            "http://[::1]/",
            "http://[::ffff:127.0.0.1]/",
            "http://[::ffff:7f00:1]/",
            "http://[0:0:0:0:0:ffff:a9fe:a9fe]/",
            "http://169.254.169.254/latest/meta-data/",
            "http://[fe80::1]/",
            "http://10.0.0.1:8080/",
            "http://%31%32%37.0.0.1/",
        ] {
            assert!(
                matches!(parse_target(url), Err(UrlRefusal::Address(_))),
                "{url} must be refused as an address, got {:?}",
                parse_target(url)
            );
        }
    }
}
