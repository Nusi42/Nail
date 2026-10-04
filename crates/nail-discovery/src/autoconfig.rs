//! Thunderbird autoconfig documents (`clientConfig` v1.1), as served by
//! `autoconfig.<domain>`, `<domain>/.well-known/autoconfig` and Mozilla's
//! ISPDB.
//!
//! Format: <https://wiki.mozilla.org/Thunderbird:Autoconfiguration:ConfigFileFormat>.

use nail_api::accounts::{
    AuthConfig, DiscoveredAccount, DiscoverySource, ImapOptions, IncomingServer, MailProtocol,
    OutgoingServer, Pop3Options, ServerEndpoint, SmtpOptions, TlsMode, default_port,
};

use crate::email::Email;
use crate::xml::{Element, parse};

/// Replaces the autoconfig placeholders.
pub(crate) fn expand(template: &str, email: &Email) -> String {
    template
        .replace("%EMAILADDRESS%", &email.address)
        .replace("%EMAILLOCALPART%", &email.local)
        .replace("%EMAILDOMAIN%", &email.domain)
}

/// The auth method for a server from its ordered `<authentication>` list:
/// the first one Nail supports.
fn auth(server: &Element, host: &str, username: &str) -> Option<AuthConfig> {
    let methods: Vec<String> = server
        .children_named("authentication")
        .map(|a| a.text.trim().to_owned())
        .collect();
    let password = || AuthConfig::Password {
        username: username.to_owned(),
        secret: None,
        mechanism: None,
    };
    if methods.is_empty() {
        return Some(password());
    }
    methods
        .iter()
        .find_map(|method| match method.to_ascii_lowercase().as_str() {
            "oauth2" => nail_auth::provider_for_host(host).map(|provider| AuthConfig::OAuth2 {
                username: username.to_owned(),
                provider: provider.to_owned(),
                secret: None,
                mechanism: None,
            }),
            // password-encrypted means CRAM-MD5 / DIGEST-MD5; Nail sends the
            // password over TLS with PLAIN / LOGIN instead.
            "password-cleartext" | "plain" | "password-encrypted" | "secure" => Some(password()),
            "none" | "client-ip-address" | "smtp-after-pop" => Some(AuthConfig::None),
            "tls-client-cert" => Some(AuthConfig::ClientCertificate {
                username: Some(username.to_owned()),
                secret: None,
            }),
            _ => None,
        })
}

fn tls_mode(socket_type: Option<&str>) -> TlsMode {
    match socket_type.map(str::to_ascii_uppercase).as_deref() {
        Some("STARTTLS") => TlsMode::Starttls,
        Some("PLAIN") => TlsMode::Plaintext,
        _ => TlsMode::Implicit,
    }
}

/// Builds the endpoint of an `incomingServer` / `outgoingServer` element.
fn endpoint(server: &Element, protocol: MailProtocol, email: &Email) -> Option<ServerEndpoint> {
    let host = expand(&server.text_of("hostname")?, email).to_ascii_lowercase();
    if host.is_empty() || host.contains(char::is_whitespace) {
        return None;
    }
    let tls = tls_mode(server.text_of("socketType").as_deref());
    let port = server
        .text_of("port")
        .and_then(|p| p.parse::<u16>().ok())
        .filter(|p| *p != 0)
        .unwrap_or_else(|| default_port(protocol, tls));
    let username = server
        .text_of("username")
        .map_or_else(|| email.address.clone(), |u| expand(&u, email));
    let auth = auth(server, &host, &username)?;
    Some(ServerEndpoint {
        host,
        port,
        tls,
        auth,
    })
}

