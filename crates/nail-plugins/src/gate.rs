//! The permission gate: every `nail.*` call of a plug-in is checked here
//! against its grants before the core ([`crate::PluginServices`]) is asked.
//!
//! * Grants count only for permissions the manifest declares.
//! * `calendar.read` (M3a) gives the event occurrences of every calendar
//!   the user shows; it is not limited to accounts.
//! * `mail.read` gives headers in the granted accounts (virtual accounts
//!   expanded); a message outside them is reported as not found, so a
//!   plug-in cannot probe for it. `mail.read.body` needs `mail.read` too and
//!   reaches the intersection of both grants; bodies of end-to-end
//!   encrypted messages are always refused.
//! * `network:<host>` allows `nail.http.fetch` to that host only:
//!   `https:` on the default port (`http:` and other ports only for
//!   loopback hosts), no credentials in the URL, a fixed set of methods,
//!   no hop-by-hop / `Host` / `Cookie` headers.
//!
//! A refused call fails with `forbidden` ([`Denied`]) and is logged to the
//! plug-in's log (source `api`).

use nail_api::plugins::{PluginGrant, PluginManifest, PluginPermission};
use nail_api::{AccountId, ApiError, ErrorCode, LocalizedMessage};

use crate::l10n;
use crate::services::AccountScope;

/// A call refused for a missing permission.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Denied {
    /// The API call (`nail.mail.getBody`).
    pub call: String,
    /// The permission it needs.
    pub permission: String,
}

impl Denied {
    /// A refusal of `call` for want of `permission`.
    pub fn new(call: &str, permission: impl ToString) -> Self {
        Self {
            call: call.to_owned(),
            permission: permission.to_string(),
        }
    }

    /// The user-facing text.
    pub fn localized(&self) -> LocalizedMessage {
        LocalizedMessage::new(l10n::API_DENIED)
            .arg("call", self.call.as_str())
            .arg("permission", self.permission.as_str())
    }

    /// As an API error (`forbidden`).
    pub fn to_api_error(&self) -> ApiError {
        ApiError::new(
            ErrorCode::Forbidden,
            format!("{} needs the permission {}", self.call, self.permission),
        )
        .with_localized(self.localized())
    }
}

/// The grants of one plug-in, as the gate uses them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Gate {
    grants: Vec<PluginGrant>,
}

impl Gate {
    /// The effective grants: those of declared permissions; account lists
    /// only on the two mail permissions.
    pub fn new(manifest: &PluginManifest, grants: &[PluginGrant]) -> Self {
        let grants = grants
            .iter()
            .filter(|g| manifest.declares(&g.permission) && !g.permission.is_placeholder())
            .map(|g| PluginGrant {
                permission: g.permission.clone(),
                accounts: g.accounts.clone().filter(|_| g.permission.takes_accounts()),
            })
            .collect();
        Self { grants }
    }

    /// The effective grants.
    pub fn grants(&self) -> &[PluginGrant] {
        &self.grants
    }

    fn grant(&self, permission: &PluginPermission) -> Option<&PluginGrant> {
        self.grants.iter().find(|g| &g.permission == permission)
    }

    /// Whether `permission` is granted.
    pub fn has(&self, permission: &PluginPermission) -> bool {
        self.grant(permission).is_some()
    }

    /// `Ok` when `permission` is granted, else a refusal of `call`.
    pub fn require(&self, permission: &PluginPermission, call: &str) -> Result<(), Denied> {
        if self.has(permission) {
            Ok(())
        } else {
            Err(Denied::new(call, permission))
        }
    }

    fn scope_of(
        grant: &PluginGrant,
        expand: &dyn Fn(&[AccountId]) -> Vec<AccountId>,
    ) -> AccountScope {
        match &grant.accounts {
            None => AccountScope::All,
            Some(accounts) => AccountScope::Only(expand(accounts)),
        }
    }

