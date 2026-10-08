//! Autodiscover "plain old XML" (the Outlook 2006a response schema) for
//! Exchange Server on-premises: the EWS URLs (`EXCH` = inside, `EXPR` =
//! Outlook Anywhere / outside), the user's display name, automapped
//! mailboxes (`AlternativeMailbox`), IMAP / SMTP for the fallback, and
//! `redirectAddr` / `redirectUrl`.

use nail_api::accounts::{ImapOptions, IncomingServer, MailProtocol, OutgoingServer, SmtpOptions};
use nail_discovery::autodiscover::{PoxAccount, endpoint, read_account};
/// The response schema asked for and the POST body asking for an
/// address's settings (shared with `nail-discovery`).
pub use nail_discovery::autodiscover::{RESPONSE_SCHEMA, request_body};
use nail_discovery::xml::{Element, parse};

/// A mailbox the server opens for the user automatically.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlternativeMailbox {
    /// `Delegate`, `TeamMailbox`, `Archive`, … as the server says.
    pub kind: String,
    /// Display name.
    pub display_name: Option<String>,
    /// Its address (lowercase).
    pub address: String,
}

/// The settings of a POX answer.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PoxSettings {
    /// The user's display name.
    pub display_name: Option<String>,
    /// EWS URL for clients inside the network (`EXCH`).
    pub internal_ews_url: Option<String>,
    /// EWS URL for clients outside (`EXPR`, Outlook Anywhere).
    pub external_ews_url: Option<String>,
    /// IMAP / POP3 servers listed (the IMAP fallback).
    pub incoming: Vec<IncomingServer>,
    /// SMTP server listed.
    pub outgoing: Option<OutgoingServer>,
    /// Automapped mailboxes.
    pub mailboxes: Vec<AlternativeMailbox>,
}

impl PoxSettings {
    /// The EWS URL to use: the external one first (it works from
    /// anywhere), else the internal one.
    pub fn ews_url(&self) -> Option<&str> {
        self.external_ews_url
            .as_deref()
            .or(self.internal_ews_url.as_deref())
    }
}

/// What a POX answer says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PoxAnswer {
    /// Settings.
    Settings(Box<PoxSettings>),
    /// Ask again for another address.
    RedirectAddr(String),
    /// Ask again at another URL.
    RedirectUrl(String),
}

/// A `Protocol` element's type (element or attribute), upper case.
fn protocol_type(protocol: &Element) -> Option<String> {
    protocol
        .text_of("Type")
        .or_else(|| protocol.attr("Type").map(str::to_owned))
        .map(|t| t.to_ascii_uppercase())
}

/// Parses a POX answer for `address`.
pub fn parse_response(xml: &[u8], address: &str) -> Result<PoxAnswer, String> {
    let root = parse(xml)?;
    let (response, account) = match read_account(&root)? {
        PoxAccount::Account { response, account } => (response, account),
        PoxAccount::RedirectAddr(to) => return Ok(PoxAnswer::RedirectAddr(to)),
        PoxAccount::RedirectUrl(to) => return Ok(PoxAnswer::RedirectUrl(to)),
    };
    let mut settings = PoxSettings {
        display_name: response
            .child("User")
            .and_then(|u| u.text_of("DisplayName")),
        ..PoxSettings::default()
    };
    for protocol in account.children_named("Protocol") {
        match protocol_type(protocol).as_deref() {
            Some("EXCH") => {
                settings.internal_ews_url = settings
                    .internal_ews_url
                    .take()
                    .or_else(|| protocol.text_of("EwsUrl"));
            }
            Some("EXPR") => {
                settings.external_ews_url = settings
                    .external_ews_url
                    .take()
                    .or_else(|| protocol.text_of("EwsUrl"));
            }
            Some("IMAP") => {
                if let Some(server) = endpoint(protocol, MailProtocol::Imap, address) {
                    settings.incoming.push(IncomingServer::Imap {
                        server,
                        options: ImapOptions::default(),
                    });
                }
            }
            Some("SMTP") if settings.outgoing.is_none() => {
                settings.outgoing = endpoint(protocol, MailProtocol::Smtp, address).map(|server| {
                    OutgoingServer::Smtp {
                        server,
                        options: SmtpOptions::default(),
                    }
                });
            }
            _ => {}
        }
    }
    for mailbox in account.children_named("AlternativeMailbox") {
        let Some(address) = mailbox
            .text_of("SmtpAddress")
            .map(|a| a.to_ascii_lowercase())
        else {
            continue;
        };
        settings.mailboxes.push(AlternativeMailbox {
            kind: mailbox.text_of("Type").unwrap_or_default(),
            display_name: mailbox.text_of("DisplayName"),
            address,
        });
    }
    Ok(PoxAnswer::Settings(Box::new(settings)))
}

#[cfg(test)]
mod tests {
    use nail_api::accounts::{AuthConfig, TlsMode};

    use super::*;

