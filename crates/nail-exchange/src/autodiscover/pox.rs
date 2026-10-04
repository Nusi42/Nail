//! Autodiscover "plain old XML" (the Outlook 2006a response schema) for
//! Exchange Server on-premises: the EWS URLs (`EXCH` = inside, `EXPR` =
//! Outlook Anywhere / outside), the user's display name, automapped
//! mailboxes (`AlternativeMailbox`), IMAP / SMTP for the fallback, and
//! `redirectAddr` / `redirectUrl`.

use nail_api::accounts::{
    AuthConfig, ImapOptions, IncomingServer, MailProtocol, OutgoingServer, ServerEndpoint,
    SmtpOptions, TlsMode, default_port,
};

use crate::xml::{Element, parse};

/// The request schema namespace.
const REQUEST_NS: &str =
    "http://schemas.microsoft.com/exchange/autodiscover/outlook/requestschema/2006";
/// The response schema asked for.
pub const RESPONSE_SCHEMA: &str =
    "http://schemas.microsoft.com/exchange/autodiscover/outlook/responseschema/2006a";

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// The POST body asking for `address`'s settings.
pub fn request_body(address: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\r\n\
         <Autodiscover xmlns=\"{REQUEST_NS}\">\r\n\
         <Request>\r\n\
         <EMailAddress>{}</EMailAddress>\r\n\
         <AcceptableResponseSchema>{RESPONSE_SCHEMA}</AcceptableResponseSchema>\r\n\
         </Request>\r\n\
         </Autodiscover>\r\n",
        escape(address)
    )
}

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

fn on(value: Option<String>) -> Option<bool> {
    value.map(|v| v.eq_ignore_ascii_case("on"))
}

fn protocol_type(protocol: &Element) -> Option<String> {
    protocol
        .text_of("Type")
        .or_else(|| protocol.attr("Type").map(str::to_owned))
        .map(|t| t.to_ascii_uppercase())
}

fn endpoint(protocol: &Element, kind: MailProtocol, address: &str) -> Option<ServerEndpoint> {
    let host = protocol.text_of("Server")?.to_ascii_lowercase();
    let port = protocol
        .text_of("Port")
        .and_then(|p| p.parse::<u16>().ok())
        .filter(|p| *p != 0);
    let implicit_port = |p: u16| matches!(p, 993 | 995 | 465);
    let tls = match protocol
        .text_of("Encryption")
        .map(|e| e.to_ascii_lowercase())
        .as_deref()
    {
        Some("ssl") => TlsMode::Implicit,
        Some("tls") => TlsMode::Starttls,
        Some("none") => TlsMode::Plaintext,
        _ if on(protocol.text_of("SSL")) == Some(false) => TlsMode::Plaintext,
        _ => match port {
            Some(p) if !implicit_port(p) => TlsMode::Starttls,
            _ => TlsMode::Implicit,
        },
    };
    let port = port.unwrap_or_else(|| default_port(kind, tls));
    let username = protocol
        .text_of("LoginName")
        .unwrap_or_else(|| address.to_owned());
    let auth = if kind == MailProtocol::Smtp && on(protocol.text_of("AuthRequired")) == Some(false)
    {
        AuthConfig::None
    } else {
        AuthConfig::Password {
            username,
            secret: None,
            mechanism: None,
        }
    };
    Some(ServerEndpoint {
        host,
        port,
        tls,
        auth,
    })
}

/// Parses a POX answer for `address`.
pub fn parse_response(xml: &[u8], address: &str) -> Result<PoxAnswer, String> {
    let root = parse(xml)?;
    if !root.name.eq_ignore_ascii_case("Autodiscover") {
        return Err(format!("unexpected root element <{}>", root.name));
    }
    let response = root.child("Response").ok_or("no Response element")?;
    if let Some(error) = response.child("Error") {
        return Err(format!(
            "Autodiscover error {}",
            error.text_of("ErrorCode").unwrap_or_default()
        ));
    }
    let account = response.child("Account").ok_or("no Account element")?;
    match account
        .text_of("Action")
        .map(|a| a.to_ascii_lowercase())
        .as_deref()
    {
        Some("redirectaddr") => {
            return account
                .text_of("RedirectAddr")
                .map(PoxAnswer::RedirectAddr)
                .ok_or_else(|| "redirectAddr without address".to_owned());
        }
        Some("redirecturl") => {
            return account
                .text_of("RedirectUrl")
                .map(PoxAnswer::RedirectUrl)
                .ok_or_else(|| "redirectUrl without URL".to_owned());
        }
        _ => {}
    }
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
    fn request_escapes_the_address() {
        let body = request_body("a<b>&c@corp.example");
        assert!(body.contains("<EMailAddress>a&lt;b&gt;&amp;c@corp.example</EMailAddress>"));
        assert!(body.contains(RESPONSE_SCHEMA));
        parse(body.as_bytes()).unwrap();
    }

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
