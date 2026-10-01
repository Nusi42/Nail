//! This machine's LAN address, for the host names of a remote UI that
//! listens on every address (`0.0.0.0` / `::`).
//!
//! Read from the network interfaces (`GetAdaptersAddresses` through the
//! `ipconfig` crate on Windows): no socket is opened. The earlier "connect
//! a UDP socket to a documentation address and read its source address"
//! trick bound `0.0.0.0:0`, which is enough for Windows Defender Firewall
//! to prompt (ADR 0057). Other platforms report no LAN address for now
//! (the computer's name is still offered).

use std::net::IpAddr;

/// What the choice needs to know about one network interface.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Interface {
    /// Operationally up.
    pub up: bool,
    /// The software loopback interface.
    pub loopback: bool,
    /// Unicast addresses.
    pub addresses: Vec<IpAddr>,
    /// Default gateways.
    pub gateways: Vec<IpAddr>,
    /// Route metric for IPv4 (lower is preferred).
    pub metric_v4: u32,
    /// Route metric for IPv6 (lower is preferred).
    pub metric_v6: u32,
}

/// The LAN address of the family of `bind` (IPv4 for `0.0.0.0`, IPv6 for
/// `::`), or `None`.
pub(crate) fn lan_address(bind: IpAddr) -> Option<IpAddr> {
    choose(&interfaces(), bind.is_ipv4())
}

/// The address the default route would use: an up, non-loopback interface
/// with a gateway of the family (lowest metric first), else any up
/// interface with a usable address. Link-local, loopback, unspecified and
/// multicast addresses are never chosen.
pub(crate) fn choose(interfaces: &[Interface], v4: bool) -> Option<IpAddr> {
    let family = |ip: &IpAddr| ip.is_ipv4() == v4;
    let mut candidates: Vec<(bool, u32, IpAddr)> = interfaces
        .iter()
        .filter(|i| i.up && !i.loopback)
        .flat_map(|i| {
            let routed = i.gateways.iter().any(|g| family(g) && !g.is_unspecified());
            let metric = if v4 { i.metric_v4 } else { i.metric_v6 };
            i.addresses
                .iter()
                .filter(|ip| family(ip) && usable(ip))
                .map(move |ip| (!routed, metric, *ip))
        })
        .collect();
    // Stable: equal interfaces keep the system's order.
    candidates.sort_by_key(|(unrouted, metric, _)| (*unrouted, *metric));
    candidates.first().map(|(_, _, ip)| *ip)
}

fn usable(ip: &IpAddr) -> bool {
    if ip.is_loopback() || ip.is_unspecified() || ip.is_multicast() {
        return false;
    }
    match ip {
        IpAddr::V4(v4) => !v4.is_link_local() && !v4.is_broadcast(),
        IpAddr::V6(v6) => !v6.is_unicast_link_local(),
    }
}

#[cfg(windows)]
fn interfaces() -> Vec<Interface> {
    let adapters = match ipconfig::get_adapters() {
        Ok(adapters) => adapters,
        Err(err) => {
            tracing::debug!(%err, "listing the network interfaces failed");
            return Vec::new();
        }
    };
    adapters
        .iter()
        .map(|a| Interface {
            up: a.oper_status() == ipconfig::OperStatus::IfOperStatusUp,
            loopback: a.if_type() == ipconfig::IfType::SoftwareLoopback,
            addresses: a.ip_addresses().to_vec(),
            gateways: a.gateways().to_vec(),
            metric_v4: a.ipv4_metric(),
            metric_v6: a.ipv6_metric(),
        })
        .collect()
}

#[cfg(not(windows))]
fn interfaces() -> Vec<Interface> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(text: &str) -> IpAddr {
        text.parse().unwrap()
    }

    fn up(addresses: &[&str], gateways: &[&str], metric: u32) -> Interface {
        Interface {
            up: true,
            loopback: false,
            addresses: addresses.iter().map(|a| ip(a)).collect(),
            gateways: gateways.iter().map(|g| ip(g)).collect(),
            metric_v4: metric,
            metric_v6: metric,
        }
    }

    #[test]
    fn prefers_the_routed_interface_with_the_lowest_metric() {
        let interfaces = [
            Interface {
                loopback: true,
                ..up(&["127.0.0.1", "::1"], &[], 1)
            },
            // A virtual switch without a gateway (Hyper-V, WSL).
            up(&["172.20.112.1", "fe80::1"], &[], 5),
            Interface {
                up: false,
                ..up(&["10.0.0.7"], &["10.0.0.1"], 1)
            },
            // VPN with a worse metric than the Wi-Fi.
            up(&["10.8.0.2"], &["10.8.0.1"], 50),
            up(
                &["169.254.10.10", "192.168.1.23", "fe80::2", "2001:db8::23"],
                &["192.168.1.1", "fe80::1"],
                25,
            ),
        ];
        assert_eq!(choose(&interfaces, true), Some(ip("192.168.1.23")));
        assert_eq!(choose(&interfaces, false), Some(ip("2001:db8::23")));
    }

    #[test]
    fn falls_back_to_an_unrouted_interface_and_never_to_loopback() {
        let unrouted = [up(&["169.254.3.4", "172.20.112.1"], &[], 5)];
        assert_eq!(choose(&unrouted, true), Some(ip("172.20.112.1")));
        assert_eq!(choose(&unrouted, false), None);
        let only_loopback = [Interface {
            loopback: true,
            ..up(&["127.0.0.1"], &[], 1)
        }];
        assert_eq!(choose(&only_loopback, true), None);
        assert_eq!(choose(&[], true), None);
    }

    #[test]
    fn the_real_interfaces_give_a_lan_address_or_none() {
        let interfaces = interfaces();
        for v4 in [true, false] {
            if let Some(lan) = choose(&interfaces, v4) {
                assert_eq!(lan.is_ipv4(), v4);
                assert!(usable(&lan), "{lan}");
            }
        }
    }
}