    /// The accounts `mail.read` reaches.
    pub fn mail_scope(
        &self,
        call: &str,
        expand: &dyn Fn(&[AccountId]) -> Vec<AccountId>,
    ) -> Result<AccountScope, Denied> {
        let grant = self
            .grant(&PluginPermission::MailRead)
            .ok_or_else(|| Denied::new(call, PluginPermission::MailRead))?;
        Ok(Self::scope_of(grant, expand))
    }

    /// The accounts whose bodies may be read (`mail.read` ∩
    /// `mail.read.body`).
    pub fn body_scope(
        &self,
        call: &str,
        expand: &dyn Fn(&[AccountId]) -> Vec<AccountId>,
    ) -> Result<AccountScope, Denied> {
        let read = self.mail_scope(call, expand)?;
        let body = self
            .grant(&PluginPermission::MailReadBody)
            .ok_or_else(|| Denied::new(call, PluginPermission::MailReadBody))?;
        Ok(read.intersect(&Self::scope_of(body, expand)))
    }

    /// The granted `network:` host patterns.
    pub fn network_patterns(&self) -> Vec<String> {
        self.grants
            .iter()
            .filter_map(|g| match &g.permission {
                PluginPermission::Network(p) => Some(p.clone()),
                _ => None,
            })
            .collect()
    }

    /// Checks a `nail.http.fetch` call; the checked parts on success.
    pub fn check_fetch(&self, init: &FetchInit) -> Result<CheckedFetch, FetchRefusal> {
        let invalid = |msg: &str| FetchRefusal::Invalid(msg.to_owned());
        let url = url::Url::parse(&init.url).map_err(|e| invalid(&e.to_string()))?;
        let host = url
            .host_str()
            .ok_or_else(|| invalid("the URL has no host"))?
            .to_owned();
        let loopback = is_loopback(&host);
        match url.scheme() {
            "https" => {}
            "http" if loopback => {}
            other => return Err(invalid(&format!("scheme `{other}` is not allowed"))),
        }
        if !url.username().is_empty() || url.password().is_some() {
            return Err(invalid("credentials in the URL are not allowed"));
        }
        if url.port().is_some() && !loopback {
            return Err(invalid("only the default port is allowed"));
        }
        let patterns = self.network_patterns();
        if !host_allowed(&patterns, &host) {
            return Err(FetchRefusal::Denied(Denied::new(
                "nail.http.fetch",
                format!("network:{host}"),
            )));
        }
        let method = init.method.to_ascii_uppercase();
        if !METHODS.contains(&method.as_str()) {
            return Err(invalid(&format!("method `{}` is not allowed", init.method)));
        }
        let mut headers = Vec::new();
        for (name, value) in &init.headers {
            let lower = name.to_ascii_lowercase();
            let token_ok = !name.is_empty()
                && name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b));
            if !token_ok || value.contains(['\r', '\n', '\0']) {
                return Err(invalid(&format!("header `{name}` is not valid")));
            }
            if FORBIDDEN_HEADERS.contains(&lower.as_str()) || lower.starts_with("proxy-") {
                return Err(invalid(&format!("header `{name}` is not allowed")));
            }
            headers.push((name.clone(), value.clone()));
        }
        if init.body.is_some() && matches!(method.as_str(), "GET" | "HEAD") {
            return Err(invalid("GET and HEAD requests have no body"));
        }
        Ok(CheckedFetch {
            url: url.to_string(),
            method,
            headers,
            allowed_hosts: patterns,
        })
    }
}

/// Methods `nail.http.fetch` allows.
pub const METHODS: &[&str] = &["GET", "HEAD", "POST", "PUT", "PATCH", "DELETE"];

/// Headers a plug-in may not set.
pub const FORBIDDEN_HEADERS: &[&str] = &[
    "host",
    "cookie",
    "cookie2",
    "content-length",
    "connection",
    "keep-alive",
    "transfer-encoding",
    "te",
    "trailer",
    "upgrade",
    "expect",
];

/// What a plug-in passes to `nail.http.fetch`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FetchInit {
    /// The URL.
    pub url: String,
    /// The method (default `GET`).
    pub method: String,
    /// Headers.
    pub headers: Vec<(String, String)>,
    /// The body.
    pub body: Option<Vec<u8>>,
}

