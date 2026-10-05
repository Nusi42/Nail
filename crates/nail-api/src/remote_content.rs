//! Remote content (images, fonts, CSS) in mail (design.md, "Reading mail
//! safely"; plan M2b WP42 / WP43; ADR 0036).
//!
//! Remote content is blocked by default. The user allows it per message,
//! per sender (the `From` address) or per sender domain; allowed content is
//! loaded **through the core's proxy**, never directly by the reader, so the
//! sender never sees the user's address and the proxy can refuse private,
//! link-local and loopback targets (no SSRF, never an open proxy).
//!
//! * [`crate::MessageBody::remote_content`] says whether a message has
//!   remote content and whether (and why) it is allowed.
//! * [`crate::Request::AllowRemoteContent`] stores an allow-list entry;
//!   [`crate::Request::ListRemoteContentAllows`] /
//!   [`crate::Request::RemoveRemoteContentAllow`] manage the lists.
//! * When a message is allowed, its `sanitized_html` references each remote
//!   resource as `nail-proxy:<token>` ([`PROXY_URL_SCHEME`]): a token
//!   signed by the core, bound to the message and the original URL, and
//!   short-lived. The reader resolves the scheme to the transport's proxy
//!   endpoint (web mode: `GET` [`PROXY_PATH`]`<token>`); nothing else can
//!   produce a valid token.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Scheme of proxied resource URLs in `sanitized_html`.
pub const PROXY_URL_SCHEME: &str = "nail-proxy:";

/// Route prefix of the remote-content proxy in web mode (`GET
/// /api/proxy/<token>`).
pub const PROXY_PATH: &str = "/api/proxy/";

/// Route prefix of mail documents on the remote UI's sandbox origin (`GET
/// /render/<token>`, see [`crate::MessageBody::render_url`]). An optional
/// `css` query parameter carries the reader stylesheet.
pub const RENDER_PATH: &str = "/render/";

/// Query parameter of a [`RENDER_PATH`] request naming the app origin that
/// frames the document (P3, ADR 0164): the sandbox origin answers only for
/// one of its `frame-ancestors` origins and posts the document's height
/// there.
pub const RENDER_PARENT_PARAM: &str = "parent";

/// `type` of the only message a sandbox-origin mail document posts to its
/// parent (P3, ADR 0164): `{"type": "nail-frame-height", "height": <CSS
/// px>}`. The parent accepts it only from its own frame's window and the
/// sandbox origin, and clamps the height to [`MAX_FRAME_HEIGHT`].
pub const FRAME_HEIGHT_MESSAGE: &str = "nail-frame-height";

/// Tallest mail frame the reader sizes to its content (CSS px); a taller
/// document scrolls inside the frame.
pub const MAX_FRAME_HEIGHT: u32 = 200_000;

/// What an allow-list entry covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum RemoteContentScope {
    /// One message (by its id).
    Message,
    /// Every message from one address (lowercase).
    Sender,
    /// Every message from one domain (lowercase, the `From` address's
    /// domain exactly; subdomains are separate entries).
    Domain,
    /// One message, this time only (M2b follow-up, ADR 0048): not stored
    /// in the allow-lists; lapses after [`ONCE_ALLOW_SECS`] or when Nail
    /// restarts. The value is the message id.
    Once,
}

/// How long a [`RemoteContentScope::Once`] allowance lasts (long enough to
/// read the message; the reader asks again afterwards).
pub const ONCE_ALLOW_SECS: i64 = 15 * 60;

impl RemoteContentScope {
    /// The stored scopes, most specific first (the order the core checks
    /// them); [`Self::Once`] is never stored.
    pub const ALL: [RemoteContentScope; 3] = [Self::Message, Self::Sender, Self::Domain];
}

/// One entry of the remote-content allow-lists.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct RemoteContentAllow {
    /// What it covers.
    pub scope: RemoteContentScope,
    /// Message id, address or domain (normalized, see
    /// [`normalize_allow_value`]).
    pub value: String,
    /// What to show for it: the subject for a message, else `null`.
    pub label: Option<String>,
    /// When it was allowed (RFC 3339).
    pub created_at: String,
}

