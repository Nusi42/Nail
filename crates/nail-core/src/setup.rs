//! Account setup and configuration requests: discovery, connection tests,
//! OAuth2 sign-in, adding / changing / removing accounts (secrets to the
//! keychain, `accounts.kdl` through the format-preserving writer, store
//! rows, engines), identities and virtual accounts.

use std::sync::Arc;

use nail_api::accounts::{
    AccountTestResult, DiscoveredAccount, EndpointSlot, EndpointTestResult, SecretInput,
    SecretValue,
};
use nail_api::engine::EngineError;
use nail_api::{
    Account, AccountConfig, AccountId, AccountsConfig, ApiError, ErrorCode, Event, Identity,
    IncomingServer, OutgoingServer, Response, ServerEndpoint, VirtualAccountConfig,
};
use nail_discovery::Discoverer;
use nail_store::Store;

use crate::accounts::edit_error;
use crate::creds::{EndpointCredentials, SecretSource, imap_config, pop3_config};
use crate::error::Result;
use crate::pop3_engine::pop3_folders;
use crate::service::{Service, lock};

/// Mirrors the configured accounts into the store (rows for foreign keys
/// and listing; POP3 accounts get their local folders). Blocking.
pub(crate) fn sync_store_accounts(
    store: &Store,
    config: &AccountsConfig,
) -> nail_store::Result<()> {
    for account in &config.accounts {
        store.upsert_account(&Account {
            id: account.id.clone(),
            name: account.name.clone(),
            email: account.email.clone(),
            kind: account.kind(),
        })?;
        if matches!(account.incoming, IncomingServer::Pop3 { .. }) {
            for folder in pop3_folders(&account.id) {
                store.upsert_folder(&folder)?;
            }
        }
    }
    Ok(())
}

/// Fills in what the `accounts.kdl` reader would (ADR 0013), so the saved
/// configuration reads back unchanged: the display name defaults to the
/// id, an account without identities gets one from its address, and every
/// identity belongs to the account.
pub(crate) fn normalize_account(config: &mut AccountConfig) {
    if config.name.trim().is_empty() {
        config.name = config.id.to_string();
    }
    config.email = config.email.trim().to_owned();
    if config.identities.is_empty() {
        config.identities.push(Identity {
            id: nail_api::IdentityId::new(config.email.clone()),
            account_id: config.id.clone(),
            email: config.email.clone(),
            name: None,
            reply_to: None,
            bcc: Vec::new(),
            signature: None,
            composer_mode: None,
        });
    }
    for identity in &mut config.identities {
        identity.account_id = config.id.clone();
    }
}

fn test_result(result: std::result::Result<Vec<String>, EngineError>) -> EndpointTestResult {
    match result {
        Ok(capabilities) => EndpointTestResult {
            error: None,
            message: None,
            capabilities,
        },
        Err(err) => EndpointTestResult {
            error: Some(err.code),
            message: Some(err.message),
            capabilities: Vec::new(),
        },
    }
}

impl Service {
    /// Mirrors the configuration into the store (blocking; at start-up).
    pub(crate) fn sync_store_accounts_blocking(&self) -> Result<()> {
        sync_store_accounts(&self.inner.store, &self.inner.accounts.config())?;
        Ok(())
    }

    async fn sync_store_accounts(&self) -> Result<()> {
        let config = self.inner.accounts.config();
        self.inner
            .store
            .call(move |s| sync_store_accounts(s, &config))
            .await?;
        Ok(())
    }

    /// Applies a changed configuration: store rows, OAuth2 providers,
    /// engines, and tells clients.
    pub(crate) async fn accounts_changed(&self) -> Result<()> {
        self.sync_store_accounts().await?;
        self.inner
            .tokens
            .reconfigure(&self.inner.accounts.config().oauth_providers);
        if self
            .inner
            .started
            .load(std::sync::atomic::Ordering::Relaxed)
        {
            self.reconcile_engines();
        }
        self.broadcast(Event::AccountsChanged);
        Ok(())
    }

    fn discoverer(&self) -> Result<Arc<Discoverer>> {
        let mut slot = lock(&self.inner.discoverer);
        if let Some(d) = slot.as_ref() {
            return Ok(Arc::clone(d));
        }
        let http = nail_auth::http_client()
            .map_err(|e| ApiError::internal(format!("HTTP client: {e}")))?;
        let discoverer =
            Arc::new(Discoverer::system(http).map_err(|e| ApiError::new(e.code(), e.to_string()))?);
        *slot = Some(Arc::clone(&discoverer));
        Ok(discoverer)
    }

