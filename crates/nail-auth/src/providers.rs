//! OAuth2 providers: the built-in Google and Microsoft endpoints and mail
//! scopes, merged with the `oauth-provider` entries of `accounts.kdl`
//! (client ids are never compiled in; registering the apps is an owner
//! task, see `docs/plan.md`).

use nail_api::accounts::{MailProtocol, OAuthProviderConfig, SecretRef};

use crate::error::AuthError;

/// Id of the built-in Google provider.
pub const GOOGLE: &str = "google";
/// Id of the built-in Microsoft provider (Outlook.com and Microsoft 365).
pub const MICROSOFT: &str = "microsoft";

/// One provider's endpoints, client registration and scopes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderSpec {
    /// Provider id.
    pub id: String,
    /// Authorization endpoint.
    pub auth_url: String,
    /// Token endpoint.
    pub token_url: String,
    /// Registered client id; `None` until configured.
    pub client_id: Option<String>,
    /// Keychain reference of the client secret, if the provider issues one
    /// to installed apps (Google does).
    pub client_secret: Option<SecretRef>,
    /// Scopes requested at sign-in.
    pub scopes: Vec<String>,
    /// Extra authorization request parameters.
    pub extra_auth_params: Vec<(String, String)>,
}

/// The mail scopes of a built-in provider for the given protocols. Unknown
/// providers have none (their scopes come from the configuration).
///
/// Microsoft issues tokens per resource: Graph scopes (M3c) and the
/// Outlook IMAP / POP / SMTP / EWS scopes cannot be asked for in one
/// sign-in, so an Exchange account signs in for its engine's protocol
/// only (`[MailProtocol::Graph]` or `[MailProtocol::Ews]`).
pub fn mail_scopes(provider: &str, protocols: &[MailProtocol]) -> Vec<String> {
    let mut scopes: Vec<String> = Vec::new();
    match provider {
        // One scope covers IMAP, POP3, SMTP and the Gmail API (M3b).
        GOOGLE => scopes.push(nail_api::gmail::GMAIL_SCOPE.to_owned()),
        MICROSOFT => {
            for protocol in protocols {
                // The Gmail API is Google's only.
                let scope = match protocol {
                    MailProtocol::Imap => "https://outlook.office.com/IMAP.AccessAsUser.All",
                    MailProtocol::Pop3 => "https://outlook.office.com/POP.AccessAsUser.All",
                    MailProtocol::Smtp => "https://outlook.office.com/SMTP.Send",
                    MailProtocol::Ews => nail_api::exchange::EWS_OAUTH_SCOPE,
                    MailProtocol::Graph => {
                        scopes.extend(
                            nail_api::exchange::GRAPH_SCOPES
                                .iter()
                                .filter(|s| **s != "offline_access")
                                .map(|s| (*s).to_owned()),
                        );
                        continue;
                    }
                    MailProtocol::Gmail => continue,
                };
                scopes.push(scope.to_owned());
            }
            // Required to receive a refresh token.
            scopes.push("offline_access".to_owned());
        }
        _ => {}
    }
    scopes.dedup();
    scopes
}

/// The built-in provider `id` without a client id, or `None`.
pub fn builtin(id: &str) -> Option<ProviderSpec> {
    let all = [MailProtocol::Imap, MailProtocol::Pop3, MailProtocol::Smtp];
    match id {
        GOOGLE => Some(ProviderSpec {
            id: GOOGLE.to_owned(),
            auth_url: "https://accounts.google.com/o/oauth2/v2/auth".to_owned(),
            token_url: "https://oauth2.googleapis.com/token".to_owned(),
            client_id: None,
            client_secret: None,
            scopes: mail_scopes(GOOGLE, &all),
            // Ask for a refresh token even when the user granted access before.
            extra_auth_params: vec![
                ("access_type".to_owned(), "offline".to_owned()),
                ("prompt".to_owned(), "consent".to_owned()),
            ],
        }),
        MICROSOFT => Some(ProviderSpec {
            id: MICROSOFT.to_owned(),
            auth_url: "https://login.microsoftonline.com/common/oauth2/v2.0/authorize".to_owned(),
            token_url: "https://login.microsoftonline.com/common/oauth2/v2.0/token".to_owned(),
            client_id: None,
            client_secret: None,
            scopes: mail_scopes(MICROSOFT, &all),
            extra_auth_params: vec![("prompt".to_owned(), "select_account".to_owned())],
        }),
        _ => None,
    }
}