    const ON_PREMISES: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<Autodiscover xmlns="http://schemas.microsoft.com/exchange/autodiscover/responseschema/2006">
  <Response xmlns="http://schemas.microsoft.com/exchange/autodiscover/outlook/responseschema/2006a">
    <User><DisplayName>Ada Lovelace</DisplayName><AutoDiscoverSMTPAddress>ada@corp.example</AutoDiscoverSMTPAddress></User>
    <Account>
      <AccountType>email</AccountType>
      <Action>settings</Action>
      <Protocol><Type>EXCH</Type><Server>EX01.corp.local</Server><EwsUrl>https://ex01.corp.local/EWS/Exchange.asmx</EwsUrl></Protocol>
      <Protocol><Type>EXPR</Type><Server>mail.corp.example</Server><EwsUrl>https://mail.corp.example/EWS/Exchange.asmx</EwsUrl></Protocol>
      <Protocol Type="mapiHttp" Version="1"><Server>mail.corp.example</Server></Protocol>
      <Protocol><Type>IMAP</Type><Server>IMAP.corp.example</Server><Port>993</Port><SSL>on</SSL></Protocol>
      <Protocol><Type>SMTP</Type><Server>smtp.corp.example</Server><Port>587</Port><Encryption>TLS</Encryption></Protocol>
      <AlternativeMailbox><Type>Delegate</Type><DisplayName>Support</DisplayName><SmtpAddress>Support@corp.example</SmtpAddress><OwnerSmtpAddress>support@corp.example</OwnerSmtpAddress></AlternativeMailbox>
      <AlternativeMailbox><Type>Archive</Type><DisplayName>Online Archive</DisplayName><SmtpAddress>archive-1@corp.example</SmtpAddress></AlternativeMailbox>
      <AlternativeMailbox><Type>Delegate</Type><DisplayName>No address</DisplayName></AlternativeMailbox>
    </Account>
  </Response>
</Autodiscover>"#;

    #[test]
    fn reads_exchange_settings() {
        let PoxAnswer::Settings(settings) =
            parse_response(ON_PREMISES.as_bytes(), "ada@corp.example").unwrap()
        else {
            panic!("settings expected");
        };
        assert_eq!(settings.display_name.as_deref(), Some("Ada Lovelace"));
        assert_eq!(
            settings.ews_url(),
            Some("https://mail.corp.example/EWS/Exchange.asmx")
        );
        assert_eq!(
            settings.internal_ews_url.as_deref(),
            Some("https://ex01.corp.local/EWS/Exchange.asmx")
        );
        assert_eq!(settings.incoming.len(), 1);
        let imap = settings.incoming[0].server();
        assert_eq!(
            (imap.host.as_str(), imap.port, imap.tls),
            ("imap.corp.example", 993, TlsMode::Implicit)
        );
        assert!(
            matches!(&imap.auth, AuthConfig::Password { username, .. } if username == "ada@corp.example")
        );
        let smtp = settings.outgoing.as_ref().unwrap().server();
        assert_eq!((smtp.port, smtp.tls), (587, TlsMode::Starttls));
        assert_eq!(
            settings.mailboxes,
            [
                AlternativeMailbox {
                    kind: "Delegate".into(),
                    display_name: Some("Support".into()),
                    address: "support@corp.example".into()
                },
                AlternativeMailbox {
                    kind: "Archive".into(),
                    display_name: Some("Online Archive".into()),
                    address: "archive-1@corp.example".into()
                }
            ]
        );
    }

    #[test]
    fn internal_url_alone_and_redirects() {
        let internal = r"<Autodiscover><Response><Account><Protocol><Type>EXCH</Type><EwsUrl>https://ex.corp.example/EWS/Exchange.asmx</EwsUrl></Protocol></Account></Response></Autodiscover>";
        let PoxAnswer::Settings(settings) = parse_response(internal.as_bytes(), "a@b").unwrap()
        else {
            panic!()
        };
        assert_eq!(
            settings.ews_url(),
            Some("https://ex.corp.example/EWS/Exchange.asmx")
        );
        let redirect = |action: &str, element: &str, value: &str| {
            format!(
                "<Autodiscover><Response><Account><Action>{action}</Action><{element}>{value}</{element}></Account></Response></Autodiscover>"
            )
        };
        assert_eq!(
            parse_response(
                redirect("redirectAddr", "RedirectAddr", "ada@tenant.example").as_bytes(),
                "a@b"
            )
            .unwrap(),
            PoxAnswer::RedirectAddr("ada@tenant.example".into())
        );
        assert_eq!(
            parse_response(
                redirect(
                    "redirectUrl",
                    "RedirectUrl",
                    "https://ad.corp.example/a.xml"
                )
                .as_bytes(),
                "a@b"
            )
            .unwrap(),
            PoxAnswer::RedirectUrl("https://ad.corp.example/a.xml".into())
        );
        let error = "<Autodiscover><Response><Error><ErrorCode>500</ErrorCode></Error></Response></Autodiscover>";
        assert!(
            parse_response(error.as_bytes(), "a@b")
                .unwrap_err()
                .contains("500")
        );
        assert!(parse_response(b"<html/>", "a@b").is_err());
        assert!(parse_response(b"", "a@b").is_err());
    }
}
