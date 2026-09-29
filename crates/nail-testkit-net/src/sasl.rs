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

/// The JSON error challenge Google and Microsoft send for a rejected token.
pub(crate) fn oauth_error_challenge() -> String {
    b64_encode(br#"{"status":"401","schemes":"bearer","scope":"https://mail.google.com/"}"#)
}

#[cfg(test)]
mod tests {
    use super::*;

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
