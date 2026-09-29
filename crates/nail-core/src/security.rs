//! Requests of wave M2a (ADR 0029): keys and certificates, recipient key
//! status, Autocrypt setup messages, the security settings and encryption
//! at rest.
//!
//! [`Service::dispatch`](crate::service::Service) routes exactly these
//! requests here (one match arm). The security settings and the vault
//! status are served from the store already; everything else answers
//! `unsupported` until the integration (WP35) wires `nail-pgp`,
//! `nail-smime` and `nail-vault` in. The parallel packages WP31–WP34 do
//! not edit this file.

use nail_api::security::{SecuritySettings, VaultStatus};
use nail_api::{ApiError, Event, Request, Response};
use nail_store::{SECURITY_SETTINGS_KEY, VaultRecord};

use crate::error::Result;
use crate::service::{Service, request_name};

/// Handles one of this module's requests.
pub(crate) async fn handle(service: &Service, request: Request) -> Result<Response> {
    match request {
        Request::GetSecuritySettings => Ok(Response::SecuritySettings {
            settings: security_settings(service).await?,
        }),
        Request::UpdateSecuritySettings { settings } => {
            let settings = update_security_settings(service, settings).await?;
            service.broadcast(Event::SecuritySettingsChanged);
            Ok(Response::SecuritySettings { settings })
        }
        Request::GetVaultStatus => {
            let record = service.store().clone();
            let record = tokio::task::spawn_blocking(move || record.vault_state()).await??;
            Ok(Response::VaultStatus {
                status: vault_status(&record),
            })
        }
        other => Err(ApiError::unsupported(format!(
            "{} is not available yet (M2a)",
            request_name(&other)
        ))
        .into()),
    }
}

/// The stored security settings, or the defaults.
pub(crate) async fn security_settings(service: &Service) -> Result<SecuritySettings> {
    let store = service.store().clone();
    let stored = tokio::task::spawn_blocking(move || {
        store.setting::<SecuritySettings>(SECURITY_SETTINGS_KEY)
    })
    .await??;
    Ok(stored.unwrap_or_default())
}

async fn update_security_settings(
    service: &Service,
    mut settings: SecuritySettings,
) -> Result<SecuritySettings> {
    let config = service.inner.accounts.config();
    for account in &settings.index_decrypted {
        if config.account(account).is_none() {
            return Err(ApiError::invalid_request(format!(
                "index_decrypted names {account}, which is not a real account"
            ))
            .into());
        }
    }
    settings.index_decrypted.sort();
    settings.index_decrypted.dedup();
    let mut seen = Vec::new();
    settings.lookup_sources.retain(|s| {
        let new = !seen.contains(s);
        seen.push(*s);
        new
    });
    let store = service.store().clone();
    let saved = settings.clone();
    tokio::task::spawn_blocking(move || store.set_setting(SECURITY_SETTINGS_KEY, &saved)).await??;
    Ok(settings)
}

/// The wire view of the stored vault state.
pub(crate) fn vault_status(record: &VaultRecord) -> VaultStatus {
    VaultStatus {
        state: record.state,
        phase: record.phase,
        done: record.done,
        total: record.total,
        key_in_keychain: record.key_ref.is_some(),
        error: record.error.as_ref().map(|_| nail_api::ErrorCode::Internal),
        message: record.error.clone(),
    }
}

#[cfg(test)]
mod tests {
    use nail_api::security::{VaultPhase, VaultState};

    use super::*;

    #[test]
    fn vault_status_from_record() {
        assert_eq!(vault_status(&VaultRecord::default()), VaultStatus::plain());
        let status = vault_status(&VaultRecord {
            state: VaultState::Encrypting,
            phase: Some(VaultPhase::Index),
            done: 3,
            total: 9,
            key_ref: Some("keychain:nail/vault".into()),
            error: Some("disk full".into()),
            ..VaultRecord::default()
        });
        assert_eq!(status.state, VaultState::Encrypting);
        assert!(status.key_in_keychain);
        assert_eq!(status.error, Some(nail_api::ErrorCode::Internal));
        assert_eq!(status.message.as_deref(), Some("disk full"));
    }
}