/// The provider whose OAuth2 sign-in a mail host uses, by host name
/// (`imap.gmail.com` → `google`, `outlook.office365.com` → `microsoft`).
pub fn provider_for_host(host: &str) -> Option<&'static str> {
    if nail_api::hosts::is_google_mail_host(host) {
        Some(GOOGLE)
    } else if nail_api::hosts::is_microsoft_mail_host(host) {
        Some(MICROSOFT)
    } else {
        None
    }
}

/// All known providers: the built-ins plus the configured ones.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderRegistry {
    providers: Vec<ProviderSpec>,
}

impl Default for ProviderRegistry {
    fn default() -> Self {
        Self {
            providers: [GOOGLE, MICROSOFT]
                .iter()
                .filter_map(|id| builtin(id))
                .collect(),
        }
    }
}

fn check_url(provider: &str, field: &str, value: &str) -> Result<(), AuthError> {
    let url = url::Url::parse(value).map_err(|e| {
        AuthError::Config(format!(
            "{field} of provider {provider:?} is not a URL: {e}"
        ))
    })?;
    let loopback = matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "[::1]"));
    if url.scheme() != "https" && !(url.scheme() == "http" && loopback) {
        return Err(AuthError::Config(format!(
            "{field} of provider {provider:?} must use https"
        )));
    }
    Ok(())
}

impl ProviderRegistry {
    /// The built-ins merged with `configured` (`AccountsConfig::oauth_providers`).
    /// A configured built-in sets its client id and secret and may override
    /// scopes and endpoints; any other provider needs both endpoints.
    /// Endpoints must be `https` (plain `http` only on loopback, for tests).
    pub fn from_config(configured: &[OAuthProviderConfig]) -> Result<Self, AuthError> {
        let mut registry = Self::default();
        for config in configured {
            let mut spec = match registry.providers.iter().position(|p| p.id == config.id) {
                Some(index) => registry.providers.remove(index),
                None => ProviderSpec {
                    id: config.id.clone(),
                    auth_url: config.auth_url.clone().ok_or_else(|| {
                        AuthError::Config(format!("provider {:?} needs auth-url", config.id))
                    })?,
                    token_url: config.token_url.clone().ok_or_else(|| {
                        AuthError::Config(format!("provider {:?} needs token-url", config.id))
                    })?,
                    client_id: None,
                    client_secret: None,
                    scopes: Vec::new(),
                    extra_auth_params: Vec::new(),
                },
            };
            if let Some(url) = &config.auth_url {
                spec.auth_url.clone_from(url);
            }
            if let Some(url) = &config.token_url {
                spec.token_url.clone_from(url);
            }
            check_url(&spec.id, "auth-url", &spec.auth_url)?;
            check_url(&spec.id, "token-url", &spec.token_url)?;
            if !config.client_id.trim().is_empty() {
                spec.client_id = Some(config.client_id.trim().to_owned());
            }
            spec.client_secret.clone_from(&config.client_secret);
            if !config.scopes.is_empty() {
                spec.scopes.clone_from(&config.scopes);
            }
            registry.providers.push(spec);
        }
        Ok(registry)
    }

    /// The provider `id`, configured or not.
    pub fn get(&self, id: &str) -> Option<&ProviderSpec> {
        self.providers.iter().find(|p| p.id == id)
    }

    /// The provider `id` if it can be used (has a client id).
    pub fn ready(&self, id: &str) -> Result<&ProviderSpec, AuthError> {
        let spec = self
            .get(id)
            .ok_or_else(|| AuthError::Config(format!("unknown OAuth2 provider {id:?}")))?;
        if spec.client_id.is_none() {
            return Err(AuthError::Config(format!(
                "no client id configured for OAuth2 provider {id:?} (oauth-provider in accounts.kdl)"
            )));
        }
        Ok(spec)
    }

