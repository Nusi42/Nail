//! Decoding of the SASL payloads the mock servers accept.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;

/// Decodes base64 (whitespace-tolerant). `None` when invalid.
pub(crate) fn b64_decode(text: &str) -> Option<Vec<u8>> {
    let clean: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    STANDARD.decode(clean).ok()
}

/// Encodes base64.
pub(crate) fn b64_encode(bytes: &[u8]) -> String {
    STANDARD.encode(bytes)
}

/// `PLAIN` (RFC 4616): `authzid NUL authcid NUL password` → (authcid, password).
pub(crate) fn parse_plain(payload: &[u8]) -> Option<(String, String)> {
    let mut parts = payload.split(|b| *b == 0);
    let _authzid = parts.next()?;
    let user = parts.next()?;
    let pass = parts.next()?;
    if parts.next().is_some() {
        return None;
    }
    Some((
        String::from_utf8(user.to_vec()).ok()?,
        String::from_utf8(pass.to_vec()).ok()?,
    ))
}

/// `XOAUTH2`: `user=<user>^Aauth=Bearer <token>^A^A` → (user, token).
pub(crate) fn parse_xoauth2(payload: &[u8]) -> Option<(String, String)> {
    let text = std::str::from_utf8(payload).ok()?;
    let mut user = None;
    let mut token = None;
    for field in text.split('\x01') {
        if let Some(u) = field.strip_prefix("user=") {
            user = Some(u.to_owned());
        } else if let Some(a) = field.strip_prefix("auth=") {
            token = a
                .strip_prefix("Bearer ")
                .or_else(|| a.strip_prefix("bearer "))
                .map(str::to_owned);
        }
    }
    Some((user?, token?))
}

/// `OAUTHBEARER` (RFC 7628): `n,a=<user>,^A...auth=Bearer <token>^A^A` →
/// (user, token).
pub(crate) fn parse_oauthbearer(payload: &[u8]) -> Option<(String, String)> {
    let text = std::str::from_utf8(payload).ok()?;
    let (gs2, rest) = text.split_once('\x01')?;
    let user = gs2
        .split(',')
        .find_map(|p| p.strip_prefix("a="))
        .map(|u| u.replace("=2C", ",").replace("=3D", "="))?;
    let token = rest
        .split('\x01')
        .find_map(|f| f.strip_prefix("auth="))
        .and_then(|a| {
            a.strip_prefix("Bearer ")
                .or_else(|| a.strip_prefix("bearer "))
        })
        .map(str::to_owned)?;
    Some((user, token))
}

/// What a SASL response proves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Proof {
    /// The client cancelled (`*`).
    Cancelled,
    /// The response is not base64.
    BadBase64,
    /// The login is accepted.
    Accepted,
    /// A wrong user name or password (`PLAIN`).
    Rejected,
    /// An unknown OAuth2 token: the server sends the error challenge
    /// ([`oauth_error_challenge`]) and reads the client's empty answer.
    TokenRejected,
}

/// Checks a SASL response (`PLAIN`, `XOAUTH2`, `OAUTHBEARER`) against the
/// accepted password login and OAuth2 tokens.
pub(crate) fn verify(
    mechanism: &str,
    payload: &str,
    username: &str,
    password: &str,
    tokens: &[(String, String)],
) -> Proof {
    if payload.trim() == "*" {
        return Proof::Cancelled;
    }
    let Some(decoded) = b64_decode(payload) else {
        return Proof::BadBase64;
    };
    if mechanism == "PLAIN" {
        let ok = parse_plain(&decoded).is_some_and(|(u, p)| u == username && p == password);
        return if ok { Proof::Accepted } else { Proof::Rejected };
    }
    let parsed = if mechanism == "XOAUTH2" {
        parse_xoauth2(&decoded)
    } else {
        parse_oauthbearer(&decoded)
    };
    if parsed.is_some_and(|pair| tokens.contains(&pair)) {
        Proof::Accepted
    } else {
        Proof::TokenRejected
    }
}

/// A protocol's answers to a failed SASL exchange.
#[derive(Debug)]
pub(crate) struct Answers {
    /// The client cancelled (`*`).
    pub cancelled: &'static str,
    /// The response is not base64.
    pub bad_base64: &'static str,
    /// The login is refused.
    pub failed: &'static str,
    /// The error challenge line for a rejected OAuth2 token.
    pub challenge: fn(&str) -> String,
}

