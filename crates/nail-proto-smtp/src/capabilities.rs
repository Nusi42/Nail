//! The server's `EHLO` reply.

/// Extensions announced in the `EHLO` reply.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Capabilities {
    /// The first word of the first reply line (the server's name).
    pub server_name: String,
    /// Every extension line as sent, e.g. `SIZE 35882577`, `AUTH PLAIN LOGIN`.
    pub extensions: Vec<String>,
}

impl Capabilities {
    /// Parses the lines of an `EHLO` reply (without reply codes).
    pub fn from_lines<'a>(lines: impl IntoIterator<Item = &'a str>) -> Self {
        let mut lines = lines.into_iter();
        let server_name = lines
            .next()
            .and_then(|l| l.split_whitespace().next())
            .unwrap_or_default()
            .to_owned();
        let extensions = lines
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(str::to_owned)
            .collect();
        Self {
            server_name,
            extensions,
        }
    }

    fn line(&self, keyword: &str) -> Option<&str> {
        self.extensions.iter().map(String::as_str).find(|l| {
            l.split(|c: char| c.is_whitespace() || c == '=')
                .next()
                .is_some_and(|k| k.eq_ignore_ascii_case(keyword))
        })
    }

    /// Whether the extension `keyword` (e.g. `STARTTLS`, `8BITMIME`) was
    /// announced.
    pub fn has(&self, keyword: &str) -> bool {
        self.line(keyword).is_some()
    }

    /// The `SIZE` limit in bytes (RFC 1870); `None` when absent or 0 (no
    /// fixed limit).
    pub fn size_limit(&self) -> Option<u64> {
        self.line("SIZE")?
            .split_whitespace()
            .nth(1)?
            .parse::<u64>()
            .ok()
            .filter(|n| *n > 0)
    }

    /// Announced SASL mechanisms in upper case (`AUTH PLAIN LOGIN` and the
    /// old `AUTH=PLAIN` form).
    pub fn auth_mechanisms(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for line in &self.extensions {
            // `AUTH PLAIN` and the pre-standard `AUTH=PLAIN`.
            let is_auth = line.get(..5).is_some_and(|p| {
                p.eq_ignore_ascii_case("AUTH ") || p.eq_ignore_ascii_case("AUTH=")
            });
            let Some(rest) = line.get(5..).filter(|_| is_auth) else {
                continue;
            };
            for mechanism in rest.split_whitespace() {
                let mechanism = mechanism.to_ascii_uppercase();
                if !out.contains(&mechanism) {
                    out.push(mechanism);
                }
            }
        }
        out
    }

    /// Whether `mechanism` (upper case) was announced.
    pub fn has_auth(&self, mechanism: &str) -> bool {
        self.auth_mechanisms().iter().any(|m| m == mechanism)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ehlo() {
        let caps = Capabilities::from_lines([
            "smtp.example.org greets you",
            "SIZE 35882577",
            "8BITMIME",
            "AUTH LOGIN PLAIN XOAUTH2",
            "AUTH=LOGIN",
            "dsn",
            "",
            "AUTHé",
        ]);
        assert_eq!(caps.server_name, "smtp.example.org");
        assert_eq!(caps.size_limit(), Some(35_882_577));
        assert!(caps.has("8bitmime"));
        assert!(caps.has("DSN"));
        assert!(!caps.has("STARTTLS"));
        assert!(!caps.has("SI"));
        assert_eq!(caps.auth_mechanisms(), ["LOGIN", "PLAIN", "XOAUTH2"]);
        assert!(caps.has_auth("XOAUTH2"));
        assert_eq!(caps.extensions.len(), 6);
    }

    #[test]
    fn size_zero_or_missing_means_unlimited() {
        assert_eq!(Capabilities::from_lines(["x", "SIZE 0"]).size_limit(), None);
        assert_eq!(Capabilities::from_lines(["x", "SIZE"]).size_limit(), None);
        assert_eq!(Capabilities::from_lines(["x"]).size_limit(), None);
        assert!(
            Capabilities::from_lines(Vec::<&str>::new())
                .auth_mechanisms()
                .is_empty()
        );
    }
}