/// Parses a `clientConfig` document into candidates for `email`: one per
/// usable incoming server, in document order, each with the first usable
/// outgoing server.
pub(crate) fn parse_client_config(
    xml: &[u8],
    email: &Email,
    source: DiscoverySource,
) -> Result<Vec<DiscoveredAccount>, String> {
    let root = parse(xml)?;
    if !root.name.eq_ignore_ascii_case("clientConfig") {
        return Err(format!("unexpected root element <{}>", root.name));
    }
    let Some(provider) = root.child("emailProvider") else {
        return Err("no emailProvider".to_owned());
    };
    let provider_name = provider
        .text_of("displayName")
        .or_else(|| provider.text_of("displayShortName"));
    let outgoing = provider
        .children_named("outgoingServer")
        .filter(|s| {
            s.attr("type")
                .is_some_and(|t| t.eq_ignore_ascii_case("smtp"))
        })
        .find_map(|s| endpoint(s, MailProtocol::Smtp, email))
        .map(|server| OutgoingServer::Smtp {
            server,
            options: SmtpOptions::default(),
        });
    let accounts = provider
        .children_named("incomingServer")
        .filter_map(|s| {
            let incoming = match s.attr("type").map(str::to_ascii_lowercase).as_deref() {
                Some("imap") => IncomingServer::Imap {
                    server: endpoint(s, MailProtocol::Imap, email)?,
                    options: ImapOptions::default(),
                },
                Some("pop3") => IncomingServer::Pop3 {
                    server: endpoint(s, MailProtocol::Pop3, email)?,
                    options: Pop3Options::default(),
                },
                _ => return None,
            };
            Some(DiscoveredAccount {
                source,
                incoming,
                outgoing: outgoing.clone(),
                provider_name: provider_name.clone(),
                warnings: Vec::new(),
            })
        })
        .collect();
    Ok(accounts)
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXAMPLE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<clientConfig version="1.1">
  <emailProvider id="example.org">
    <domain>example.org</domain>
    <displayName>Example Mail</displayName>
    <incomingServer type="imap">
      <hostname>imap.%EMAILDOMAIN%</hostname>
      <port>993</port>
      <socketType>SSL</socketType>
      <username>%EMAILLOCALPART%</username>
      <authentication>password-cleartext</authentication>
    </incomingServer>
    <incomingServer type="pop3">
      <hostname>pop.example.org</hostname>
      <port>110</port>
      <socketType>STARTTLS</socketType>
      <username>%EMAILADDRESS%</username>
      <authentication>password-encrypted</authentication>
    </incomingServer>
    <incomingServer type="exchange">
      <hostname>ews.example.org</hostname>
    </incomingServer>
    <outgoingServer type="smtp">
      <hostname>smtp.example.org</hostname>
      <port>587</port>
      <socketType>STARTTLS</socketType>
      <username>%EMAILADDRESS%</username>
      <authentication>NTLM</authentication>
      <authentication>password-cleartext</authentication>
    </outgoingServer>
  </emailProvider>
</clientConfig>"#;

    fn email() -> Email {
        Email::parse("ada@example.org").unwrap()
    }

    #[test]
    fn parses_servers_and_placeholders() {
        let found =
            parse_client_config(EXAMPLE.as_bytes(), &email(), DiscoverySource::Autoconfig).unwrap();
        assert_eq!(found.len(), 2);
        let IncomingServer::Imap { server, .. } = &found[0].incoming else {
            panic!("expected IMAP first")
        };
        assert_eq!(
            (server.host.as_str(), server.port, server.tls),
            ("imap.example.org", 993, TlsMode::Implicit)
        );
        assert_eq!(
            server.auth,
            AuthConfig::Password {
                username: "ada".into(),
                secret: None,
                mechanism: None
            }
        );
        let IncomingServer::Pop3 { server, .. } = &found[1].incoming else {
            panic!("expected POP3 second")
        };
        assert_eq!((server.port, server.tls), (110, TlsMode::Starttls));
        let Some(OutgoingServer::Smtp { server, .. }) = &found[0].outgoing else {
            panic!("expected SMTP")
        };
        assert_eq!((server.port, server.tls), (587, TlsMode::Starttls));
        // NTLM is skipped, the next method is used.
        assert!(
            matches!(&server.auth, AuthConfig::Password { username, .. } if username == "ada@example.org")
        );
        assert_eq!(found[0].provider_name.as_deref(), Some("Example Mail"));
        assert_eq!(found[0].source, DiscoverySource::Autoconfig);
    }

    #[test]
    fn oauth_for_known_providers_and_defaults() {
        let xml = r#"<clientConfig><emailProvider id="gmail.com">
            <incomingServer type="imap"><hostname>imap.gmail.com</hostname><socketType>SSL</socketType>
              <username>%EMAILADDRESS%</username>
              <authentication>OAuth2</authentication><authentication>password-cleartext</authentication></incomingServer>
            <incomingServer type="imap"><hostname>imap.unknown.example</hostname><socketType>plain</socketType>
              <authentication>OAuth2</authentication></incomingServer>
            <outgoingServer type="smtp"><hostname>smtp.gmail.com</hostname><socketType>SSL</socketType>
              <authentication>OAuth2</authentication></outgoingServer>
        </emailProvider></clientConfig>"#;
        let found = parse_client_config(xml.as_bytes(), &email(), DiscoverySource::Ispdb).unwrap();
        // The unknown host only offers OAuth2 without a known provider.
        assert_eq!(found.len(), 1);
        let server = found[0].incoming.server();
        assert_eq!(server.port, 993);
        assert!(
            matches!(&server.auth, AuthConfig::OAuth2 { provider, .. } if provider == "google")
        );
        let smtp = found[0].outgoing.as_ref().unwrap().server();
        assert_eq!(smtp.port, 465);
        assert!(
            matches!(&smtp.auth, AuthConfig::OAuth2 { username, .. } if username == "ada@example.org")
        );
    }

    #[test]
    fn rejects_other_documents() {
        assert!(
            parse_client_config(
                b"<html><body/></html>",
                &email(),
                DiscoverySource::Autoconfig
            )
            .is_err()
        );
        assert!(
            parse_client_config(b"<clientConfig/>", &email(), DiscoverySource::Autoconfig).is_err()
        );
        assert!(parse_client_config(b"garbage", &email(), DiscoverySource::Autoconfig).is_err());
        let no_servers = b"<clientConfig><emailProvider id='x'/></clientConfig>";
        assert!(
            parse_client_config(no_servers, &email(), DiscoverySource::Autoconfig)
                .unwrap()
                .is_empty()
        );
    }
}
