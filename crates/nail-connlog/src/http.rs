//! HTTP rules: request and response lines of the HTTP-based protocols
//! (CalDAV / CardDAV, Gmail, Graph, EWS, OAuth token endpoints,
//! Autodiscover). Bodies are never logged, only their size; for failed
//! answers the error code and message of a JSON or SOAP body.

use crate::imap::{SEARCH_TERM, VALUE};
use crate::lexer::bytes_placeholder;
use crate::redact::{REDACTED, cut};

/// Query parameters whose values are kept (paging, field lists, flags).
const KEEP_QUERY: &[&str] = &[
    "$count",
    "$expand",
    "$orderby",
    "$select",
    "$skip",
    "$top",
    "alt",
    "api-version",
    "depth",
    "fields",
    "format",
    "grant_type",
    "historytypes",
    "includespamtrash",
    "labelid",
    "labelids",
    "maxresults",
    "pagesize",
    "prompt",
    "response_mode",
    "response_type",
    "scope",
    "uploadtype",
    "view",
];

/// Query parameters that hold search criteria.
const SEARCH_QUERY: &[&str] = &["$filter", "$search", "q", "query"];

/// Query parameters that hold opaque cursors (long, not secret).
const CURSOR_QUERY: &[&str] = &[
    "$deltatoken",
    "$skiptoken",
    "pagetoken",
    "starthistoryid",
    "synctoken",
];

/// Query parameters that hold credentials.
const SECRET_QUERY: &[&str] = &[
    "access_token",
    "assertion",
    "client_secret",
    "code",
    "code_challenge",
    "code_verifier",
    "id_token",
    "key",
    "password",
    "refresh_token",
    "sig",
    "signature",
    "state",
    "token",
];

/// Longest error text kept from a failed answer's body.
pub const ERROR_TEXT_BYTES: usize = 300;

/// The URL as logged: credentials in the authority removed, query values
/// kept, hidden or replaced by placeholders by name, no fragment.
/// (Addresses in the path are masked by the final scrub.)
pub fn url(url: &str) -> String {
    let url = url.split('#').next().unwrap_or_default();
    let (base, query) = match url.split_once('?') {
        Some((base, query)) => (base, Some(query)),
        None => (url, None),
    };
    let base = match base.split_once("://") {
        Some((scheme, rest)) => {
            let (authority, path) = rest.split_at(rest.find('/').unwrap_or(rest.len()));
            match authority.rsplit_once('@') {
                Some((_, host)) => format!("{scheme}://{REDACTED}@{host}{path}"),
                None => format!("{scheme}://{authority}{path}"),
            }
        }
        None => base.to_owned(),
    };
    let Some(query) = query else {
        return base;
    };
    let pairs: Vec<String> = query
        .split('&')
        .filter(|pair| !pair.is_empty())
        .map(|pair| {
            let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
            let name = key.to_ascii_lowercase();
            let name = name.as_str();
            let shown = if value.is_empty() || KEEP_QUERY.contains(&name) {
                return pair.to_owned();
            } else if SEARCH_QUERY.contains(&name) {
                SEARCH_TERM
            } else if CURSOR_QUERY.contains(&name) {
                "<cursor>"
            } else if SECRET_QUERY.contains(&name) {
                REDACTED
            } else {
                VALUE
            };
            format!("{key}={shown}")
        })
        .collect();
    format!("{base}?{}", pairs.join("&"))
}

/// The URL with its path and query hidden (`https://host/<path>`): for
/// calendar subscriptions, whose secret URLs carry their access in the path
/// (`…/private-<token>/basic.ics`).
pub fn origin_only(url: &str) -> String {
    let url = self::url(url);
    match url.split_once("://") {
        Some((scheme, rest)) => {
            let authority = rest.split('/').next().unwrap_or_default();
            format!("{scheme}://{authority}/<path>")
        }
        None => "<path>".to_owned(),
    }
}

/// One request line: `POST https://graph.microsoft.com/v1.0/me/sendMail
/// (Authorization: <redacted>) {4211 bytes}`.
pub fn request_line(method: &str, target: &str, authorization: bool, body: Option<u64>) -> String {
    let mut line = format!("{method} {}", url(target));
    if authorization {
        line.push_str(" (Authorization: ");
        line.push_str(REDACTED);
        line.push(')');
    }
    if let Some(body) = body.filter(|b| *b > 0) {
        line.push(' ');
        line.push_str(&bytes_placeholder(body));
    }
    line
}

/// One response line: `401 · 312 ms · {123 bytes} · invalid_grant: AADSTS…`.
pub fn response_line(status: u16, millis: u128, body: Option<u64>, error: Option<&str>) -> String {
    let mut line = format!("{status} · {millis} ms");
    if let Some(body) = body {
        line.push_str(" · ");
        line.push_str(&bytes_placeholder(body));
    }
    if let Some(error) = error.filter(|e| !e.is_empty()) {
        line.push_str(" · ");
        line.push_str(error);
    }
    line
}

/// The error code and message of a failed answer's body: OAuth / Graph /
/// Gmail JSON (`error`, `error_description`, `error.code`, `error.message`,
/// `error.status`) or SOAP / XML (`faultstring`, `ResponseCode`,
/// `MessageText`). Cut to [`ERROR_TEXT_BYTES`]; `None` for anything else
/// (the body itself is never logged).
pub fn error_summary(body: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(body).ok()?.trim();
    let summary = if text.starts_with('{') {
        json_error(text)?
    } else if text.starts_with('<') {
        xml_error(text)?
    } else {
        return None;
    };
    let summary = summary.split_whitespace().collect::<Vec<_>>().join(" ");
    (!summary.is_empty()).then(|| cut(&summary, ERROR_TEXT_BYTES))
}

