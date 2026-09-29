//! The address being set up.

/// A parsed email address.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Email {
    /// The whole address as entered (trimmed).
    pub address: String,
    /// Part before the `@`.
    pub local: String,
    /// Domain in lower-case ASCII (IDNs in punycode).
    pub domain: String,
}

impl Email {
    /// Parses `user@domain`. `None` when it is not a plausible address.
    pub fn parse(text: &str) -> Option<Self> {
        let address = text.trim();
        let (local, domain) = address.rsplit_once('@')?;
        if local.is_empty()
            || local.contains(char::is_whitespace)
            || domain.contains(['/', '?', '#', ':', '[', ']'])
        {
            return None;
        }
        let url = url::Url::parse(&format!("http://{domain}/")).ok()?;
        let domain = match url.host()? {
            url::Host::Domain(d) => d.trim_end_matches('.').to_ascii_lowercase(),
            _ => return None,
        };
        if !domain.contains('.') || domain.split('.').any(str::is_empty) {
            return None;
        }
        Some(Self {
            address: address.to_owned(),
            local: local.to_owned(),
            domain,
        })
    }
}

/// The registrable-looking base of a host name: its last two labels
/// (`aspmx.l.google.com` → `google.com`). A heuristic without the public
/// suffix list, used only to look up the ISPDB for an MX host.
pub(crate) fn base_domain(host: &str) -> String {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    let labels: Vec<&str> = host.split('.').collect();
    if labels.len() <= 2 {
        host
    } else {
        labels[labels.len() - 2..].join(".")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_addresses() {
        let e = Email::parse("  Ada.L@Example.ORG ").unwrap();
        assert_eq!(
            (e.address.as_str(), e.local.as_str(), e.domain.as_str()),
            ("Ada.L@Example.ORG", "Ada.L", "example.org")
        );
        assert_eq!(
            Email::parse("jörg@bücher.example").unwrap().domain,
            "xn--bcher-kva.example"
        );
        assert_eq!(
            Email::parse("a\"@b\"@example.org").unwrap().local,
            "a\"@b\""
        );
        for bad in [
            "",
            "ada",
            "@example.org",
            "ada@",
            "ada@localhost",
            "ada@exa mple.org",
            "ada@example..org",
            "ada@1.2.3.4",
            "ada@x.org/evil",
            "a b@x.org",
        ] {
            assert_eq!(Email::parse(bad), None, "{bad}");
        }
    }

    #[test]
    fn base_domains() {
        assert_eq!(base_domain("aspmx.l.google.com."), "google.com");
        assert_eq!(base_domain("example.org"), "example.org");
        assert_eq!(base_domain("MX.Example.org"), "example.org");
    }
}