/// Remote content of one message, in [`crate::MessageBody::remote_content`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct RemoteContentInfo {
    /// Number of remote resources found (images, backgrounds, fonts, style
    /// sheets).
    pub resources: u32,
    /// Why they are loaded, `null` = blocked.
    pub allowed_by: Option<RemoteContentScope>,
    /// The sender address an allow per sender would use (lowercase),
    /// `null` when the message has no usable `From`.
    pub sender: Option<String>,
    /// The domain an allow per domain would use (lowercase).
    pub domain: Option<String>,
}

/// The canonical form of an allow-list value: message ids unchanged,
/// addresses and domains trimmed and lowercased (a trailing dot of a domain
/// dropped). `None` when the value cannot be an entry of that scope.
pub fn normalize_allow_value(scope: RemoteContentScope, value: &str) -> Option<String> {
    let value = value.trim();
    match scope {
        RemoteContentScope::Message | RemoteContentScope::Once => {
            (!value.is_empty()).then(|| value.to_owned())
        }
        RemoteContentScope::Sender => {
            let (local, domain) = value.rsplit_once('@')?;
            let domain = normalize_domain(domain)?;
            (!local.is_empty() && !local.contains(char::is_whitespace))
                .then(|| format!("{}@{domain}", local.to_lowercase()))
        }
        RemoteContentScope::Domain => normalize_domain(value),
    }
}

/// The domain of an address, normalized for [`RemoteContentScope::Domain`].
pub fn sender_domain(address: &str) -> Option<String> {
    let (_, domain) = address.trim().rsplit_once('@')?;
    normalize_domain(domain)
}

fn normalize_domain(domain: &str) -> Option<String> {
    let domain = domain.trim().trim_end_matches('.').to_lowercase();
    let valid = !domain.is_empty()
        && domain.contains('.')
        && !domain.starts_with('.')
        && domain
            .chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '.')
        && !domain.contains("..");
    valid.then_some(domain)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn values_are_normalized() {
        use RemoteContentScope as S;
        assert_eq!(
            normalize_allow_value(S::Sender, " News@Shop.Example. "),
            Some("news@shop.example".into())
        );
        assert_eq!(
            normalize_allow_value(S::Domain, "Shop.EXAMPLE"),
            Some("shop.example".into())
        );
        assert_eq!(
            normalize_allow_value(S::Message, " m-42 "),
            Some("m-42".into())
        );
        assert_eq!(normalize_allow_value(S::Message, "  "), None);
        assert_eq!(normalize_allow_value(S::Sender, "no-at-sign"), None);
        assert_eq!(normalize_allow_value(S::Sender, "@shop.example"), None);
        assert_eq!(normalize_allow_value(S::Domain, "localhost"), None);
        assert_eq!(normalize_allow_value(S::Domain, "a..b"), None);
        assert_eq!(normalize_allow_value(S::Domain, "evil.example/x"), None);
        assert_eq!(
            sender_domain("Ada <ignored>@Mail.Example.org"),
            Some("mail.example.org".into())
        );
        assert_eq!(sender_domain("nobody"), None);
    }

    #[test]
    fn wire_shapes() {
        assert_eq!(
            serde_json::to_value(RemoteContentScope::Domain).unwrap(),
            json!("domain")
        );
        assert_eq!(RemoteContentScope::ALL[0], RemoteContentScope::Message);
        let info = RemoteContentInfo {
            resources: 3,
            allowed_by: None,
            sender: Some("news@shop.example".into()),
            domain: Some("shop.example".into()),
        };
        let value = serde_json::to_value(&info).unwrap();
        assert_eq!(value["allowed_by"], json!(null));
        assert_eq!(
            serde_json::from_value::<RemoteContentInfo>(value).unwrap(),
            info
        );
        assert!(PROXY_PATH.starts_with("/api/") && PROXY_PATH.ends_with('/'));
        assert!(PROXY_URL_SCHEME.ends_with(':'));
    }
}