    /// `DiscoverAccount`.
    pub(crate) async fn discover(&self, email: &str) -> Result<Vec<DiscoveredAccount>> {
        let discoverer = self.discoverer()?;
        discoverer
            .discover(email)
            .await
            .map_err(|e| ApiError::new(e.code(), e.to_string()).into())
    }

    /// Where a test or save gets an endpoint's secret: the request's
    /// secrets (the incoming one also serves the outgoing server when it
    /// has none of its own and uses the same method), else the keychain.
    fn secret_source(
        &self,
        slot: EndpointSlot,
        secrets: &[SecretInput],
        endpoint: &ServerEndpoint,
        other: Option<&ServerEndpoint>,
    ) -> Result<SecretSource> {
        let own = secrets.iter().find(|s| s.endpoint == slot);
        let shared = || {
            secrets.iter().find(|s| {
                s.endpoint != slot
                    && other.is_some_and(|o| o.auth.secret_kind() == endpoint.auth.secret_kind())
            })
        };
        match own.or_else(shared) {
            Some(input) => Ok(match &input.value {
                SecretValue::Password { password } => SecretSource::Password(password.clone()),
                SecretValue::OAuth2 { flow_id } => {
                    SecretSource::AccessToken(self.inner.flows.peek(flow_id)?.1.access_token)
                }
                SecretValue::ClientCertificate { .. } => {
                    return Err(
                        ApiError::unsupported("client certificates are not supported yet").into(),
                    );
                }
            }),
            None => Ok(SecretSource::Stored {
                secrets: Arc::clone(&self.inner.secrets),
                tokens: Arc::clone(&self.inner.tokens)
                    as Arc<dyn nail_api::engine::AccessTokenProvider>,
            }),
        }
    }

    /// `TestAccount`: connect and log in to each server, save nothing.
    pub(crate) async fn test_account(
        &self,
        config: &AccountConfig,
        secrets: &[SecretInput],
    ) -> Result<AccountTestResult> {
        let account = if config.id.as_str().is_empty() {
            AccountId::new("test")
        } else {
            config.id.clone()
        };
        let incoming_server = config.incoming.server();
        let outgoing_server = config.outgoing.as_ref().map(OutgoingServer::server);
        let incoming_creds = EndpointCredentials {
            account: account.clone(),
            auth: incoming_server.auth.clone(),
            source: self.secret_source(
                EndpointSlot::Incoming,
                secrets,
                incoming_server,
                outgoing_server,
            )?,
        };
        let incoming = match &config.incoming {
            IncomingServer::Imap { server, options } => {
                let settings = crate::sync::ImapSettings {
                    config: imap_config(server, options),
                    credentials: Arc::new(incoming_creds),
                };
                test_result(
                    async {
                        let client = crate::sync::connect(&settings).await?;
                        let caps = client.capabilities().iter().map(str::to_owned).collect();
                        let _ = client.logout().await;
                        Ok::<_, nail_proto_imap::ImapError>(caps)
                    }
                    .await
                    .map_err(EngineError::from),
                )
            }
            IncomingServer::Pop3 { server, .. } => match incoming_creds.login(false).await {
                Ok(login) => {
                    nail_proto_pop3::test_login(
                        &pop3_config(server),
                        &crate::creds::pop3_credentials(login),
                    )
                    .await
                }
                Err(err) => test_result(Err(err)),
            },
        };
        let outgoing = match (&config.outgoing, outgoing_server) {
            (Some(OutgoingServer::Smtp { .. }), Some(server)) => {
                let creds = EndpointCredentials {
                    account: account.clone(),
                    auth: server.auth.clone(),
                    source: self.secret_source(
                        EndpointSlot::Outgoing,
                        secrets,
                        server,
                        Some(incoming_server),
                    )?,
                };
                let smtp = nail_proto_smtp::SmtpConfig::from_endpoint(server)
                    .with_timeout(crate::creds::NETWORK_TIMEOUT);
                Some(match creds.login(false).await {
                    Ok(login) => {
                        let auth = nail_proto_smtp::SmtpAuth::Credentials(
                            crate::creds::smtp_credentials(login),
                        );
                        nail_proto_smtp::test_login(&smtp, &auth).await
                    }
                    Err(err) => test_result(Err(err)),
                })
            }
            _ => None,
        };
        Ok(AccountTestResult { incoming, outgoing })
    }

