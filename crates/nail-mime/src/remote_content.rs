//! Remote content in sanitized HTML → proxy URLs (M2b WP42; empty after
//! WP40, ADR 0036).
//!
//! Expected shape: next to the sanitizer (`sanitize.rs` keeps blocking by
//! default), find every remote reference the sanitizer drops today —
//! `src` / `background` attributes and CSS `url(...)` in `style` — count
//! them (`RemoteContentInfo::resources`) and, for an allowed message,
//! replace each with the URL a signer returns
//! (`nail-proxy:<token>`, [`nail_api::remote_content::PROXY_URL_SCHEME`]).
//! The signer is passed in (a closure / trait implemented by `nail-proxy`),
//! so `nail-mime` stays free of keys and network code. Only `http` /
//! `https` targets are ever rewritten; everything else stays removed.

#[cfg(test)]
mod tests {
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
}
