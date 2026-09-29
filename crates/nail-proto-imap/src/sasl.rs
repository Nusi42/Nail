//! Authentication: choosing the mechanism and building SASL messages
//! (`PLAIN` RFC 4616, `XOAUTH2`, `OAUTHBEARER` RFC 7628).

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;

use crate::capabilities::Capabilities;
use crate::config::{AuthMechanism, Credentials, Secret};
use crate::error::{ImapError, Result};

/// How the client logs in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Login {
    /// The `LOGIN` command.
    Login,
    /// `AUTHENTICATE <mechanism>`.
    Sasl(Mechanism),
}

/// SASL mechanisms the client speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mechanism {
    Plain,
    XOAuth2,
    OAuthBearer,
}

impl Mechanism {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Plain => "PLAIN",
            Self::XOAuth2 => "XOAUTH2",
            Self::OAuthBearer => "OAUTHBEARER",
        }
    }

    /// The initial client response (not yet base64-encoded).
    pub(crate) fn initial_response(
        self,
        credentials: &Credentials,
        host: &str,
        port: u16,
    ) -> Vec<u8> {
        let user = &credentials.username;
        let secret = credentials.secret.expose();
        match self {
            // [authzid] NUL authcid NUL passwd
            Self::Plain => format!("\0{user}\0{secret}").into_bytes(),
            Self::XOAuth2 => format!("user={user}\x01auth=Bearer {secret}\x01\x01").into_bytes(),
            // gs2-header, then key=value pairs separated by ^A.
            Self::OAuthBearer => format!(
                "n,a={},\x01host={host}\x01port={port}\x01auth=Bearer {secret}\x01\x01",
                gs2_escape(user)
            )
            .into_bytes(),
        }
    }

    /// What to answer a server challenge with. The OAuth mechanisms get an
    /// error challenge (base64 JSON) on failure and must answer it before
    /// the server sends the tagged `NO`: XOAUTH2 with an empty response,
    /// OAUTHBEARER with `%x01` (RFC 7628 section 3.2.3). `PLAIN` has no
    /// challenges, so the exchange is cancelled with `*`.
    pub(crate) fn challenge_response(self) -> &'static str {
        match self {
            Self::XOAuth2 => "",
            Self::OAuthBearer => "AQ==",
            Self::Plain => "*",
        }
    }
}

/// RFC 5801 `saslname`: `,` and `=` are escaped.
fn gs2_escape(user: &str) -> String {
    user.replace('=', "=3D").replace(',', "=2C")
}

/// Base64 for an `AUTHENTICATE` response (`=` for an empty initial
/// response, RFC 4959).
pub(crate) fn encode_response(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        "=".to_owned()
    } else {
        BASE64.encode(bytes)
    }
}

/// Decodes an OAuth error challenge (`{"status":"401",...}`) for messages.
pub(crate) fn describe_challenge(challenge: &str) -> String {
    BASE64
        .decode(challenge.trim())
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .unwrap_or_else(|| challenge.to_owned())
}