fn json_error(text: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(text).ok()?;
    let string = |v: Option<&serde_json::Value>| -> Option<String> {
        match v? {
            serde_json::Value::String(s) => Some(s.clone()),
            serde_json::Value::Number(n) => Some(n.to_string()),
            _ => None,
        }
    };
    let mut parts = Vec::new();
    match value.get("error") {
        Some(serde_json::Value::Object(error)) => {
            parts.extend(string(error.get("code")));
            parts.extend(string(error.get("status")));
            parts.extend(string(error.get("message")));
        }
        other => parts.extend(string(other)),
    }
    parts.extend(string(value.get("error_description")));
    if parts.is_empty() {
        parts.extend(string(value.get("message")));
    }
    (!parts.is_empty()).then(|| parts.join(": "))
}

fn xml_error(text: &str) -> Option<String> {
    let mut parts = Vec::new();
    for name in ["faultstring", "ResponseCode", "MessageText"] {
        if let Some(value) = element_text(text, name)
            && !parts.contains(&value)
        {
            parts.push(value);
        }
    }
    (!parts.is_empty()).then(|| parts.join(": "))
}

/// The text of the first element with this local name (`<m:MessageText>`).
fn element_text(text: &str, local: &str) -> Option<String> {
    let mut search = 0;
    while let Some(at) = text[search..].find('<') {
        let start = search + at + 1;
        let end = start + text[start..].find('>')?;
        let tag = &text[start..end];
        search = end;
        let name = tag.split_whitespace().next().unwrap_or_default();
        let name = name.rsplit(':').next().unwrap_or(name);
        if name == local && !tag.ends_with('/') {
            let close = text[end + 1..].find("</")?;
            return Some(text[end + 1..end + 1 + close].trim().to_owned());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn urls_keep_paging_and_hide_criteria_and_secrets() {
        assert_eq!(
            url(
                "https://graph.microsoft.com/v1.0/me/messages?$top=50&$search=%22salary%22&$filter=from/emailAddress/address%20eq%20'a@b.org'&$skiptoken=abc#frag"
            ),
            "https://graph.microsoft.com/v1.0/me/messages?$top=50&$search=<search term>&$filter=<search term>&$skiptoken=<cursor>"
        );
        assert_eq!(
            url("https://user:pass@dav.example.org/cal/?code=xyz&state=s&foo=bar&flag"),
            "https://<redacted>@dav.example.org/cal/?code=<redacted>&state=<redacted>&foo=<value>&flag"
        );
        assert_eq!(
            url(
                "https://gmail.googleapis.com/gmail/v1/users/me/messages?q=from:boss&maxResults=100"
            ),
            "https://gmail.googleapis.com/gmail/v1/users/me/messages?q=<search term>&maxResults=100"
        );
    }

    #[test]
    fn subscription_urls_keep_their_origin_only() {
        assert_eq!(
            origin_only("https://calendar.google.com/calendar/ical/x/private-abc/basic.ics?a=b"),
            "https://calendar.google.com/<path>"
        );
        assert_eq!(
            origin_only("https://user:pw@feeds.example/x"),
            "https://<redacted>@feeds.example/<path>"
        );
    }

    #[test]
    fn request_and_response_lines() {
        assert_eq!(
            request_line(
                "POST",
                "https://login.microsoftonline.com/common/oauth2/v2.0/token",
                false,
                Some(512)
            ),
            "POST https://login.microsoftonline.com/common/oauth2/v2.0/token {512 bytes}"
        );
        assert_eq!(
            request_line("GET", "https://x.example/a", true, None),
            "GET https://x.example/a (Authorization: <redacted>)"
        );
        assert_eq!(
            response_line(401, 312, Some(120), Some("invalid_grant: expired")),
            "401 · 312 ms · {120 bytes} · invalid_grant: expired"
        );
    }

    #[test]
    fn error_bodies_give_code_and_message_only() {
        assert_eq!(
            error_summary(br#"{"error":"invalid_grant","error_description":"AADSTS70008: The refresh token has expired.","access_token":"x"}"#).as_deref(),
            Some("invalid_grant: AADSTS70008: The refresh token has expired.")
        );
        assert_eq!(
            error_summary(
                br#"{"error":{"code":"ErrorAccessDenied","message":"Access is denied."}}"#
            )
            .as_deref(),
            Some("ErrorAccessDenied: Access is denied.")
        );
        assert_eq!(
            error_summary(br#"{"error":{"code":403,"message":"Insufficient Permission","status":"PERMISSION_DENIED"}}"#).as_deref(),
            Some("403: PERMISSION_DENIED: Insufficient Permission")
        );
        let soap = br#"<s:Envelope><s:Body><m:GetItemResponse><m:ResponseMessages><m:GetItemResponseMessage ResponseClass="Error"><m:MessageText>The specified object was not found in the store.</m:MessageText><m:ResponseCode>ErrorItemNotFound</m:ResponseCode></m:GetItemResponseMessage></m:ResponseMessages></m:GetItemResponse></s:Body></s:Envelope>"#;
        assert_eq!(
            error_summary(soap).as_deref(),
            Some("ErrorItemNotFound: The specified object was not found in the store.")
        );
        assert_eq!(
            error_summary(b"BEGIN:VCALENDAR\r\nSUMMARY:Secret\r\n"),
            None
        );
        assert_eq!(error_summary(br#"{"items":[{"subject":"x"}]}"#), None);
        let long = format!(r#"{{"error":"{}"}}"#, "x".repeat(500));
        assert!(error_summary(long.as_bytes()).unwrap().len() < 330);
    }
}
