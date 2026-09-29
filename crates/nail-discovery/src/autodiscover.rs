//! Microsoft Autodiscover, plain-old-XML variant (the Outlook 2006a response
//! schema): IMAP / POP3 / SMTP settings, `redirectAddr` and `redirectUrl`.
//!
//! Exchange-only answers (`EXCH`, `EXPR`, `WEB`) are ignored until the
//! Exchange engines (M3).

use nail_api::accounts::{
    AuthConfig, DiscoveredAccount, DiscoverySource, ImapOptions, IncomingServer, MailProtocol,
    OutgoingServer, Pop3Options, ServerEndpoint, SmtpOptions, TlsMode, default_port,
};

use crate::email::Email;
use crate::xml::{Element, parse};

/// The request schema namespace.
const REQUEST_NS: &str =
    "http://schemas.microsoft.com/exchange/autodiscover/outlook/requestschema/2006";
/// The response schema Nail understands.
const RESPONSE_SCHEMA: &str =
    "http://schemas.microsoft.com/exchange/autodiscover/outlook/responseschema/2006a";

/// Escapes text for an XML element.
fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// The POST body asking for `address`'s settings.
pub(crate) fn request_body(address: &str) -> String {
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

/// What an Autodiscover response says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Answer {
    /// Settings (possibly none Nail can use).
    Settings(Vec<DiscoveredAccount>),
    /// Ask again for another address.
    RedirectAddr(String),
    /// Ask again at another URL.
    RedirectUrl(String),
}

fn on(value: Option<String>) -> Option<bool> {
    value.map(|v| v.eq_ignore_ascii_case("on"))
}

