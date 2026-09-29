//! Account autoconfiguration for Nail.
//!
//! **Stub (M1a WP10).** Work package WP12 fills this crate in: given an
//! address, query `autoconfig.<domain>`, `<domain>/.well-known/autoconfig`,
//! Mozilla's ISPDB (Thunderbird autoconfig XML, parsed with [`quick_xml`]),
//! RFC 6186 SRV records ([`hickory_resolver`]), Microsoft Autodiscover and
//! common host names, and return ranked
//! [`nail_api::accounts::DiscoveredAccount`]s for the user to confirm.
//!
//! HTTP and DNS are reached through small traits so tests run against the
//! mock HTTP and DNS layers of `nail-testkit-net`, never the network.
//! Expected shape: `docs/adr/0012-m1a-contract.md`.

#![warn(missing_docs)]

/// Name of the crate, for diagnostics.
pub const CRATE: &str = env!("CARGO_PKG_NAME");

#[cfg(test)]
mod tests {
    use quick_xml::Reader;
    use quick_xml::events::Event;

    #[test]
    fn autoconfig_xml_can_be_read() {
        let xml = r#"<clientConfig version="1.1"><emailProvider id="example.org">
            <incomingServer type="imap"><hostname>imap.example.org</hostname><port>993</port></incomingServer>
            </emailProvider></clientConfig>"#;
        let mut reader = Reader::from_str(xml);
        let mut hosts = Vec::new();
        let mut in_host = false;
        loop {
            match reader.read_event().unwrap() {
                Event::Start(e) => in_host = e.name().as_ref() == "hostname",
                Event::Text(t) if in_host => hosts.push(t.xml10_content().into_owned()),
                Event::End(_) => in_host = false,
                Event::Eof => break,
                _ => {}
            }
        }
        assert_eq!(hosts, ["imap.example.org"]);
    }

    #[test]
    fn srv_names_parse() {
        let name =
            hickory_resolver::proto::rr::Name::from_ascii("_imaps._tcp.example.org.").unwrap();
        assert_eq!(name.num_labels(), 4);
        assert_eq!(super::CRATE, "nail-discovery");
    }
}