/// The login a mock server's `config` accepts (its `username`, `password`
/// and `oauth_tokens` fields), as [`finish`] takes it.
macro_rules! login_of {
    ($config:expr) => {
        (
            $config.username.as_str(),
            $config.password.as_str(),
            $config.oauth_tokens.as_slice(),
        )
    };
}
pub(crate) use login_of;

/// Checks a SASL response ([`verify`]) and sends the protocol's error
/// answers (for a rejected token: the error challenge, then the client's
/// dummy line is read); `Ok(true)` when the client is logged in (the
/// caller sends the success answer).
pub(crate) async fn finish(
    conn: &mut crate::stream::Connection,
    mechanism: &str,
    payload: &str,
    login: (&str, &str, &[(String, String)]),
    answers: &Answers,
) -> std::io::Result<bool> {
    let (username, password, tokens) = login;
    let ok = match verify(mechanism, payload, username, password, tokens) {
        Proof::Cancelled => {
            conn.line(answers.cancelled).await?;
            return Ok(false);
        }
        Proof::BadBase64 => {
            conn.line(answers.bad_base64).await?;
            return Ok(false);
        }
        Proof::TokenRejected => {
            conn.line(&(answers.challenge)(&oauth_error_challenge()))
                .await?;
            drop(conn.read_line().await?);
            false
        }
        Proof::Rejected => false,
        Proof::Accepted => true,
    };
    if !ok {
        conn.line(answers.failed).await?;
    }
    Ok(ok)
}

/// The JSON error challenge Google and Microsoft send for a rejected token.
pub(crate) fn oauth_error_challenge() -> String {
    b64_encode(br#"{"status":"401","schemes":"bearer","scope":"https://mail.google.com/"}"#)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verifies_responses() {
        let tokens = vec![("ada".to_owned(), "t1".to_owned())];
        let login = ("ada", "pw");
        let plain = b64_encode(b"\0ada\0pw");
        assert_eq!(
            verify("PLAIN", &plain, login.0, login.1, &tokens),
            Proof::Accepted
        );
        let wrong = b64_encode(b"\0ada\0no");
        assert_eq!(
            verify("PLAIN", &wrong, login.0, login.1, &tokens),
            Proof::Rejected
        );
        assert_eq!(
            verify("PLAIN", " * ", login.0, login.1, &tokens),
            Proof::Cancelled
        );
        assert_eq!(
            verify("PLAIN", "%%%", login.0, login.1, &tokens),
            Proof::BadBase64
        );
        let xoauth = b64_encode(b"user=ada\x01auth=Bearer t1\x01\x01");
        assert_eq!(
            verify("XOAUTH2", &xoauth, login.0, login.1, &tokens),
            Proof::Accepted
        );
        let stale = b64_encode(b"user=ada\x01auth=Bearer t2\x01\x01");
        assert_eq!(
            verify("XOAUTH2", &stale, login.0, login.1, &tokens),
            Proof::TokenRejected
        );
        let bearer = b64_encode(b"n,a=ada,\x01auth=Bearer t1\x01\x01");
        assert_eq!(
            verify("OAUTHBEARER", &bearer, login.0, login.1, &tokens),
            Proof::Accepted
        );
    }

    #[test]
    fn decodes_mechanisms() {
        assert_eq!(
            parse_plain(b"\0ada\0secret"),
            Some(("ada".into(), "secret".into()))
        );
        assert_eq!(parse_plain(b"ada"), None);
        assert_eq!(
            parse_xoauth2(b"user=ada@x\x01auth=Bearer tok\x01\x01"),
            Some(("ada@x".into(), "tok".into()))
        );
        assert_eq!(parse_xoauth2(b"user=ada@x\x01\x01"), None);
        assert_eq!(
            parse_oauthbearer(b"n,a=ada@x,\x01host=h\x01port=1\x01auth=Bearer tok\x01\x01"),
            Some(("ada@x".into(), "tok".into()))
        );
        assert_eq!(b64_decode(&b64_encode(b"hi")), Some(b"hi".to_vec()));
        assert_eq!(b64_decode("!!"), None);
        assert!(b64_decode(&oauth_error_challenge()).is_some());
    }
}