fn endpoint(protocol: &Element, kind: MailProtocol, email: &Email) -> Option<ServerEndpoint> {
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
        .unwrap_or_else(|| email.address.clone());
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

/// Parses an Autodiscover response.
pub(crate) fn parse_response(xml: &[u8], email: &Email) -> Result<Answer, String> {
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
                .map(Answer::RedirectAddr)
                .ok_or_else(|| "redirectAddr without address".to_owned());
        }
        Some("redirecturl") => {
            return account
                .text_of("RedirectUrl")
                .map(Answer::RedirectUrl)
                .ok_or_else(|| "redirectUrl without URL".to_owned());
        }
        _ => {}
    }
    let provider_name = response
        .child("User")
        .and_then(|u| u.text_of("DisplayName"));
    let protocols: Vec<&Element> = account.children_named("Protocol").collect();
    let of_type = |t: &'static str| -> Vec<&Element> {
        protocols
            .iter()
            .copied()
            .filter(|p| p.text_of("Type").is_some_and(|x| x.eq_ignore_ascii_case(t)))
            .collect()
    };
    let outgoing = of_type("SMTP")
        .into_iter()
        .find_map(|p| endpoint(p, MailProtocol::Smtp, email))
        .map(|server| OutgoingServer::Smtp {
            server,
            options: SmtpOptions::default(),
        });
    let mut accounts = Vec::new();
    for p in of_type("IMAP") {
        if let Some(server) = endpoint(p, MailProtocol::Imap, email) {
            accounts.push(IncomingServer::Imap {
                server,
                options: ImapOptions::default(),
            });
        }
    }
    for p in of_type("POP3") {
        if let Some(server) = endpoint(p, MailProtocol::Pop3, email) {
            accounts.push(IncomingServer::Pop3 {
                server,
                options: Pop3Options::default(),
            });
        }
    }
    Ok(Answer::Settings(
        accounts
            .into_iter()
            .map(|incoming| DiscoveredAccount {
                source: DiscoverySource::Autodiscover,
                incoming,
                outgoing: outgoing.clone(),
                provider_name: provider_name.clone(),
                warnings: Vec::new(),
            })
            .collect(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn email() -> Email {
        Email::parse("ada@example.org").unwrap()
    }

    const SETTINGS: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<Autodiscover xmlns="http://schemas.microsoft.com/exchange/autodiscover/responseschema/2006">
  <Response xmlns="http://schemas.microsoft.com/exchange/autodiscover/outlook/responseschema/2006a">
    <User><DisplayName>Ada Lovelace</DisplayName></User>
    <Account>
      <AccountType>email</AccountType>
      <Action>settings</Action>
      <Protocol><Type>EXCH</Type><Server>ex.example.org</Server></Protocol>
      <Protocol><Type>IMAP</Type><Server>IMAP.example.org</Server><Port>993</Port><SSL>on</SSL><LoginName>ada</LoginName></Protocol>
      <Protocol><Type>POP3</Type><Server>pop.example.org</Server><Port>110</Port><Encryption>TLS</Encryption></Protocol>
      <Protocol><Type>SMTP</Type><Server>smtp.example.org</Server><Port>587</Port><SSL>on</SSL><AuthRequired>on</AuthRequired></Protocol>
    </Account>
  </Response>
</Autodiscover>"#;

    #[test]
    fn request_escapes_the_address() {
        let body = request_body("a<b>&c@example.org");
        assert!(body.contains("<EMailAddress>a&lt;b&gt;&amp;c@example.org</EMailAddress>"));
        assert!(body.contains(RESPONSE_SCHEMA));
        parse(body.as_bytes()).unwrap();
    }

    #[test]
    fn parses_settings() {
        let Answer::Settings(found) = parse_response(SETTINGS.as_bytes(), &email()).unwrap() else {
            panic!("expected settings")
        };
        assert_eq!(found.len(), 2);
        let imap = found[0].incoming.server();
        assert_eq!(
            (imap.host.as_str(), imap.port, imap.tls),
            ("imap.example.org", 993, TlsMode::Implicit)
        );
        assert!(matches!(&imap.auth, AuthConfig::Password { username, .. } if username == "ada"));
        let pop = found[1].incoming.server();
        assert_eq!((pop.port, pop.tls), (110, TlsMode::Starttls));
        assert!(
            matches!(&pop.auth, AuthConfig::Password { username, .. } if username == "ada@example.org")
        );
        // SSL=on with port 587 means STARTTLS.
        let smtp = found[0].outgoing.as_ref().unwrap().server();
        assert_eq!((smtp.port, smtp.tls), (587, TlsMode::Starttls));
        assert_eq!(found[0].provider_name.as_deref(), Some("Ada Lovelace"));
    }

    #[test]
    fn redirects_and_errors() {
        let redirect = |action: &str, element: &str, value: &str| {
            format!(
                "<Autodiscover><Response><Account><Action>{action}</Action><{element}>{value}</{element}></Account></Response></Autodiscover>"
            )
        };
        assert_eq!(
            parse_response(
                redirect("redirectAddr", "RedirectAddr", "ada@new.example").as_bytes(),
                &email()
            )
            .unwrap(),
            Answer::RedirectAddr("ada@new.example".into())
        );
        assert_eq!(
            parse_response(
                redirect("redirectUrl", "RedirectUrl", "https://ad.example/a.xml").as_bytes(),
                &email()
            )
            .unwrap(),
            Answer::RedirectUrl("https://ad.example/a.xml".into())
        );
        let error = "<Autodiscover><Response><Error><ErrorCode>600</ErrorCode></Error></Response></Autodiscover>";
        assert!(
            parse_response(error.as_bytes(), &email())
                .unwrap_err()
                .contains("600")
        );
        assert!(parse_response(b"<html/>", &email()).is_err());
        let plain = "<Autodiscover><Response><Account><Protocol><Type>SMTP</Type><Server>s.example.org</Server><SSL>off</SSL><AuthRequired>off</AuthRequired></Protocol></Account></Response></Autodiscover>";
        assert_eq!(
            parse_response(plain.as_bytes(), &email()).unwrap(),
            Answer::Settings(vec![])
        );
    }
}