    /// `StartOAuth`.
    pub(crate) async fn start_oauth(
        &self,
        provider: String,
        login_hint: Option<String>,
    ) -> Result<Response> {
        let manager = self.inner.tokens.manager();
        let mut options = nail_auth::FlowOptions::new(crate::strings::oauth_callback_pages(
            self.inner.locale.as_deref(),
        ));
        options.login_hint = login_hint;
        let flow = manager.start_flow(&provider, options).await?;
        let flow_id = self.inner.flows.next_id();
        let authorize_url = flow.authorize_url().to_owned();
        self.inner
            .flows
            .started(flow_id.clone(), provider, flow.canceller());
        let service = self.clone();
        let id = flow_id.clone();
        tokio::spawn(async move {
            let result = flow.finish().await;
            let (error, message) = match &result {
                Ok(_) => (None, None),
                Err(err) => (Some(err.code()), Some(err.to_string())),
            };
            service.inner.flows.finished(&id, result.ok());
            service.broadcast(Event::OAuthFinished {
                flow_id: id,
                error,
                message,
            });
        });
        let opened = self.inner.open_browser && open::that_detached(&authorize_url).is_ok();
        Ok(Response::OAuthStarted {
            flow_id,
            authorize_url,
            opened,
        })
    }

    /// Stores the request's secrets for `account` in the keychain.
    async fn store_secrets(&self, account: &AccountConfig, secrets: &[SecretInput]) -> Result<()> {
        for input in secrets {
            let endpoint = match input.endpoint {
                EndpointSlot::Incoming => account.incoming.server(),
                EndpointSlot::Outgoing => match &account.outgoing {
                    Some(outgoing) => outgoing.server(),
                    None => continue,
                },
            };
            let Some(reference) = endpoint.auth.secret_ref(&account.id) else {
                continue;
            };
            match &input.value {
                SecretValue::Password { password } => {
                    let store = Arc::clone(&self.inner.secrets);
                    let password = password.clone();
                    tokio::task::spawn_blocking(move || store.set(&reference, &password)).await??;
                }
                SecretValue::OAuth2 { flow_id } => {
                    let (_, tokens) = self.inner.flows.peek(flow_id)?;
                    self.inner
                        .tokens
                        .manager()
                        .store(&reference, &tokens)
                        .await?;
                }
                SecretValue::ClientCertificate { .. } => {
                    return Err(
                        ApiError::unsupported("client certificates are not supported yet").into(),
                    );
                }
            }
        }
        // Claim the sign-ins that were used.
        for input in secrets {
            if let SecretValue::OAuth2 { flow_id } = &input.value {
                let _ = self.inner.flows.take(flow_id);
            }
        }
        Ok(())
    }

    /// `AddAccount` / `UpdateAccount`.
    pub(crate) async fn save_account(
        &self,
        mut config: AccountConfig,
        secrets: &[SecretInput],
        add: bool,
    ) -> Result<AccountConfig> {
        let current = self.inner.accounts.config();
        if add {
            if config.id.as_str().is_empty() {
                let hint = if config.email.is_empty() {
                    config.name.clone()
                } else {
                    config.email.clone()
                };
                config.id = nail_config::derive_account_id(&hint, |id| {
                    current.accounts.iter().any(|a| a.id.as_str() == id)
                        || current.virtual_accounts.iter().any(|v| v.id.as_str() == id)
                });
            } else if current.account(&config.id).is_some()
                || current.virtual_account(&config.id).is_some()
            {
                return Err(ApiError::new(
                    ErrorCode::Conflict,
                    format!("the name {} is taken", config.id),
                )
                .into());
            }
        } else if current.account(&config.id).is_none() {
            return Err(ApiError::not_found(format!("account {}", config.id)).into());
        }
        normalize_account(&mut config);
        let id = config.id.clone();
        let (_, saved) = self
            .inner
            .accounts
            .edit(|doc| doc.upsert_account(config).map_err(edit_error))?;
        let saved_account = saved
            .account(&id)
            .cloned()
            .ok_or_else(|| ApiError::internal(format!("account {id} vanished")))?;
        self.store_secrets(&saved_account, secrets).await?;
        self.accounts_changed().await?;
        tracing::info!(account = %id, add, "account saved");
        Ok(saved_account)
    }

