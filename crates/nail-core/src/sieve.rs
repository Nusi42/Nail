//! Requests of wave M2c for server-side rules and vacation replies
//! (ADR 0043): `GetSieveStatus`, `SyncSieve`, `PreviewSieveScript`,
//! `GetVacation`, `UpdateVacation`.
//!
//! [`Service::dispatch`](crate::service::Service) routes exactly these
//! requests here (one match arm). What needs only `accounts.kdl` and the
//! store is served now: the Sieve state per account (from the `sieve` node
//! and the recorded uploads) and the vacation settings. Compiling rules to
//! Sieve and talking ManageSieve (`nail-sieve`, WP51) are wired by WP55:
//! until then `SyncSieve` and `PreviewSieveScript` answer `unsupported`,
//! rule placements are empty and no script is uploaded.

use nail_api::accounts::{AccountConfig, IncomingServer};
use nail_api::sieve::{SieveAccountStatus, SieveState};
use nail_api::{AccountId, ApiError, Event, Request, Response};
use nail_store::SieveRecord;

use crate::error::Result;
use crate::service::Service;

/// Handles one of this module's requests.
pub(crate) async fn handle(service: &Service, request: Request) -> Result<Response> {
    Ok(match request {
        Request::GetSieveStatus { account_id } => {
            let config = service.inner.accounts.config();
            let accounts: Vec<AccountConfig> = match account_id {
                Some(id) => vec![imap_account(&config, &id)?.clone()],
                None => config
                    .accounts
                    .iter()
                    .filter(|a| a.sieve.is_some())
                    .cloned()
                    .collect(),
            };
            let mut statuses = Vec::with_capacity(accounts.len());
            for account in accounts {
                statuses.push(status(service, &account).await?);
            }
            Response::SieveStatus { accounts: statuses }
        }
        Request::SyncSieve { account_id } | Request::PreviewSieveScript { account_id } => {
            let config = service.inner.accounts.config();
            sieve_account(&config, &account_id)?;
            return Err(ApiError::unsupported(
                "compiling rules to Sieve and ManageSieve uploads are not wired yet (WP55)",
            )
            .into());
        }
        Request::GetVacation { account_id } => {
            let config = service.inner.accounts.config();
            imap_account(&config, &account_id)?;
            let id = account_id.clone();
            let record = service.store().call(move |s| s.sieve_record(&id)).await?;
            Response::Vacation {
                account_id,
                settings: record.vacation,
            }
        }
        Request::UpdateVacation {
            account_id,
            settings,
        } => {
            let config = service.inner.accounts.config();
            sieve_account(&config, &account_id)?;
            if let Some(issue) = settings.issues().first() {
                return Err(ApiError::invalid_request(format!(
                    "invalid vacation settings ({})",
                    issue.message_id()
                ))
                .with_localized(nail_api::LocalizedMessage::new(issue.message_id()))
                .into());
            }
            let (id, saved) = (account_id.clone(), settings.clone());
            service
                .store()
                .call(move |s| s.set_vacation(&id, &saved))
                .await?;
            service.broadcast(Event::SieveStatusChanged {
                account_id: account_id.clone(),
            });
            Response::Vacation {
                account_id,
                settings,
            }
        }
        other => {
            return Err(ApiError::internal(format!(
                "{} is not a Sieve request",
                crate::service::request_name(&other)
            ))
            .into());
        }
    })
}

/// A configured IMAP account (`not_found` / `unsupported` otherwise).
fn imap_account<'a>(
    config: &'a nail_api::AccountsConfig,
    id: &AccountId,
) -> Result<&'a AccountConfig> {
    let account = config
        .account(id)
        .ok_or_else(|| ApiError::not_found(format!("account {id}")))?;
    if !matches!(account.incoming, IncomingServer::Imap { .. }) {
        return Err(ApiError::unsupported(format!(
            "account {id} is not an IMAP account; Sieve needs IMAP"
        ))
        .into());
    }
    Ok(account)
}

/// An IMAP account with a `sieve` node (`unsupported` otherwise).
fn sieve_account<'a>(
    config: &'a nail_api::AccountsConfig,
    id: &AccountId,
) -> Result<&'a AccountConfig> {
    let account = imap_account(config, id)?;
    if account.sieve.is_none() {
        return Err(ApiError::unsupported(format!(
            "account {id} has no ManageSieve settings (`sieve` in accounts.kdl)"
        ))
        .into());
    }
    Ok(account)
}

