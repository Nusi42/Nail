//! Links the user clicked in a message (ADR 0206; contract in
//! [`nail_api::links`]): `Request::OpenLink`.
//!
//! [`classify`] checks the link: `mailto:` is decoded for the composer
//! (the parser of `mailto:` launches, [`crate::launch_args`]); `http:`,
//! `https:` and `tel:` are parsed and normalized, so the system only ever
//! receives a well-formed URL of one of those schemes, never a path or
//! another protocol. The desktop host opens them; without it (web mode)
//! and for every remote session the UI opens them in a browser tab.

use nail_api::links::{EXTERNAL_LINK_SCHEMES, LinkOutcome, MAILTO_SCHEME, MAX_LINK_BYTES};
use nail_api::{ApiError, LocalizedMessage, Response};

use crate::error::Result;
use crate::service::Service;

/// What a link is, once checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LinkTarget {
    /// Opened outside Nail: the normalized URL.
    External(String),
    /// A `mailto:` link: the composer.
    Compose(nail_api::MailtoLink),
}

fn unsupported_link(message: impl Into<String>) -> ApiError {
    ApiError::invalid_localized(message, "link-unsupported")
}

/// Checks `url` (see the module docs).
pub(crate) fn classify(url: &str) -> std::result::Result<LinkTarget, ApiError> {
    let url = url.trim();
    if url.is_empty() || url.len() > MAX_LINK_BYTES {
        return Err(unsupported_link("empty or oversized link"));
    }
    if let Some((scheme, rest)) = url.split_once(':')
        && scheme.eq_ignore_ascii_case(MAILTO_SCHEME)
    {
        return Ok(LinkTarget::Compose(crate::launch_args::parse_mailto(rest)));
    }
    let parsed = url::Url::parse(url).map_err(|e| unsupported_link(format!("bad link: {e}")))?;
    if !EXTERNAL_LINK_SCHEMES.contains(&parsed.scheme()) {
        return Err(unsupported_link(format!(
            "links of scheme {} are not opened",
            parsed.scheme()
        )));
    }
    // A web link needs a host.
    if parsed.scheme() != "tel" && parsed.host_str().is_none_or(str::is_empty) {
        return Err(unsupported_link("web link without a host"));
    }
    Ok(LinkTarget::External(parsed.into()))
}

fn answer(outcome: LinkOutcome) -> Response {
    Response::LinkOpened { outcome }
}

/// `OpenLink` from the desktop UI or web mode: the desktop host opens
/// external links; without one the UI does.
pub(crate) fn open(service: &Service, url: &str) -> Result<Response> {
    Ok(answer(match classify(url)? {
        LinkTarget::Compose(mailto) => LinkOutcome::Compose { mailto },
        LinkTarget::External(url) => match service.inner.desktop.as_ref() {
            Some(host) => {
                host.open_link(&url).map_err(|e| {
                    tracing::warn!(error = %e, "cannot open a link");
                    ApiError::internal(format!("cannot open the link: {e}"))
                        .with_localized(LocalizedMessage::new("link-open-failed"))
                })?;
                LinkOutcome::Opened
            }
            None => LinkOutcome::OpenInBrowser { url },
        },
    }))
}

/// `OpenLink` from a remote session: never the desktop's browser.
pub(crate) fn open_remote(url: &str) -> Result<Response> {
    Ok(answer(match classify(url)? {
        LinkTarget::Compose(mailto) => LinkOutcome::Compose { mailto },
        LinkTarget::External(url) => LinkOutcome::OpenInBrowser { url },
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use nail_api::ErrorCode;
    use pretty_assertions::assert_eq;

    fn external(url: &str) -> String {
        match classify(url).unwrap() {
            LinkTarget::External(url) => url,
            other @ LinkTarget::Compose(_) => panic!("{other:?}"),
        }
    }

    #[test]
    fn web_and_phone_links_are_normalized() {
        assert_eq!(
            external("https://track.example.net/e/abc/click"),
            "https://track.example.net/e/abc/click"
        );
        assert_eq!(external("  HTTP://Example.COM"), "http://example.com/");
        assert_eq!(
            external("https://example.com/a b?q=<x>"),
            "https://example.com/a%20b?q=%3Cx%3E"
        );
        assert_eq!(external("tel:+1-555-0100"), "tel:+1-555-0100");
    }

    #[test]
    fn mailto_links_go_to_the_composer() {
        let LinkTarget::Compose(link) =
            classify("MAILTO:ada@example.com?subject=Hello%20there&cc=b@example.org").unwrap()
        else {
            panic!("not a mailto link");
        };
        assert_eq!(link.to, ["ada@example.com"]);
        assert_eq!(link.cc, ["b@example.org"]);
        assert_eq!(link.subject.as_deref(), Some("Hello there"));
    }

    #[test]
    fn other_links_are_refused() {
        for url in [
            "",
            "javascript:alert(1)",
            "file:///C:/Windows/notepad.exe",
            "smb://host.example/share",
            "ms-settings:defaultapps",
            "C:\\Windows\\notepad.exe",
            "https://",
            "/relative/path",
            "nail-proxy:token",
        ] {
            let err = classify(url).unwrap_err();
            assert_eq!(err.code, ErrorCode::InvalidRequest, "{url}");
            assert_eq!(
                err.localized.as_ref().map(|l| l.id.as_str()),
                Some("link-unsupported"),
                "{url}"
            );
        }
        let long = format!("https://example.com/{}", "a".repeat(MAX_LINK_BYTES));
        assert!(classify(&long).is_err());
    }

    #[test]
    fn remote_sessions_always_open_in_their_browser() {
        let Response::LinkOpened { outcome } = open_remote("https://example.com/x").unwrap() else {
            panic!("not LinkOpened");
        };
        assert_eq!(
            outcome,
            LinkOutcome::OpenInBrowser {
                url: "https://example.com/x".into()
            }
        );
        let Response::LinkOpened { outcome } = open_remote("mailto:a@example.com").unwrap() else {
            panic!("not LinkOpened");
        };
        assert!(matches!(outcome, LinkOutcome::Compose { .. }));
    }
}
