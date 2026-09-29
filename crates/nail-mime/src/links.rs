//! Link and attachment safety → [`nail_api::security::MessageWarning`]
//! (M2a WP33; empty after WP30, ADR 0029).
//!
//! Expected shape: walk the links of the sanitized HTML (document order,
//! the numbering `MessageWarning` uses) or the URLs of the text body and
//! flag display-vs-target mismatches, punycode look-alike domains
//! (`idna` for Unicode ↔ ASCII, `unicode-security` for mixed scripts and
//! confusables) and unusual targets; flag dangerous attachment types
//! (executables, macro documents, disguised names incl. right-to-left
//! override characters, disk images, shortcuts, MIME type mismatches).

#[cfg(test)]
mod tests {
    //! Smoke test of the libraries WP33 builds on (verified by WP30).

    #[test]
    fn idna_and_confusable_detection_work() {
        let (unicode, result) = idna::domain_to_unicode("xn--pple-43d.com");
        assert!(result.is_ok());
        assert_eq!(unicode, "\u{430}pple.com");
        assert_eq!(
            idna::domain_to_ascii("\u{430}pple.com").unwrap(),
            "xn--pple-43d.com"
        );
        use unicode_security::MixedScript;
        assert!("paypal".is_single_script());
        assert!(!"p\u{430}ypal".is_single_script());
        assert_eq!(
            unicode_security::skeleton("p\u{430}ypal").collect::<String>(),
            unicode_security::skeleton("paypal").collect::<String>()
        );
    }
}
