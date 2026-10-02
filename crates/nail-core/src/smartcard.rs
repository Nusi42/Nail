//! M4c smartcards and the vault recovery key (ADR 0094).
//!
//! Contract stage (WP120): the smartcard settings are stored
//! ([`SMARTCARD_SETTINGS_KEY`]); listing tokens, using their keys, PIN
//! prompts and the recovery key answer `unsupported` (`m4c-not-ready`)
//! until WP125 wires `nail-pkcs11` / `nail-vault` (WP122) in.

use nail_api::smartcard::SmartcardSettings;
use nail_api::{ApiError, LocalizedMessage, Request, Response};

use crate::error::Result;
use crate::service::Service;

/// `app_settings` key of [`SmartcardSettings`].
pub const SMARTCARD_SETTINGS_KEY: &str = "security.smartcards";

pub(crate) async fn handle(service: &Service, request: Request) -> Result<Response> {
    match request {
        Request::GetSmartcardSettings => {
            let settings: SmartcardSettings = service
                .store()
                .call(|s| s.setting(SMARTCARD_SETTINGS_KEY))
                .await?
                .unwrap_or_default();
            Ok(Response::SmartcardSettings { settings })
        }
        Request::SetSmartcardSettings { settings } => {
            if let Some(issue) = settings.issues().first() {
                return Err(ApiError::invalid_request(format!(
                    "smartcard settings: {}",
                    issue.as_str()
                ))
                .with_localized(LocalizedMessage::new(issue.message_id()))
                .into());
            }
            let saved = settings.clone();
            service
                .store()
                .call(move |s| s.set_setting(SMARTCARD_SETTINGS_KEY, &saved))
                .await?;
            Ok(Response::SmartcardSettings { settings })
        }
        other => Err(crate::portability::not_ready(&other)),
    }
}
