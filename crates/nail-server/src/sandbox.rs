//! The sandbox origin: mail HTML on a separate port.
//!
//! Mail HTML must never run on the app's origin. The sandbox origin is the
//! same host on another port — a different origin, so a mail document can
//! read nothing of the app (DOM, storage, WebSocket, blobs) — and it serves
//! only two capability routes:
//!
//! * `GET /render/<token>`: a message's sanitized HTML, for a render token
//!   the core signed ([`nail_proxy::Purpose::Render`]) for a remote
//!   session; the request must carry that session's cookies (browsers send
//!   the app's cookies to the same host on another port). The response's
//!   CSP carries `sandbox` without `allow-same-origin`: the document gets
//!   an **opaque** origin, so its requests are cross-site and carry no
//!   `SameSite` cookies; it may be framed only by the app's origins
//!   (`frame-ancestors`). `nail-proxy:` URLs in the HTML point to this
//!   origin's proxy route. Since P3 (ADR 0164) the sandbox has
//!   `allow-scripts`, but `script-src` allows exactly one script by its
//!   hash: [`HEIGHT_SCRIPT`], which posts the document's height to the app
//!   origin named in the `parent` query parameter (one of the
//!   `frame-ancestors`; any other answers 400). No mail script, inline
//!   handler or `javascript:` URL runs.
//! * `GET /api/proxy/<token>`: the remote-content proxy.
//!
//! No WebSocket, no blobs, no login, no UI bundle; cookies sent to it are
//! only compared with the session a render token names (never touched,
//! rotated or set). `Host` is checked as on the app origin (DNS
//! rebinding).

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, LazyLock};

use axum::Router;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, HeaderName, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use nail_api::remote_content::{PROXY_PATH, PROXY_URL_SCHEME};
use nail_api::{MessageId, Request, Response as ApiResponse};
use nail_core::Service;
use nail_proxy::{Proxy, Purpose};
use sha2::{Digest, Sha256};

/// Route prefix of mail documents on the sandbox origin.
pub const RENDER_PATH: &str = nail_api::remote_content::RENDER_PATH;

/// Longest reader stylesheet accepted in the `css` query parameter.
pub const MAX_CSS_LEN: usize = 16 * 1024;

/// A boxed HTML lookup.
pub type HtmlLookup<'a> = Pin<Box<dyn Future<Output = Option<String>> + Send + 'a>>;

/// Where the sandbox gets a message's sanitized HTML.
pub trait MailSource: Send + Sync + 'static {
    /// The sanitized HTML of `message_id` as the reader renders it
    /// (`MessageBody::reader_html`, else `sanitized_html`), `None` for
    /// text-only or unknown messages.
    fn sanitized_html<'a>(&'a self, message_id: &'a str) -> HtmlLookup<'a>;

    /// Whether the request's cookies belong to the live remote session
    /// `session_id` (the session a render token was minted for).
    fn session_matches(&self, session_id: &str, headers: &HeaderMap) -> bool;
}

impl MailSource for Service {
    fn sanitized_html<'a>(&'a self, message_id: &'a str) -> HtmlLookup<'a> {
        Box::pin(async move {
            let request = Request::GetMessage {
                id: MessageId::from(message_id),
            };
            match self.handle(request).await {
                // The reader's form (inline images embedded) when there is one.
                ApiResponse::Message { message } => message.reader_html.or(message.sanitized_html),
                _ => None,
            }
        })
    }

    fn session_matches(&self, session_id: &str, headers: &HeaderMap) -> bool {
        let cookies = nail_remote_auth::Cookies::from_headers(headers);
        self.remote_auth()
            .session_id_of(&cookies)
            .is_some_and(|id| id.as_str() == session_id)
    }
}

#[derive(Clone)]
pub(crate) struct SandboxState {
    pub(crate) proxy: Arc<Proxy>,
    pub(crate) source: Arc<dyn MailSource>,
    /// `frame-ancestors` sources: the app's origins.
    pub(crate) frame_ancestors: String,
    /// `http` / `https`.
    pub(crate) scheme: &'static str,
}

