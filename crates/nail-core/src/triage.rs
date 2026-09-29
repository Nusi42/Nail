//! Requests of wave M2c for junk mail, snooze and notifications
//! (ADR 0043): `MarkJunk`, `GetJunkStatus`, `UpdateJunkSettings`,
//! `ResetJunkClassifier`, `SnoozeMessages`, `UnsnoozeMessages`,
//! `ListSnoozed`, `GetNotificationSettings`, `UpdateNotificationSettings`,
//! `TestNotification`.
//!
//! [`Service::dispatch`](crate::service::Service) routes exactly these
//! requests here (one match arm). Settings, status and lists are served
//! from the store now. Training and classifying (`nail-triage`, WP52),
//! snoozing (hidden folder, keyword, wake timer) and the notification
//! policy on arrival are wired by WP55: until then `MarkJunk`,
//! `SnoozeMessages` and `UnsnoozeMessages` answer `unsupported`.

use nail_api::triage::{JunkStatus, MailNotification, NotificationKind, SnoozedMessage};
use nail_api::{AccountId, ApiError, Event, LocalizedMessage, Request, Response};

use crate::engine::unix_now;
use crate::error::Result;
use crate::rules::rfc3339;
use crate::service::Service;

/// Handles one of this module's requests.
pub(crate) async fn handle(service: &Service, request: Request) -> Result<Response> {
    Ok(match request {
        Request::MarkJunk { .. } => {
            return Err(ApiError::unsupported(
                "junk training is not wired yet (nail-triage, WP55)",
            )
            .into());
        }
        Request::SnoozeMessages { .. } | Request::UnsnoozeMessages { .. } => {
            return Err(
                ApiError::unsupported("snoozing is not wired yet (nail-triage, WP55)").into(),
            );
        }
        Request::GetJunkStatus { account_id } => junk_status(service, account_id).await?,
        Request::UpdateJunkSettings {
            account_id,
            settings,
        } => {
            check_account(service, &account_id)?;
            if let Some(issue) = settings.issues().first() {
                return Err(ApiError::invalid_request(format!(
                    "invalid junk settings ({})",
                    issue.message_id()
                ))
                .with_localized(LocalizedMessage::new(issue.message_id()))
                .into());
            }
            let id = account_id.clone();
            service
                .store()
                .call(move |s| s.set_junk_settings(&id, &settings))
                .await?;
            service.broadcast(Event::JunkStatusChanged {
                account_id: account_id.clone(),
            });
            junk_status(service, account_id).await?
        }
        Request::ResetJunkClassifier { account_id } => {
            check_account(service, &account_id)?;
            let id = account_id.clone();
            service.store().call(move |s| s.reset_junk(&id)).await?;
            service.broadcast(Event::JunkStatusChanged {
                account_id: account_id.clone(),
            });
            junk_status(service, account_id).await?
        }
        Request::ListSnoozed { account_id } => {
            let members = match &account_id {
                None => None,
                Some(id) => Some(member_accounts(service, id).await?),
            };
            let records = service
                .store()
                .call(move |s| {
                    let mut out = Vec::new();
                    for record in s.list_snoozed(None)? {
                        if members
                            .as_ref()
                            .is_some_and(|m| !m.contains(&record.account_id))
                        {
                            continue;
                        }
                        // Entries whose message vanished are skipped (the
                        // wake timer cleans them up).
                        if let Ok(message) = s.get_message(&record.message_id) {
                            out.push(SnoozedMessage {
                                message: message.summary,
                                until: rfc3339(record.until),
                                wake_folder_id: record.wake_folder_id,
                                snoozed_at: rfc3339(record.snoozed_at),
                            });
                        }
                    }
                    Ok(out)
                })
                .await?;
            Response::Snoozed { messages: records }
        }
        Request::GetNotificationSettings => Response::NotificationSettings {
            settings: service.store().call(|s| s.notification_settings()).await?,
        },
        Request::UpdateNotificationSettings { settings } => {
            if let Some(issue) = settings.issues().first() {
                return Err(ApiError::invalid_request(format!(
                    "invalid notification settings ({})",
                    issue.message_id()
                ))
                .with_localized(LocalizedMessage::new(issue.message_id()))
                .into());
            }
            let saved = settings.clone();
            service
                .store()
                .call(move |s| s.set_notification_settings(&saved))
                .await?;
            service.broadcast(Event::NotificationSettingsChanged);
            Response::NotificationSettings { settings }
        }
        Request::TestNotification => {
            let settings = service.store().call(|s| s.notification_settings()).await?;
            service.broadcast(Event::NotificationRequested {
                notification: test_notification(unix_now(), settings.sound),
            });
            Response::Done
        }
        other => {
            return Err(ApiError::internal(format!(
                "{} is not a triage request",
                crate::service::request_name(&other)
            ))
            .into());
        }
    })
}

/// The notification [`Request::TestNotification`] pushes.
pub(crate) fn test_notification(now: i64, sound: bool) -> MailNotification {
    MailNotification {
        id: format!("test-{now}"),
        tag: "test".to_owned(),
        kind: NotificationKind::Test,
        account_id: None,
        folder_id: None,
        message_ids: Vec::new(),
        title: LocalizedMessage::new("notification-test-title"),
        body: Some(LocalizedMessage::new("notification-test-body")),
        sound,
    }
}

/// The real accounts behind an account id (itself, or a virtual
/// account's members).
async fn member_accounts(service: &Service, id: &AccountId) -> Result<Vec<AccountId>> {
    if id.as_str() == AccountId::ALL {
        return service.all_members().await;
    }
    let config = service.inner.accounts.config();
    Ok(match config.virtual_account(id) {
        Some(va) => va.members.clone(),
        None => vec![id.clone()],
    })
}

fn check_account(service: &Service, id: &AccountId) -> Result<()> {
    if service.inner.accounts.config().account(id).is_none() {
        return Err(ApiError::not_found(format!("account {id}")).into());
    }
    Ok(())
}

async fn junk_status(service: &Service, account_id: AccountId) -> Result<Response> {
    check_account(service, &account_id)?;
    let id = account_id.clone();
    let (settings, stats) = service
        .store()
        .call(move |s| Ok((s.junk_settings(&id)?, s.junk_stats(&id)?)))
        .await?;
    let ready =
        stats.trained_junk >= settings.min_training && stats.trained_ham >= settings.min_training;
    Ok(Response::JunkStatus {
        status: JunkStatus {
            account_id,
            settings,
            trained_junk: stats.trained_junk,
            trained_ham: stats.trained_ham,
            tokens: stats.tokens,
            ready,
            last_trained_at: stats.last_trained_at.map(rfc3339),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_notification_is_localized() {
        let n = test_notification(5, false);
        assert_eq!(n.kind, NotificationKind::Test);
        assert!(n.title.is_valid() && n.body.as_ref().is_some_and(|b| b.is_valid()));
        assert!(!n.sound && n.message_ids.is_empty());
    }
}
