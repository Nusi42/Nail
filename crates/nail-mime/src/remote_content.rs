//! Remote content in sanitized HTML → proxy URLs (M2b; ADRs 0036, 0038).
//!
//! The sanitizer ([`crate::sanitize`]) finds every remote reference —
//! `src` / `background` attributes and CSS `url(...)` in `style` — and
//! counts it ([`crate::SanitizedHtml::remote_resources`],
//! [`crate::ParsedMessage::remote_resources`]). By default it removes
//! them. For a message whose remote content the user allowed, the core
//! passes a [`ProxyRewriter`] ([`crate::parse_message_with`]): each
//! reference is replaced with what the rewriter returns — a signed
//! `nail-proxy:<token>` URL ([`nail_api::remote_content::PROXY_URL_SCHEME`])
//! minted by `nail-proxy` — and removed when it returns `None`.
//!
//! The rewriter is a closure, so this crate stays free of keys and network
//! code. It only ever sees `http` / `https` targets ([`remote_target`]);
//! everything else stays removed.

use std::sync::Arc;

/// Maps an original `http(s)` URL to its replacement (a `nail-proxy:`
/// URL), `None` to remove it.
pub type ProxyRewriter = Arc<dyn Fn(&str) -> Option<String> + Send + Sync>;

/// The absolute `http` / `https` URL of a remote reference as written in
/// mail: surrounding whitespace removed, protocol-relative `//host/…`
/// read as `https:`. `None` for anything else.
pub fn remote_target(value: &str) -> Option<String> {
    let value = value.trim();
    if value.chars().any(char::is_control) {
        return None;
    }
    let absolute = if value.starts_with("//") {
        format!("https:{value}")
    } else {
        value.to_owned()
    };
    let url = url::Url::parse(&absolute).ok()?;
    (matches!(url.scheme(), "http" | "https") && url.host_str().is_some_and(|h| !h.is_empty()))
        .then(|| url.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sanitize::{sanitize_html, sanitize_html_with};

    fn signer() -> ProxyRewriter {
        Arc::new(|url: &str| {
            (!url.contains("refuse")).then(|| {
                let token: String = url
                    .bytes()
                    .map(|b| {
                        if b.is_ascii_alphanumeric() {
                            b as char
                        } else {
                            '_'
                        }
                    })
                    .collect();
                format!("{}{token}", nail_api::remote_content::PROXY_URL_SCHEME)
            })
        })
    }

    #[test]
    fn proxy_scheme_is_a_url_scheme() {
        let url = format!(
            "{}{}",
            nail_api::remote_content::PROXY_URL_SCHEME,
            "dG9rZW4"
        );
        assert!(url.starts_with("nail-proxy:"));
        assert!(!url.contains("//"));
    }

    #[test]
    fn remote_targets_are_absolute_http_urls() {
        assert_eq!(
            remote_target(" https://x.example/a.png "),
            Some("https://x.example/a.png".into())
        );
        assert_eq!(
            remote_target("//cdn.example/p.gif"),
            Some("https://cdn.example/p.gif".into())
        );
        assert_eq!(
            remote_target("HTTP://X.example"),
            Some("http://x.example/".into())
        );
        for refused in [
            "ftp://x.example/a",
            "javascript:alert(1)",
            "data:image/png;base64,AAAA",
            "cid:a@b",
            "/relative",
            "https://x.example/\u{0}a",
        ] {
            assert_eq!(remote_target(refused), None, "{refused}");
        }
    }

    #[test]
    fn remote_references_are_counted_while_blocked() {
        let out = sanitize_html(
            r#"<img src="https://t.example/p.gif"><table background="//x.example/bg.jpg"><tr><td style="background: url('https://x.example/a.png'); color: red">a</td></tr></table><img src="cid:logo">"#,
        );
        assert_eq!(out.remote_resources, 3);
        assert!(out.remote_content_blocked);
        assert!(!out.html.contains("example"));
        assert!(out.html.contains("color: red"));
    }

    #[test]
    fn allowed_content_is_rewritten_to_proxy_urls() {
        let out = sanitize_html_with(
            r#"<img src="https://t.example/p.gif" alt="p"><table background="//x.example/bg.jpg"><tr><td style="background: url('https://x.example/a.png') no-repeat; color: red">a</td></tr></table><img src="cid:logo">"#,
            Some(signer()),
        );
        assert_eq!(out.remote_resources, 3);
        assert!(!out.remote_content_blocked);
        assert!(
            out.html
                .contains(r#"<img src="nail-proxy:https___t_example_p_gif" alt="p">"#),
            "{}",
            out.html
        );
        assert!(
            out.html
                .contains(r#"background="nail-proxy:https___x_example_bg_jpg""#)
        );
        assert!(
            out.html.contains(
                r#"style="background: url(&quot;nail-proxy:https___x_example_a_png&quot;) no-repeat; color: red""#
            ),
            "{}",
            out.html
        );
        assert!(out.html.contains(r#"src="cid:logo""#));
        assert!(!out.html.contains("https://"));
    }

    #[test]
    fn refused_or_unsafe_targets_stay_removed() {
        let out = sanitize_html_with(
            r#"<img src="https://refuse.example/a.png"><img src="https://ok.example/b.png"><p style="background: url(https://ok.example/c.png), url(javascript:x)">t</p><p style="background: image-set('https://ok.example/d.png' 1x)">u</p>"#,
            Some(signer()),
        );
        assert!(out.remote_content_blocked, "one target was refused");
        assert!(!out.html.contains("refuse"));
        assert!(out.html.contains("nail-proxy:https___ok_example_b_png"));
        assert!(!out.html.contains("javascript"));
        assert!(!out.html.contains("c_png"), "{}", out.html);
        assert!(!out.html.contains("d.png"));
        // A rewriter answer that could break out of the CSS string is dropped.
        let evil: ProxyRewriter = Arc::new(|_: &str| Some("nail-proxy:x\");evil(".into()));
        let out = sanitize_html_with(
            r#"<p style="background: url(https://ok.example/c.png)">t</p>"#,
            Some(evil),
        );
        assert_eq!(out.html, "<p>t</p>");
    }

    #[test]
    fn links_are_not_remote_content() {
        let out = sanitize_html_with(r#"<a href="https://x.example/">x</a>"#, Some(signer()));
        assert_eq!(out.remote_resources, 0);
        assert!(out.html.contains(r#"href="https://x.example/""#));
    }
}
