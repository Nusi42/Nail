//! Smoke tests of the libraries WP42 builds on (WP40, ADR 0036).

#![cfg(test)]

use std::net::IpAddr;

use hmac::{Hmac, KeyInit, Mac};
use ipnet::IpNet;
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

#[test]
fn hmac_sha256_signs_and_verifies() {
    let mut mac = HmacSha256::new_from_slice(b"per-process key").unwrap();
    mac.update(b"m-1\nhttps://img.example/a.png\n1790000000");
    let tag = mac.finalize().into_bytes();
    assert_eq!(tag.len(), 32);

    let mut check = HmacSha256::new_from_slice(b"per-process key").unwrap();
    check.update(b"m-1\nhttps://img.example/a.png\n1790000000");
    assert!(check.verify_slice(&tag).is_ok());
    let mut forged = HmacSha256::new_from_slice(b"per-process key").unwrap();
    forged.update(b"m-1\nhttp://127.0.0.1/\n1790000000");
    assert!(forged.verify_slice(&tag).is_err());
}

#[test]
fn private_ranges_are_recognised() {
    let blocked: Vec<IpNet> = [
        "127.0.0.0/8",
        "10.0.0.0/8",
        "172.16.0.0/12",
        "192.168.0.0/16",
        "169.254.0.0/16",
        "100.64.0.0/10",
        "::1/128",
        "fc00::/7",
        "fe80::/10",
    ]
    .iter()
    .map(|n| n.parse().unwrap())
    .collect();
    let is_blocked = |ip: &str| {
        let ip: IpAddr = ip.parse().unwrap();
        // IPv4-mapped IPv6 addresses are checked as IPv4.
        let ip = match ip {
            IpAddr::V6(v6) => v6.to_ipv4_mapped().map_or(ip, IpAddr::V4),
            v4 @ IpAddr::V4(_) => v4,
        };
        blocked.iter().any(|net| net.contains(&ip))
    };
    for ip in [
        "127.0.0.1",
        "10.1.2.3",
        "192.168.1.20",
        "169.254.169.254",
        "::1",
        "fd00::1",
        "::ffff:127.0.0.1",
    ] {
        assert!(is_blocked(ip), "{ip}");
    }
    for ip in ["93.184.216.34", "2606:2800:220:1::1"] {
        assert!(!is_blocked(ip), "{ip}");
    }
}

#[test]
fn urls_parse_hosts_as_browsers_do() {
    // Decimal and hex IPv4 forms normalize to dotted quads, so checks on
    // the parsed host see the real address.
    let url = url::Url::parse("http://2130706433/").unwrap();
    assert_eq!(url.host_str(), Some("127.0.0.1"));
    let url = url::Url::parse("http://0x7f.1/").unwrap();
    assert_eq!(url.host_str(), Some("127.0.0.1"));
    assert_eq!(nail_api::remote_content::PROXY_URL_SCHEME, "nail-proxy:");
}
