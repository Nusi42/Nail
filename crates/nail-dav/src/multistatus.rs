//! `207 Multi-Status` answers (RFC 4918 §13, RFC 6578 §6).

use url::Url;

use crate::NS_DAV as D;
use crate::error::DavError;
use crate::xml::{self, Element};

/// One `<response>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MsResponse {
    /// The `href` as sent (usually an absolute, percent-encoded path).
    pub href: String,
    /// A response-level `status` (e.g. `404` for a removed member in a
    /// sync report, `507` for a truncated one), if present.
    pub status: Option<u16>,
    /// `(status, <prop>)` per `propstat`.
    pub propstats: Vec<(u16, Element)>,
    /// A response-level `<error>` element, if present.
    pub error: Option<Element>,
}

impl MsResponse {
    /// A property found with a `2xx` status.
    pub fn prop(&self, ns: &str, name: &str) -> Option<&Element> {
        self.propstats
            .iter()
            .filter(|(status, _)| (200..300).contains(status))
            .find_map(|(_, prop)| prop.child(ns, name))
    }

    /// The trimmed text of a found property (`None` when empty).
    pub fn prop_text(&self, ns: &str, name: &str) -> Option<String> {
        self.prop(ns, name)
            .map(|p| p.text.trim().to_owned())
            .filter(|t| !t.is_empty())
    }

    /// The `href` children of a found property, resolved against `base`.
    pub fn prop_hrefs(&self, base: &Url, ns: &str, name: &str) -> Vec<Url> {
        self.prop(ns, name)
            .map(|p| {
                p.children_named(D, "href")
                    .filter_map(|h| base.join(h.text.trim()).ok())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The `href` resolved against `base`.
    pub fn url(&self, base: &Url) -> Option<Url> {
        base.join(&self.href).ok()
    }

    /// Whether the response stands for a missing resource (`404`).
    pub fn is_not_found(&self) -> bool {
        self.status == Some(404)
    }
}

/// A whole multistatus answer.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Multistatus {
    /// The responses.
    pub responses: Vec<MsResponse>,
    /// `sync-token` of a `sync-collection` report.
    pub sync_token: Option<String>,
}

/// The status code of `HTTP/1.1 200 OK`.
pub fn parse_status_line(line: &str) -> Option<u16> {
    line.split_whitespace().nth(1)?.parse().ok()
}

/// Parses a multistatus body.
pub fn parse(body: &str) -> Result<Multistatus, DavError> {
    let root = xml::parse(body).map_err(|e| DavError::Protocol(e.to_string()))?;
    if !root.is(D, "multistatus") {
        return Err(DavError::Protocol(format!(
            "expected a multistatus, got {{{}}}{}",
            root.ns, root.name
        )));
    }
    let mut out = Multistatus {
        sync_token: root
            .child_text(D, "sync-token")
            .filter(|t| !t.is_empty())
            .map(str::to_owned),
        ..Multistatus::default()
    };
    for response in root.children_named(D, "response") {
        let Some(href) = response.child_text(D, "href") else {
            continue;
        };
        let propstats = response
            .children_named(D, "propstat")
            .filter_map(|ps| {
                let status = ps.child_text(D, "status").and_then(parse_status_line)?;
                Some((status, ps.child(D, "prop")?.clone()))
            })
            .collect();
        out.responses.push(MsResponse {
            href: href.to_owned(),
            status: response.child_text(D, "status").and_then(parse_status_line),
            propstats,
            error: response.child(D, "error").cloned(),
        });
    }
    Ok(out)
}

/// Whether an error body names the precondition `{ns}name` (e.g.
/// `DAV:valid-sync-token`).
pub fn error_names(body: &str, ns: &str, name: &str) -> bool {
    xml::parse(body).is_ok_and(|root| root.is(D, "error") && root.child(ns, name).is_some())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NS_CALENDARSERVER;

    const BODY: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<D:multistatus xmlns:D="DAV:" xmlns:CS="http://calendarserver.org/ns/">
  <D:response>
    <D:href>/cal/work/</D:href>
    <D:propstat>
      <D:prop><D:displayname>Work</D:displayname><CS:getctag>"7"</CS:getctag></D:prop>
      <D:status>HTTP/1.1 200 OK</D:status>
    </D:propstat>
    <D:propstat>
      <D:prop><D:sync-token/></D:prop>
      <D:status>HTTP/1.1 404 Not Found</D:status>
    </D:propstat>
  </D:response>
  <D:response>
    <D:href>/cal/work/gone.ics</D:href>
    <D:status>HTTP/1.1 404 Not Found</D:status>
  </D:response>
  <D:response><D:status>HTTP/1.1 200 OK</D:status></D:response>
  <D:sync-token>tok-1</D:sync-token>
</D:multistatus>"#;

    #[test]
    fn parses_responses_propstats_and_tokens() {
        let ms = parse(BODY).unwrap();
        assert_eq!(ms.sync_token.as_deref(), Some("tok-1"));
        assert_eq!(ms.responses.len(), 2, "a response without href is skipped");
        let first = &ms.responses[0];
        assert_eq!(first.prop_text(D, "displayname").as_deref(), Some("Work"));
        assert_eq!(
            first.prop_text(NS_CALENDARSERVER, "getctag").as_deref(),
            Some("\"7\"")
        );
        assert_eq!(
            first.prop(D, "sync-token"),
            None,
            "404 propstats are ignored"
        );
        assert!(!first.is_not_found());
        assert!(ms.responses[1].is_not_found());
        let base = Url::parse("https://dav.example.org/cal/").unwrap();
        assert_eq!(
            ms.responses[1].url(&base).unwrap().as_str(),
            "https://dav.example.org/cal/work/gone.ics"
        );
    }

    #[test]
    fn rejects_other_documents() {
        assert!(matches!(parse("<a/>"), Err(DavError::Protocol(_))));
        assert!(matches!(parse("<<"), Err(DavError::Protocol(_))));
        assert_eq!(
            parse_status_line("HTTP/1.1 507 Insufficient Storage"),
            Some(507)
        );
        assert_eq!(parse_status_line("garbage"), None);
    }

    #[test]
    fn error_bodies() {
        let body = r#"<D:error xmlns:D="DAV:"><D:valid-sync-token/></D:error>"#;
        assert!(error_names(body, D, "valid-sync-token"));
        assert!(!error_names(body, D, "other"));
        assert!(!error_names("not xml", D, "valid-sync-token"));
    }
}