    /// Every provider.
    pub fn providers(&self) -> &[ProviderSpec] {
        &self.providers
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn provider(id: &str) -> OAuthProviderConfig {
        OAuthProviderConfig {
            id: id.into(),
            client_id: "cid".into(),
            client_secret: None,
            auth_url: None,
            token_url: None,
            scopes: vec![],
        }
    }

    #[test]
    fn builtins_need_a_client_id() {
        let registry = ProviderRegistry::default();
        assert!(registry.get(GOOGLE).is_some());
        assert_eq!(
            registry.ready(GOOGLE).unwrap_err().code(),
            nail_api::ErrorCode::Config
        );
        assert!(registry.ready("yahoo").is_err());
        let registry = ProviderRegistry::from_config(&[provider(GOOGLE)]).unwrap();
        let google = registry.ready(GOOGLE).unwrap();
        assert_eq!(google.client_id.as_deref(), Some("cid"));
        assert_eq!(google.scopes, ["https://mail.google.com/"]);
        assert!(
            google
                .extra_auth_params
                .contains(&("access_type".into(), "offline".into()))
        );
        assert_eq!(registry.providers().len(), 2);
    }

    #[test]
    fn microsoft_scopes_per_protocol() {
        assert_eq!(
            mail_scopes(MICROSOFT, &[MailProtocol::Imap, MailProtocol::Smtp]),
            [
                "https://outlook.office.com/IMAP.AccessAsUser.All",
                "https://outlook.office.com/SMTP.Send",
                "offline_access"
            ]
        );
        assert_eq!(
            mail_scopes(GOOGLE, &[MailProtocol::Pop3]),
            ["https://mail.google.com/"]
        );
        assert_eq!(mail_scopes("other", &[MailProtocol::Imap]).len(), 0);
        assert_eq!(
            mail_scopes(GOOGLE, &[MailProtocol::Gmail]),
            ["https://mail.google.com/"]
        );
        assert_eq!(
            mail_scopes(MICROSOFT, &[MailProtocol::Gmail]),
            ["offline_access"]
        );
        // M3c: Exchange through Graph or EWS.
        let graph = mail_scopes(MICROSOFT, &[MailProtocol::Graph]);
        assert!(graph.contains(&"https://graph.microsoft.com/Mail.ReadWrite".to_owned()));
        assert!(graph.contains(&"https://graph.microsoft.com/Tasks.ReadWrite".to_owned()));
        assert_eq!(graph.last().map(String::as_str), Some("offline_access"));
        assert_eq!(graph.iter().filter(|s| *s == "offline_access").count(), 1);
        assert_eq!(
            mail_scopes(MICROSOFT, &[MailProtocol::Ews]),
            [
                "https://outlook.office365.com/EWS.AccessAsUser.All",
                "offline_access"
            ]
        );
        let ms = builtin(MICROSOFT).unwrap();
        assert!(
            ms.scopes
                .iter()
                .any(|s| s.ends_with("POP.AccessAsUser.All"))
        );
        assert!(
            ms.token_url
                .starts_with("https://login.microsoftonline.com/")
        );
    }

    #[test]
    fn custom_providers_and_overrides() {
        let custom = OAuthProviderConfig {
            auth_url: Some("https://id.example.org/authorize".into()),
            token_url: Some("https://id.example.org/token".into()),
            scopes: vec!["mail".into()],
            ..provider("example")
        };
        let local = OAuthProviderConfig {
            auth_url: Some("http://127.0.0.1:9/authorize".into()),
            token_url: Some("http://127.0.0.1:9/token".into()),
            ..provider(MICROSOFT)
        };
        let registry = ProviderRegistry::from_config(&[custom, local]).unwrap();
        assert_eq!(registry.ready("example").unwrap().scopes, ["mail"]);
        let ms = registry.ready(MICROSOFT).unwrap();
        assert_eq!(ms.token_url, "http://127.0.0.1:9/token");
        assert!(ms.scopes.contains(&"offline_access".to_owned()));

        let missing = ProviderRegistry::from_config(&[provider("example")]).unwrap_err();
        assert!(matches!(missing, AuthError::Config(ref m) if m.contains("auth-url")));
        let insecure = OAuthProviderConfig {
            auth_url: Some("http://id.example.org/authorize".into()),
            token_url: Some("https://id.example.org/token".into()),
            ..provider("example")
        };
        assert!(ProviderRegistry::from_config(&[insecure]).is_err());
    }

    #[test]
    fn hosts_map_to_providers() {
        assert_eq!(provider_for_host("imap.gmail.com"), Some(GOOGLE));
        assert_eq!(provider_for_host("SMTP.Gmail.com."), Some(GOOGLE));
        assert_eq!(provider_for_host("outlook.office365.com"), Some(MICROSOFT));
        assert_eq!(provider_for_host("smtp-mail.outlook.com"), Some(MICROSOFT));
        assert_eq!(provider_for_host("graph.microsoft.com"), Some(MICROSOFT));
        assert_eq!(provider_for_host("notgmail.com"), None);
        assert_eq!(provider_for_host("imap.example.org"), None);
    }
}