    /// `RemoveAccount`.
    pub(crate) async fn remove_account(&self, id: &AccountId) -> Result<()> {
        let config = self
            .inner
            .accounts
            .config()
            .account(id)
            .cloned()
            .ok_or_else(|| ApiError::not_found(format!("account {id}")))?;
        self.inner
            .accounts
            .edit(|doc| doc.remove_account(id).map_err(edit_error))?;
        // No engine may write the account's mail while it is deleted.
        self.inner.engines.stop_account(id).await;
        let mut refs = Vec::new();
        refs.extend(config.incoming.server().auth.secret_ref(id));
        if let Some(outgoing) = &config.outgoing {
            refs.extend(outgoing.server().auth.secret_ref(id));
        }
        for reference in refs {
            self.inner.tokens.manager().forget(&reference);
            let store = Arc::clone(&self.inner.secrets);
            let result = tokio::task::spawn_blocking(move || store.delete(&reference)).await?;
            if let Err(err) = result {
                tracing::warn!(%err, "cannot delete a keychain entry");
            }
        }
        let account = id.clone();
        let blobs = self.inner.blobs.clone();
        let folders = self
            .inner
            .store
            .call(move |s| {
                let folders = s.list_folders(&account)?;
                let mut hashes = Vec::new();
                for folder in &folders {
                    hashes.extend(s.folder_blob_hashes(&folder.id)?);
                }
                s.delete_account(&account)?;
                s.remove_unreferenced_blobs(&blobs, &hashes)?;
                Ok(folders.into_iter().map(|f| f.id).collect::<Vec<_>>())
            })
            .await?;
        let index = Arc::clone(&self.inner.search);
        let account = id.clone();
        tokio::task::spawn_blocking(move || index.remove_account(&account)).await??;
        self.accounts_changed().await?;
        self.broadcast(Event::MailChanged {
            folder_ids: folders,
        });
        tracing::info!(account = %id, "account removed");
        Ok(())
    }

    /// `ListIdentities`.
    pub(crate) async fn list_identities(
        &self,
        account: Option<&AccountId>,
    ) -> Result<Vec<Identity>> {
        let config = self.inner.accounts.config();
        let members: Option<Vec<AccountId>> = match account {
            None => None,
            Some(id) => Some(self.expand_account(id).await?),
        };
        let mut out: Vec<Identity> = config
            .accounts
            .iter()
            .filter(|a| members.as_ref().is_none_or(|m| m.contains(&a.id)))
            .flat_map(|a| a.identities.iter().cloned())
            .collect();
        // A virtual account's default identity comes first.
        if let Some(id) = account
            && let Some(va) = config.virtual_account(id)
            && let Some(default) = &va.default_identity
            && let Some(pos) = out.iter().position(|i| &i.id == default)
        {
            let identity = out.remove(pos);
            out.insert(0, identity);
        }
        Ok(out)
    }

    /// `AddVirtualAccount` / `UpdateVirtualAccount`.
    pub(crate) fn save_virtual_account(
        &self,
        mut config: VirtualAccountConfig,
        add: bool,
    ) -> Result<VirtualAccountConfig> {
        let current = self.inner.accounts.config();
        if add {
            if config.id.as_str().is_empty() {
                config.id = nail_config::derive_account_id(&config.name, |id| {
                    current.accounts.iter().any(|a| a.id.as_str() == id)
                        || current.virtual_accounts.iter().any(|v| v.id.as_str() == id)
                });
            } else if current.account(&config.id).is_some()
                || current.virtual_account(&config.id).is_some()
            {
                return Err(ApiError::new(
                    ErrorCode::Conflict,
                    format!("the name {} is taken", config.id),
                )
                .into());
            }
        } else if current.virtual_account(&config.id).is_none() {
            return Err(ApiError::not_found(format!("virtual account {}", config.id)).into());
        }
        let id = config.id.clone();
        let (_, saved) = self
            .inner
            .accounts
            .edit(|doc| doc.upsert_virtual_account(config).map_err(edit_error))?;
        self.broadcast(Event::AccountsChanged);
        saved
            .virtual_account(&id)
            .cloned()
            .ok_or_else(|| ApiError::internal(format!("virtual account {id} vanished")).into())
    }

    /// `RemoveVirtualAccount`.
    pub(crate) fn remove_virtual_account(&self, id: &AccountId) -> Result<()> {
        let (removed, _) = self
            .inner
            .accounts
            .edit(|doc| doc.remove_virtual_account(id).map_err(edit_error))?;
        if !removed {
            return Err(ApiError::not_found(format!("virtual account {id}")).into());
        }
        self.broadcast(Event::AccountsChanged);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_results() {
        let ok = test_result(Ok(vec!["IMAP4rev1".into()]));
        assert!(ok.ok());
        let failed = test_result(Err(EngineError::permanent(ErrorCode::AuthFailed, "no")));
        assert_eq!(failed.error, Some(ErrorCode::AuthFailed));
    }
}