/// The only script a sandbox document runs (P3, ADR 0164), allowed by its
/// hash ([`height_script_source`]). It reads the app origin from the
/// document's `parent` query parameter
/// ([`nail_api::remote_content::RENDER_PARENT_PARAM`]) and posts
/// `{type: "nail-frame-height", height}`
/// ([`nail_api::remote_content::FRAME_HEIGHT_MESSAGE`]) there — after
/// `load`, after each image's `load` / `error`, on `<details>` toggles,
/// when fonts arrive, on resize and whenever the root or body resizes; only
/// when the height changed. The arithmetic is the reader's
/// (`ui/src/theme/frameHeight.ts`, `contentHeight`): the overflow when the
/// document overflows the frame, else the root's own height, plus a
/// horizontal scrollbar. It touches nothing else (an opaque origin has no
/// cookies or storage anyway). Changing one byte changes its hash; the CSP
/// follows automatically.
pub const HEIGHT_SCRIPT: &str = concat!(
    "(function(){\"use strict\";",
    "var t=new URLSearchParams(location.search).get(\"parent\");if(!t)return;",
    "var d=document.documentElement,l=-1;",
    "function m(){var v=d.clientHeight,s=d.scrollHeight;",
    "var h=Math.min(Math.ceil((s>v?s:d.offsetHeight)+Math.max(0,innerHeight-v)),200000);",
    "if(h!==l){l=h;parent.postMessage({type:\"nail-frame-height\",height:h},t)}}",
    "addEventListener(\"load\",m);addEventListener(\"resize\",m);",
    "[\"load\",\"error\",\"toggle\"].forEach(function(e){document.addEventListener(e,m,true)});",
    "var r=typeof ResizeObserver===\"function\"?new ResizeObserver(m):null;if(r)r.observe(d);",
    "document.addEventListener(\"DOMContentLoaded\",function(){if(r&&document.body)r.observe(document.body);m()});",
    "if(document.fonts)document.fonts.ready.then(m)",
    "})();"
);

static HEIGHT_SCRIPT_SOURCE: LazyLock<String> = LazyLock::new(|| {
    let digest = Sha256::digest(HEIGHT_SCRIPT.as_bytes());
    format!("'sha256-{}'", STANDARD.encode(digest))
});

/// The CSP source that allows [`HEIGHT_SCRIPT`] (and nothing else):
/// `'sha256-<base64>'`.
pub fn height_script_source() -> &'static str {
    &HEIGHT_SCRIPT_SOURCE
}

/// Whether `parent` is one of the space-separated `frame_ancestors`
/// sources (exactly; the app origins the route may be framed by).
pub fn is_frame_ancestor(parent: &str, frame_ancestors: &str) -> bool {
    !parent.is_empty() && frame_ancestors.split(' ').any(|origin| origin == parent)
}

/// The sandbox document for `html`: `nail-proxy:` URLs resolved to this
/// origin's proxy route, no referrer, and the reader stylesheet `css`
/// (the app's theme; it cannot close its `<style>` element), with the
/// height script ([`HEIGHT_SCRIPT`]) in its head.
pub fn document(html: &str, css: &str) -> String {
    let body = html.replace(PROXY_URL_SCHEME, PROXY_PATH);
    let css = css.replace("</", "<\\/");
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\">\
         <meta name=\"referrer\" content=\"no-referrer\"><style>{css}</style>\
         <script>{HEIGHT_SCRIPT}</script></head>\
         <body>{body}</body></html>"
    )
}

/// The CSP of a sandbox document served as `self_origin`: scripts only by
/// the height script's hash (no `'unsafe-inline'`, no `'unsafe-hashes'`:
/// inline handlers and `javascript:` URLs stay blocked), and a sandbox
/// with `allow-scripts` but never `allow-same-origin` (an opaque origin).
pub fn document_csp(self_origin: &str, frame_ancestors: &str) -> String {
    let script = height_script_source();
    format!(
        "default-src 'none'; script-src {script}; img-src {self_origin} data:; \
         style-src {self_origin} 'unsafe-inline'; \
         font-src {self_origin} data:; media-src 'none'; frame-ancestors {frame_ancestors}; \
         base-uri 'none'; form-action 'none'; \
         sandbox allow-scripts allow-popups allow-popups-to-escape-sandbox"
    )
}

pub(crate) fn router(state: SandboxState) -> Router {
    let proxy = Arc::clone(&state.proxy);
    Router::new()
        .route("/render/{token}", get(render))
        .with_state(state)
        .merge(crate::http::proxy_routes(proxy))
        .fallback(|| async { StatusCode::NOT_FOUND })
}

#[derive(Debug, serde::Deserialize)]
struct RenderQuery {
    css: Option<String>,
    /// [`nail_api::remote_content::RENDER_PARENT_PARAM`]: the app origin
    /// the height script posts to.
    parent: Option<String>,
}