/// A fetch that passed the gate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckedFetch {
    /// The normalized URL.
    pub url: String,
    /// The upper-case method.
    pub method: String,
    /// The headers.
    pub headers: Vec<(String, String)>,
    /// The granted host patterns (for redirect checks in the core).
    pub allowed_hosts: Vec<String>,
}

/// Why a fetch was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FetchRefusal {
    /// The host is not granted.
    Denied(Denied),
    /// The request is malformed or not allowed in any case.
    Invalid(String),
}

impl FetchRefusal {
    /// As an API error.
    pub fn to_api_error(&self) -> ApiError {
        match self {
            Self::Denied(d) => d.to_api_error(),
            Self::Invalid(msg) => ApiError::invalid_request(msg.clone()),
        }
    }
}

/// Whether `host` matches one of the `network:` patterns (for the core's
/// redirect checks too).
pub fn host_allowed(patterns: &[String], host: &str) -> bool {
    patterns
        .iter()
        .any(|p| PluginPermission::Network(p.clone()).allows_host(host))
}

fn is_loopback(host: &str) -> bool {
    let host = host.trim_start_matches('[').trim_end_matches(']');
    host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(permissions: &[&str]) -> PluginManifest {
        let json = serde_json::json!({
            "id": "t", "name": "T", "version": "1.0.0", "apiVersion": "1", "main": "i.js",
            "permissions": permissions,
        });
        PluginManifest::parse(json.to_string().as_bytes()).unwrap()
    }

    fn grant(p: &str, accounts: Option<&[&str]>) -> PluginGrant {
        PluginGrant {
            permission: PluginPermission::parse(p).unwrap(),
            accounts: accounts.map(|a| a.iter().map(|x| AccountId::from(*x)).collect()),
        }
    }

    fn no_expand(a: &[AccountId]) -> Vec<AccountId> {
        a.to_vec()
    }

    #[test]
    fn grants_count_only_when_declared() {
        let m = manifest(&["mail.read", "storage", "calendar.read"]);
        let gate = Gate::new(
            &m,
            &[
                grant("mail.read", None),
                grant("ui.dialogs", None),
                grant("calendar.read", None),
                grant("storage", Some(&["work"])),
            ],
        );
        assert!(gate.has(&PluginPermission::MailRead));
        assert!(!gate.has(&PluginPermission::UiDialogs));
        assert!(gate.has(&PluginPermission::CalendarRead));
        assert_eq!(
            gate.grant(&PluginPermission::Storage).unwrap().accounts,
            None
        );
        let denied = gate
            .require(&PluginPermission::UiDialogs, "nail.ui.showDialog")
            .unwrap_err();
        let api = denied.to_api_error();
        assert_eq!(api.code, ErrorCode::Forbidden);
        let localized = api.localized.unwrap();
        assert_eq!(localized.id, l10n::API_DENIED);
        assert_eq!(localized.args["permission"].to_string(), "ui.dialogs");
    }

    #[test]
    fn mail_scopes() {
        let m = manifest(&["mail.read", "mail.read.body"]);
        let gate = Gate::new(
            &m,
            &[
                grant("mail.read", Some(&["work", "home"])),
                grant("mail.read.body", Some(&["work", "all-inboxes"])),
            ],
        );
        let expand = |a: &[AccountId]| -> Vec<AccountId> {
            a.iter()
                .flat_map(|x| {
                    if x.as_str() == "all-inboxes" {
                        vec![AccountId::from("work"), AccountId::from("home")]
                    } else {
                        vec![x.clone()]
                    }
                })
                .collect()
        };
        let read = gate.mail_scope("nail.mail.get", &expand).unwrap();
        assert!(read.contains(&"home".into()) && !read.contains(&"other".into()));
        let body = gate.body_scope("nail.mail.getBody", &expand).unwrap();
        assert!(body.contains(&"work".into()) && body.contains(&"home".into()));

        let headers_only = Gate::new(&m, &[grant("mail.read", None)]);
        assert_eq!(
            headers_only.mail_scope("x", &no_expand).unwrap(),
            AccountScope::All
        );
        let err = headers_only
            .body_scope("nail.mail.getBody", &no_expand)
            .unwrap_err();
        assert_eq!(err.permission, "mail.read.body");
        let nothing = Gate::new(&m, &[]);
        assert_eq!(
            nothing.body_scope("y", &no_expand).unwrap_err().permission,
            "mail.read"
        );
    }

    fn fetch(url: &str) -> FetchInit {
        FetchInit {
            url: url.into(),
            method: "get".into(),
            ..FetchInit::default()
        }
    }

    #[test]
    fn fetch_only_to_granted_hosts() {
        let m = manifest(&[
            "network:api.example.com",
            "network:*.example.org",
            "network:127.0.0.1",
        ]);
        let gate = Gate::new(
            &m,
            &[
                grant("network:api.example.com", None),
                grant("network:*.example.org", None),
                grant("network:127.0.0.1", None),
            ],
        );
        let ok = gate
            .check_fetch(&fetch("https://API.example.com/v1?q=1"))
            .unwrap();
        assert_eq!(ok.url, "https://api.example.com/v1?q=1");
        assert_eq!(ok.method, "GET");
        assert_eq!(ok.allowed_hosts.len(), 3);
        assert!(gate.check_fetch(&fetch("https://a.b.example.org/")).is_ok());
        assert!(gate.check_fetch(&fetch("http://127.0.0.1:8080/x")).is_ok());

        for (url, denied) in [
            ("https://example.org/", true),
            ("https://evil.com/", true),
            ("https://api.example.com.evil.com/", true),
            ("http://api.example.com/", false),
            ("https://api.example.com:8443/", false),
            ("https://user:pw@api.example.com/", false),
            ("file:///etc/passwd", false),
            ("ftp://api.example.com/", false),
            ("not a url", false),
            ("http://localhost/", true),
        ] {
            let refusal = gate.check_fetch(&fetch(url)).unwrap_err();
            assert_eq!(
                matches!(refusal, FetchRefusal::Denied(_)),
                denied,
                "{url}: {refusal:?}"
            );
            let code = refusal.to_api_error().code;
            assert!(matches!(
                code,
                ErrorCode::Forbidden | ErrorCode::InvalidRequest
            ));
        }
    }

    #[test]
    fn fetch_methods_and_headers() {
        let m = manifest(&["network:api.example.com"]);
        let gate = Gate::new(&m, &[grant("network:api.example.com", None)]);
        let with = |method: &str, headers: &[(&str, &str)], body: Option<&[u8]>| FetchInit {
            url: "https://api.example.com/".into(),
            method: method.into(),
            headers: headers
                .iter()
                .map(|(a, b)| (a.to_string(), b.to_string()))
                .collect(),
            body: body.map(<[u8]>::to_vec),
        };
        assert!(
            gate.check_fetch(&with("POST", &[("Authorization", "Bearer x")], Some(b"{}")))
                .is_ok()
        );
        for bad in [
            with("CONNECT", &[], None),
            with("TRACE", &[], None),
            with("GET", &[("Host", "evil.com")], None),
            with("GET", &[("Cookie", "a=b")], None),
            with("GET", &[("Proxy-Authorization", "x")], None),
            with("GET", &[("X-A", "a\r\nb: c")], None),
            with("GET", &[("bad name", "x")], None),
            with("GET", &[], Some(b"x")),
        ] {
            assert!(
                matches!(gate.check_fetch(&bad), Err(FetchRefusal::Invalid(_))),
                "{bad:?}"
            );
        }
        // Without any network grant everything is denied.
        let none = Gate::new(&m, &[]);
        assert!(matches!(
            none.check_fetch(&with("GET", &[], None)),
            Err(FetchRefusal::Denied(_))
        ));
    }

    #[test]
    fn redirect_checks() {
        let patterns = vec!["*.example.org".to_owned()];
        assert!(host_allowed(&patterns, "a.example.org"));
        assert!(!host_allowed(&patterns, "example.org"));
        assert!(!host_allowed(&[], "a.example.org"));
    }
}