/// Picks the login method for the credentials and server capabilities.
pub(crate) fn choose_login(
    requested: AuthMechanism,
    credentials: &Credentials,
    caps: &Capabilities,
) -> Result<Login> {
    let token = matches!(credentials.secret, Secret::AccessToken(_));
    if token != requested.is_oauth() && requested != AuthMechanism::Auto {
        return Err(ImapError::AuthUnsupported(format!(
            "{requested:?} does not work with {}",
            if token {
                "an access token"
            } else {
                "a password"
            }
        )));
    }
    let login_disabled = caps.has("LOGINDISABLED");
    match requested {
        AuthMechanism::Plain => Ok(Login::Sasl(Mechanism::Plain)),
        AuthMechanism::XOAuth2 => Ok(Login::Sasl(Mechanism::XOAuth2)),
        AuthMechanism::OAuthBearer => Ok(Login::Sasl(Mechanism::OAuthBearer)),
        AuthMechanism::Login if login_disabled => {
            Err(ImapError::AuthUnsupported("LOGINDISABLED".to_owned()))
        }
        AuthMechanism::Login => Ok(Login::Login),
        AuthMechanism::Auto if token => {
            if caps.has_auth("OAUTHBEARER") {
                Ok(Login::Sasl(Mechanism::OAuthBearer))
            } else if caps.has_auth("XOAUTH2") {
                Ok(Login::Sasl(Mechanism::XOAuth2))
            } else {
                Err(ImapError::AuthUnsupported(
                    "the server offers neither OAUTHBEARER nor XOAUTH2".to_owned(),
                ))
            }
        }
        AuthMechanism::Auto if caps.has_auth("PLAIN") => Ok(Login::Sasl(Mechanism::Plain)),
        AuthMechanism::Auto if login_disabled => Err(ImapError::AuthUnsupported(
            "LOGINDISABLED without AUTH=PLAIN".to_owned(),
        )),
        AuthMechanism::Auto => Ok(Login::Login),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mechanism_choice_for_passwords() {
        let password = Credentials::new("sam", "pw");
        let plain = Capabilities::from_atoms(["IMAP4rev1", "AUTH=PLAIN"]);
        let none = Capabilities::from_atoms(["IMAP4rev1"]);
        let disabled = Capabilities::from_atoms(["IMAP4rev1", "LOGINDISABLED"]);
        let choose = |m, caps| choose_login(m, &password, caps);
        assert_eq!(
            choose(AuthMechanism::Auto, &plain).unwrap(),
            Login::Sasl(Mechanism::Plain)
        );
        assert_eq!(choose(AuthMechanism::Auto, &none).unwrap(), Login::Login);
        assert!(choose(AuthMechanism::Auto, &disabled).is_err());
        assert!(choose(AuthMechanism::Login, &disabled).is_err());
        assert_eq!(
            choose(AuthMechanism::Plain, &none).unwrap(),
            Login::Sasl(Mechanism::Plain)
        );
        assert!(matches!(
            choose(AuthMechanism::XOAuth2, &plain),
            Err(ImapError::AuthUnsupported(_))
        ));
    }

    #[test]
    fn mechanism_choice_for_tokens() {
        let token = Credentials::oauth2("sam", "tok");
        let both = Capabilities::from_atoms(["IMAP4rev1", "AUTH=XOAUTH2", "AUTH=OAUTHBEARER"]);
        let xoauth = Capabilities::from_atoms(["IMAP4rev1", "AUTH=XOAUTH2", "AUTH=PLAIN"]);
        let plain = Capabilities::from_atoms(["IMAP4rev1", "AUTH=PLAIN"]);
        let choose = |m, caps| choose_login(m, &token, caps);
        assert_eq!(
            choose(AuthMechanism::Auto, &both).unwrap(),
            Login::Sasl(Mechanism::OAuthBearer)
        );
        assert_eq!(
            choose(AuthMechanism::Auto, &xoauth).unwrap(),
            Login::Sasl(Mechanism::XOAuth2)
        );
        assert!(matches!(
            choose(AuthMechanism::Auto, &plain),
            Err(ImapError::AuthUnsupported(_))
        ));
        assert_eq!(
            choose(AuthMechanism::XOAuth2, &both).unwrap(),
            Login::Sasl(Mechanism::XOAuth2)
        );
        assert!(choose(AuthMechanism::Plain, &both).is_err());
        assert!(choose(AuthMechanism::Login, &both).is_err());
    }

    #[test]
    fn initial_responses() {
        let password = Credentials::new("sam", "pw");
        assert_eq!(
            Mechanism::Plain.initial_response(&password, "h", 1),
            b"\0sam\0pw"
        );
        let token = Credentials::oauth2("sam@x.example", "ya29");
        assert_eq!(
            Mechanism::XOAuth2.initial_response(&token, "h", 1),
            b"user=sam@x.example\x01auth=Bearer ya29\x01\x01"
        );
        let token = Credentials::oauth2("a,b=c", "t");
        assert_eq!(
            Mechanism::OAuthBearer.initial_response(&token, "imap.x", 993),
            b"n,a=a=2Cb=3Dc,\x01host=imap.x\x01port=993\x01auth=Bearer t\x01\x01"
        );
        assert_eq!(encode_response(b""), "=");
        assert_eq!(encode_response(b"\x01"), "AQ==");
        assert_eq!(
            describe_challenge("eyJzdGF0dXMiOiI0MDEifQ=="),
            r#"{"status":"401"}"#
        );
        assert_eq!(describe_challenge("not base64!"), "not base64!");
        assert_eq!(Mechanism::OAuthBearer.challenge_response(), "AQ==");
    }
}