async fn render(
    State(state): State<SandboxState>,
    Path(token): Path<String>,
    axum::extract::Query(query): axum::extract::Query<RenderQuery>,
    headers: HeaderMap,
) -> Response {
    let css = query.css.unwrap_or_default();
    if css.len() > MAX_CSS_LEN {
        return StatusCode::PAYLOAD_TOO_LARGE.into_response();
    }
    // The height goes only to an origin that may frame this document.
    if let Some(parent) = query.parent.as_deref()
        && !is_frame_ancestor(parent, &state.frame_ancestors)
    {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let Ok(grant) = state.proxy.signer().verify(&token, Purpose::Render) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    // Bound to the session it was minted for (M2b follow-up): a leaked
    // render URL is useless without that session's cookies.
    let bound = grant
        .session
        .as_deref()
        .is_some_and(|session| state.source.session_matches(session, &headers));
    if !bound {
        return StatusCode::NOT_FOUND.into_response();
    }
    let Some(html) = state.source.sanitized_html(&grant.message_id).await else {
        return StatusCode::NOT_FOUND.into_response();
    };
    // `Host` was checked by the guard: it is one of this origin's names.
    let host = headers
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");
    let self_origin = format!("{}://{host}", state.scheme);
    let csp = document_csp(&self_origin, &state.frame_ancestors);
    let csp = HeaderValue::from_str(&csp)
        .unwrap_or(HeaderValue::from_static("default-src 'none'; sandbox"));
    (
        [
            (
                header::CONTENT_TYPE,
                HeaderValue::from_static("text/html; charset=utf-8"),
            ),
            (header::CONTENT_SECURITY_POLICY, csp),
            (header::CACHE_CONTROL, HeaderValue::from_static("no-store")),
            // The app (COEP require-corp) embeds this document.
            (
                HeaderName::from_static("cross-origin-resource-policy"),
                HeaderValue::from_static("cross-origin"),
            ),
        ],
        document(&html, &css),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn documents_point_proxy_urls_at_this_origin() {
        let doc = document(
            r#"<p><img src="nail-proxy:abc.def" alt=""></p>"#,
            "body { color: red } </style><script>x</script>",
        );
        assert!(doc.contains("<style>body { color: red } <\\/style><script>x<\\/script></style>"));
        assert!(doc.contains(r#"<img src="/api/proxy/abc.def""#));
        assert!(!doc.contains("nail-proxy:"));
        assert!(doc.starts_with("<!doctype html>"));
        assert!(doc.contains("no-referrer"));
    }

    #[test]
    fn the_document_csp_runs_only_the_height_script_in_an_opaque_origin() {
        let csp = document_csp("https://nail.example:4781", "https://nail.example:4780");
        assert!(csp.starts_with("default-src 'none'"));
        let script_src = csp
            .split(';')
            .map(str::trim)
            .find(|d| d.starts_with("script-src"))
            .unwrap();
        assert_eq!(script_src, format!("script-src {}", height_script_source()));
        assert!(!script_src.contains("unsafe") && !script_src.contains("'self'"));
        assert!(!csp.contains("unsafe-hashes") && !csp.contains("unsafe-eval"));
        let sandbox = csp
            .split(';')
            .find(|d| d.trim().starts_with("sandbox"))
            .unwrap();
        assert!(sandbox.contains("allow-scripts"));
        assert!(!sandbox.contains("allow-same-origin"));
        assert!(!sandbox.contains("allow-forms"));
        assert!(!sandbox.contains("allow-top-navigation "));
        assert!(csp.contains("frame-ancestors https://nail.example:4780;"));
        assert!(csp.contains("img-src https://nail.example:4781 data:;"));
    }

    #[test]
    fn the_height_script_is_the_contracts_and_pinned_by_its_hash() {
        use nail_api::remote_content::{
            FRAME_HEIGHT_MESSAGE, MAX_FRAME_HEIGHT, RENDER_PARENT_PARAM,
        };
        assert!(HEIGHT_SCRIPT.contains(&format!("type:\"{FRAME_HEIGHT_MESSAGE}\"")));
        assert!(HEIGHT_SCRIPT.contains(&format!(".get(\"{RENDER_PARENT_PARAM}\")")));
        assert!(HEIGHT_SCRIPT.contains(&format!(",{MAX_FRAME_HEIGHT})")));
        // It posts to the named parent only, never to `*`.
        assert!(HEIGHT_SCRIPT.contains("parent.postMessage("));
        assert!(!HEIGHT_SCRIPT.contains("\"*\""));
        // Nothing that reads or writes state.
        for forbidden in [
            "cookie",
            "Storage",
            "fetch",
            "XMLHttpRequest",
            "innerHTML",
            "eval",
        ] {
            assert!(!HEIGHT_SCRIPT.contains(forbidden), "{forbidden}");
        }
        // It cannot end its own element.
        assert!(!HEIGHT_SCRIPT.contains("</"));
        let expected = STANDARD.encode(Sha256::digest(HEIGHT_SCRIPT.as_bytes()));
        assert_eq!(height_script_source(), format!("'sha256-{expected}'"));
        let doc = document("<p>Hi</p>", "");
        assert!(doc.contains(&format!("<script>{HEIGHT_SCRIPT}</script></head>")));
        assert_eq!(doc.matches("<script").count(), 1);
    }

    #[test]
    fn only_frame_ancestors_are_parents() {
        let ancestors = "http://127.0.0.1:4780 https://nail.example:4780";
        assert!(is_frame_ancestor("http://127.0.0.1:4780", ancestors));
        assert!(is_frame_ancestor("https://nail.example:4780", ancestors));
        assert!(!is_frame_ancestor("https://evil.example", ancestors));
        assert!(!is_frame_ancestor("http://127.0.0.1:478", ancestors));
        assert!(!is_frame_ancestor("", ancestors));
        assert!(!is_frame_ancestor("*", ancestors));
    }
}
