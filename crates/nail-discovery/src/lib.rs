//! Account autoconfiguration for Nail.
//!
//! [`discover`] takes an email address and returns ranked
//! [`nail_api::accounts::DiscoveredAccount`]s for the user to confirm, from:
//!
//! * `autoconfig.<domain>` and `<domain>/.well-known/autoconfig`
//!   (Thunderbird `clientConfig` XML, parsed with [`quick_xml`]);
//! * Mozilla's ISPDB, by domain and by the domain of the MX host;
//! * RFC 6186 SRV records (`_imaps`, `_imap`, `_pop3s`, `_pop3`,
//!   `_submissions`, `_submission`) via [`hickory_resolver`];
//! * Microsoft Autodiscover (POX, with `redirectAddr` / `redirectUrl`);
//! * common host names (`imap.<domain>`, `mail.<domain>`, …) when nothing
//!   else answered.
//!
//! HTTP, DNS and TCP reachability go through the [`HttpFetch`],
//! [`DnsLookup`] and [`HostProbe`] traits held by a [`Discoverer`]; the real
//! implementations can be pointed at the mock servers of
//! `nail-testkit-net` ([`ReqwestFetch::route_all_to`],
//! [`HickoryDns::with_name_server`], [`TcpProbe::with_overrides`]), so tests
//! never touch the network. See
//! `docs/adr/0017-wp12-outgoing-pop3-auth-discovery.md`.

#![warn(missing_docs)]

mod autoconfig;
mod autodiscover;
mod discover;
mod email;
mod net;
mod xml;

pub use discover::{
    Discoverer, DiscoveryError, ISPDB_BASE, discover, flag_foreign_hosts, protocols, rank,
};
pub use net::{
    DnsLookup, HickoryDns, HostProbe, HttpFetch, HttpReply, MxRecord, NetError, ReqwestFetch,
    SrvRecord, TcpProbe,
};