/// The state of an account's script from its settings and the store.
async fn status(service: &Service, account: &AccountConfig) -> Result<SieveAccountStatus> {
    let id = account.id.clone();
    let record = service.store().call(move |s| s.sieve_record(&id)).await?;
    Ok(account_status(account, record))
}

/// Pure part of [`status`].
pub(crate) fn account_status(account: &AccountConfig, record: SieveRecord) -> SieveAccountStatus {
    let config = account.sieve.clone().unwrap_or_default();
    let enabled = account.sieve.is_some()
        && config.is_enabled()
        && matches!(account.incoming, IncomingServer::Imap { .. });
    let state = if !enabled {
        SieveState::Disabled
    } else if record.last_error.is_some() {
        SieveState::Failed
    } else if record.script_hash.is_some() {
        SieveState::InSync
    } else {
        SieveState::Pending
    };
    let error = record.last_error.clone();
    SieveAccountStatus {
        account_id: account.id.clone(),
        state,
        script_name: config.effective_script_name().to_owned(),
        uploaded_at: record.uploaded_at.map(crate::rules::rfc3339),
        extensions: record.extensions,
        other_scripts: record.other_scripts,
        rules: Vec::new(),
        vacation_active: record.script_hash.is_some() && record.vacation.enabled,
        error: error.as_ref().map(|e| e.code),
        message: error.as_ref().map(|e| e.message.clone()),
        localized: error.and_then(|e| e.localized).map(|l| *l),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nail_api::accounts::{AuthConfig, ImapOptions, ServerEndpoint, SyncPolicy, TlsMode};
    use nail_api::sieve::{ManageSieveConfig, VacationSettings};
    use nail_api::{ErrorCode, LocalizedMessage};

    fn account(sieve: Option<ManageSieveConfig>) -> AccountConfig {
        AccountConfig {
            id: "work".into(),
            name: "Work".into(),
            email: "ada@example.org".into(),
            enabled: true,
            incoming: IncomingServer::Imap {
                server: ServerEndpoint {
                    host: "imap.example.org".into(),
                    port: 993,
                    tls: TlsMode::Implicit,
                    auth: AuthConfig::None,
                },
                options: ImapOptions::default(),
            },
            outgoing: None,
            identities: vec![],
            sync: SyncPolicy::default(),
            save_sent_copy: None,
            authserv_ids: vec![],
            sieve,
        }
    }

    #[test]
    fn states_follow_settings_and_records() {
        let off = account_status(&account(None), SieveRecord::default());
        assert_eq!(off.state, SieveState::Disabled);
        assert_eq!(off.script_name, "nail");

        let on = account(Some(ManageSieveConfig::default()));
        assert_eq!(
            account_status(&on, SieveRecord::default()).state,
            SieveState::Pending
        );
        let uploaded = SieveRecord {
            script_hash: Some("0".repeat(64)),
            uploaded_at: Some(1_790_000_000),
            vacation: VacationSettings {
                enabled: true,
                body: "Away".into(),
                ..VacationSettings::default()
            },
            ..SieveRecord::default()
        };
        let status = account_status(&on, uploaded.clone());
        assert_eq!(status.state, SieveState::InSync);
        assert!(status.vacation_active);
        assert_eq!(status.uploaded_at.as_deref(), Some("2026-09-21T14:13:20Z"));

        let failed = SieveRecord {
            last_error: Some(
                ApiError::new(ErrorCode::Network, "refused")
                    .with_localized(LocalizedMessage::new("sieve-error-connect")),
            ),
            ..uploaded
        };
        let status = account_status(&on, failed);
        assert_eq!(status.state, SieveState::Failed);
        assert_eq!(status.error, Some(ErrorCode::Network));
        assert_eq!(
            status.localized.map(|l| l.id).as_deref(),
            Some("sieve-error-connect")
        );

        let disabled = account(Some(ManageSieveConfig {
            enabled: Some(false),
            script_name: Some("mine".into()),
            ..ManageSieveConfig::default()
        }));
        let status = account_status(&disabled, SieveRecord::default());
        assert_eq!(status.state, SieveState::Disabled);
        assert_eq!(status.script_name, "mine");
    }
}
