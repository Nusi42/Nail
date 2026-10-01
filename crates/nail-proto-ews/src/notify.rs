//! Streaming notifications (push): `Subscribe` with a
//! `StreamingSubscriptionRequest`, `GetStreamingEvents` (one long HTTP
//! answer carrying an envelope per batch of events until the server closes
//! it with `ConnectionStatus` `Closed`), reconnects, and re-subscription
//! when the server forgot the subscription (it expires 30 minutes after
//! the last connection).
//!
//! [`StreamingWatcher::wait`] is what `wait_for_changes` runs: it keeps one
//! subscription per mailbox for the folders asked, reconnects within the
//! caller's time window, and answers the watched folders that changed.
//! When a subscription had to be renewed, events may have been lost, so
//! every watched folder is reported as changed.

use std::collections::HashMap;
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use tokio::time::Instant;

use crate::client::{EwsClient, Operation};
use crate::error::EwsError;
use crate::soap::{self, Answer};
use crate::types::FolderRef;
use crate::xml::{Element, el};

/// The event types Nail subscribes to.
pub const EVENT_TYPES: [&str; 6] = [
    "CopiedEvent",
    "CreatedEvent",
    "DeletedEvent",
    "ModifiedEvent",
    "MovedEvent",
    "NewMailEvent",
];

/// The longest `ConnectionTimeout` EWS accepts (minutes).
pub const MAX_CONNECTION_MINUTES: u32 = 30;

/// Response codes meaning the subscription is gone.
const SUBSCRIPTION_GONE: [&str; 5] = [
    "ErrorSubscriptionNotFound",
    "ErrorInvalidSubscription",
    "ErrorExpiredSubscription",
    "ErrorSubscriptionUnsubscribed",
    "ErrorInvalidSubscriptionRequest",
];

/// One notification event.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NotificationEvent {
    /// `CreatedEvent`, `ModifiedEvent`, … (`StatusEvent` = heartbeat).
    pub kind: String,
    /// The item, for item events.
    pub item_id: Option<String>,
    /// The folder, for folder events.
    pub folder_id: Option<String>,
    /// The folder the item / folder is in.
    pub parent_folder_id: Option<String>,
    /// The folder it was in (moves, copies).
    pub old_parent_folder_id: Option<String>,
}

impl NotificationEvent {
    /// The folders whose contents this event changed.
    pub fn folders(&self) -> impl Iterator<Item = &str> {
        let own = self
            .folder_id
            .as_deref()
            .filter(|_| self.kind == "ModifiedEvent" && self.item_id.is_none());
        [
            self.parent_folder_id.as_deref(),
            self.old_parent_folder_id.as_deref(),
            own,
        ]
        .into_iter()
        .flatten()
    }
}

/// One envelope of a `GetStreamingEvents` answer.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StreamBatch {
    /// Events per subscription id.
    pub events: Vec<(String, NotificationEvent)>,
    /// The server closed the connection (`ConnectionStatus` `Closed`).
    pub closed: bool,
    /// Subscriptions the server reports as broken.
    pub broken_subscriptions: Vec<String>,
}

/// Reads one `GetStreamingEventsResponse`.
pub fn parse_stream_batch(body: &Element) -> Result<StreamBatch, EwsError> {
    let mut out = StreamBatch::default();
    for message in soap::response_messages(body) {
        if message.is_error() {
            if SUBSCRIPTION_GONE.contains(&message.code) {
                out.broken_subscriptions.extend(
                    message
                        .element
                        .child("ErrorSubscriptionIds")
                        .map(|l| {
                            l.children
                                .iter()
                                .map(|s| s.text.trim().to_owned())
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default(),
                );
                if out.broken_subscriptions.is_empty() {
                    out.broken_subscriptions.push(String::new());
                }
                continue;
            }
            return Err(message.error());
        }
        if let Some(notifications) = message.element.child("Notifications") {
            for notification in notifications.children_named("Notification") {
                let subscription = notification
                    .child_text("SubscriptionId")
                    .unwrap_or_default()
                    .to_owned();
                for event in &notification.children {
                    if !event.name.ends_with("Event") {
                        continue;
                    }
                    let id = |name: &str| {
                        event
                            .child(name)
                            .and_then(|e| e.attr("Id"))
                            .map(str::to_owned)
                    };
                    out.events.push((
                        subscription.clone(),
                        NotificationEvent {
                            kind: event.name.clone(),
                            item_id: id("ItemId"),
                            folder_id: id("FolderId"),
                            parent_folder_id: id("ParentFolderId"),
                            old_parent_folder_id: id("OldParentFolderId"),
                        },
                    ));
                }
            }
        }
        if message.element.child_text("ConnectionStatus") == Some("Closed") {
            out.closed = true;
        }
    }
    Ok(out)
}

impl EwsClient {
    /// `Subscribe` for streaming notifications on `folders`; answers the
    /// subscription id.
    pub async fn subscribe_streaming(
        &self,
        mailbox: Option<&str>,
        folders: &[FolderRef<'_>],
    ) -> Result<String, EwsError> {
        let ids: String = folders.iter().map(FolderRef::xml).collect();
        let types: String = EVENT_TYPES.iter().map(|t| el("t:EventType", t)).collect();
        let body = format!(
            "<m:Subscribe><m:StreamingSubscriptionRequest><t:FolderIds>{ids}</t:FolderIds><t:EventTypes>{types}</t:EventTypes></m:StreamingSubscriptionRequest></m:Subscribe>"
        );
        let response = self
            .call(&Operation::write("Subscribe", body).anchored(mailbox))
            .await?;
        let message = soap::single_message(&response.body)?;
        message
            .child_value("SubscriptionId")
            .ok_or_else(|| EwsError::Protocol("Subscribe without a subscription id".into()))
    }

    /// `Unsubscribe` (an unknown subscription is not an error).
    pub async fn unsubscribe(&self, mailbox: Option<&str>, id: &str) -> Result<(), EwsError> {
        let body = format!(
            "<m:Unsubscribe>{}</m:Unsubscribe>",
            el("m:SubscriptionId", id)
        );
        let response = self
            .call(&Operation::read("Unsubscribe", body).anchored(mailbox))
            .await?;
        match soap::single_message(&response.body) {
            Ok(_) | Err(EwsError::NotFound(_)) => Ok(()),
            Err(EwsError::Rejected { code, .. }) if SUBSCRIPTION_GONE.contains(&code.as_str()) => {
                Ok(())
            }
            Err(e) => Err(e),
        }
    }

    /// Opens `GetStreamingEvents` for subscriptions, kept open by the
    /// server for `minutes` (1–30).
    pub async fn get_streaming_events(
        &self,
        mailbox: Option<&str>,
        subscriptions: &[String],
        minutes: u32,
        timeout: Duration,
    ) -> Result<crate::client::EnvelopeStream, EwsError> {
        let ids: String = subscriptions
            .iter()
            .map(|s| el("t:SubscriptionId", s))
            .collect();
        let body = format!(
            "<m:GetStreamingEvents><m:SubscriptionIds>{ids}</m:SubscriptionIds><m:ConnectionTimeout>{}</m:ConnectionTimeout></m:GetStreamingEvents>",
            minutes.clamp(1, MAX_CONNECTION_MINUTES)
        );
        self.open_stream(
            &Operation::read("GetStreamingEvents", body).anchored(mailbox),
            timeout,
        )
        .await
    }
}

#[derive(Debug, Clone)]
struct Active {
    id: String,
    /// The folders asked, sorted (the caller's names).
    asked: Vec<String>,
    /// Server folder id → the caller's name.
    names: HashMap<String, String>,
}

/// Keeps streaming subscriptions per mailbox and waits for changes.
#[derive(Debug, Default)]
pub struct StreamingWatcher {
    active: Mutex<HashMap<Option<String>, Active>>,
}

/// How often a broken stream is reopened within one wait before the error
/// is passed on.
pub const MAX_RECONNECTS: u32 = 3;

impl StreamingWatcher {
    /// A watcher without subscriptions.
    pub fn new() -> Self {
        Self::default()
    }

    /// The subscription id of a mailbox (`None` = own), if any.
    pub fn subscription_id(&self, mailbox: Option<&str>) -> Option<String> {
        self.lock()
            .get(&mailbox.map(str::to_ascii_lowercase))
            .map(|a| a.id.clone())
    }

    /// Forgets the subscription of a mailbox (the next wait subscribes
    /// again).
    pub fn forget(&self, mailbox: Option<&str>) {
        self.lock().remove(&mailbox.map(str::to_ascii_lowercase));
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<Option<String>, Active>> {
        self.active.lock().unwrap_or_else(PoisonError::into_inner)
    }

    async fn ensure(
        &self,
        client: &EwsClient,
        mailbox: Option<&str>,
        folders: &[String],
    ) -> Result<(Active, bool), EwsError> {
        let key = mailbox.map(str::to_ascii_lowercase);
        let mut asked: Vec<String> = folders.to_vec();
        asked.sort();
        asked.dedup();
        let previous = self.lock().get(&key).cloned();
        if let Some(active) = &previous
            && active.asked == asked
        {
            return Ok((active.clone(), false));
        }
        if let Some(old) = previous {
            let _ = client.unsubscribe(mailbox, &old.id).await;
        }
        // Distinguished names (`inbox`) are resolved to ids: events name
        // folders by id.
        let mut names = HashMap::new();
        let refs: Vec<FolderRef<'_>> = asked.iter().map(|f| FolderRef::named(mailbox, f)).collect();
        let distinguished: Vec<(usize, FolderRef<'_>)> = refs
            .iter()
            .enumerate()
            .filter(|(_, r)| matches!(r, FolderRef::Distinguished { .. }))
            .map(|(i, r)| (i, *r))
            .collect();
        if !distinguished.is_empty() {
            let list: Vec<FolderRef<'_>> = distinguished.iter().map(|(_, r)| *r).collect();
            let found = client.get_folders(mailbox, &list).await?;
            for ((i, _), result) in distinguished.iter().zip(found) {
                if let Some(id) = result?.child("FolderId").and_then(|f| f.attr("Id")) {
                    names.insert(id.to_owned(), asked[*i].clone());
                }
            }
        }
        for (i, r) in refs.iter().enumerate() {
            if let FolderRef::Id(id) = r {
                names.insert((*id).to_owned(), asked[i].clone());
            }
        }
        let id = client.subscribe_streaming(mailbox, &refs).await?;
        let active = Active { id, asked, names };
        self.lock().insert(key, active.clone());
        Ok((active, true))
    }

    /// Waits up to `timeout` for changes in `folders` of a mailbox and
    /// answers the folders (as named by the caller) that changed; empty =
    /// none before the timeout.
    pub async fn wait(
        &self,
        client: &EwsClient,
        mailbox: Option<&str>,
        folders: &[String],
        timeout: Duration,
    ) -> Result<Vec<String>, EwsError> {
        if folders.is_empty() {
            tokio::time::sleep(timeout).await;
            return Ok(Vec::new());
        }
        let deadline = Instant::now() + timeout;
        let retry = client.retry_policy();
        let mut failures = 0u32;
        let mut renewed = false;
        loop {
            let (active, fresh) = self.ensure(client, mailbox, folders).await?;
            if fresh && renewed {
                // A lost subscription was replaced: events in between are
                // gone, so everything may have changed.
                return Ok(active.asked.clone());
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Ok(Vec::new());
            }
            let minutes = u32::try_from(remaining.as_secs().div_ceil(60))
                .unwrap_or(MAX_CONNECTION_MINUTES)
                .clamp(1, MAX_CONNECTION_MINUTES);
            let opened = client
                .get_streaming_events(
                    mailbox,
                    std::slice::from_ref(&active.id),
                    minutes,
                    remaining + Duration::from_secs(60),
                )
                .await;
            let mut stream = match opened {
                Ok(stream) => stream,
                Err(e) if is_gone(&e) => {
                    self.forget(mailbox);
                    if renewed {
                        return Err(e);
                    }
                    renewed = true;
                    continue;
                }
                Err(e) if is_transient(&e) && failures < MAX_RECONNECTS => {
                    failures += 1;
                    tokio::time::sleep(retry.backoff(failures).min(remaining)).await;
                    continue;
                }
                Err(e) => return Err(e),
            };
            loop {
                let next = match tokio::time::timeout_at(deadline, stream.next()).await {
                    Err(_) => return Ok(Vec::new()),
                    Ok(next) => next,
                };
                let answer = match next {
                    Ok(Some(answer)) => answer,
                    Ok(None) => break,
                    Err(e) if is_transient(&e) && failures < MAX_RECONNECTS => {
                        failures += 1;
                        tracing::debug!(error = %e, "EWS stream broke, reconnecting");
                        tokio::time::sleep(retry.backoff(failures)).await;
                        break;
                    }
                    Err(e) => return Err(e),
                };
                let response = match answer {
                    Answer::Fault(fault, _) => {
                        let error = fault.error();
                        if is_gone(&error) {
                            self.forget(mailbox);
                            renewed = true;
                            break;
                        }
                        return Err(error);
                    }
                    Answer::Response(response) => response,
                };
                let batch = parse_stream_batch(&response.body)?;
                if !batch.broken_subscriptions.is_empty() {
                    self.forget(mailbox);
                    renewed = true;
                    break;
                }
                let mut changed: Vec<String> = Vec::new();
                for (_, event) in &batch.events {
                    for folder in event.folders() {
                        if let Some(name) = active.names.get(folder)
                            && !changed.contains(name)
                        {
                            changed.push(name.clone());
                        }
                    }
                }
                if !changed.is_empty() {
                    changed.sort();
                    return Ok(changed);
                }
                if batch.closed {
                    break;
                }
            }
        }
    }
}

fn is_gone(error: &EwsError) -> bool {
    match error {
        EwsError::Rejected { code, .. } => SUBSCRIPTION_GONE.contains(&code.as_str()),
        EwsError::NotFound(message) => message.starts_with("ErrorSubscriptionNotFound"),
        _ => false,
    }
}

fn is_transient(error: &EwsError) -> bool {
    matches!(
        error,
        EwsError::Network(_) | EwsError::Timeout | EwsError::Unavailable(..)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xml::parse;
    use pretty_assertions::assert_eq;

    #[test]
    fn reads_notifications() {
        let body = parse(concat!(
            r#"<m:GetStreamingEventsResponse xmlns:m="m" xmlns:t="t"><m:ResponseMessages>"#,
            r#"<m:GetStreamingEventsResponseMessage ResponseClass="Success"><m:ResponseCode>NoError</m:ResponseCode>"#,
            r#"<m:Notifications><m:Notification><t:SubscriptionId>S1</t:SubscriptionId>"#,
            r#"<t:StatusEvent><t:Watermark>W0</t:Watermark></t:StatusEvent>"#,
            r#"<t:CreatedEvent><t:TimeStamp>2026-10-01T10:00:00Z</t:TimeStamp><t:ItemId Id="M1" ChangeKey="k"/>"#,
            r#"<t:ParentFolderId Id="F-inbox"/></t:CreatedEvent>"#,
            r#"<t:MovedEvent><t:ItemId Id="M2"/><t:ParentFolderId Id="F-archive"/><t:OldItemId Id="M0"/>"#,
            r#"<t:OldParentFolderId Id="F-inbox"/></t:MovedEvent>"#,
            r#"<t:ModifiedEvent><t:FolderId Id="F-inbox"/><t:ParentFolderId Id="F-root"/><t:UnreadCount>2</t:UnreadCount></t:ModifiedEvent>"#,
            r#"</m:Notification></m:Notifications><m:ConnectionStatus>OK</m:ConnectionStatus>"#,
            r#"</m:GetStreamingEventsResponseMessage></m:ResponseMessages></m:GetStreamingEventsResponse>"#
        ))
        .unwrap();
        let batch = parse_stream_batch(&body).unwrap();
        assert!(!batch.closed);
        assert_eq!(batch.events.len(), 4);
        assert_eq!(batch.events[0].1.kind, "StatusEvent");
        assert_eq!(batch.events[0].1.folders().count(), 0);
        let created = &batch.events[1].1;
        assert_eq!(created.item_id.as_deref(), Some("M1"));
        assert_eq!(created.folders().collect::<Vec<_>>(), ["F-inbox"]);
        let moved = &batch.events[2].1;
        assert_eq!(
            moved.folders().collect::<Vec<_>>(),
            ["F-archive", "F-inbox"]
        );
        let folder = &batch.events[3].1;
        assert_eq!(folder.folders().collect::<Vec<_>>(), ["F-root", "F-inbox"]);
    }

    #[test]
    fn reads_closes_and_lost_subscriptions() {
        let closed = parse(concat!(
            r#"<m:GetStreamingEventsResponse xmlns:m="m"><m:ResponseMessages>"#,
            r#"<m:GetStreamingEventsResponseMessage ResponseClass="Success"><m:ResponseCode>NoError</m:ResponseCode>"#,
            r#"<m:ConnectionStatus>Closed</m:ConnectionStatus></m:GetStreamingEventsResponseMessage>"#,
            r#"</m:ResponseMessages></m:GetStreamingEventsResponse>"#
        ))
        .unwrap();
        assert!(parse_stream_batch(&closed).unwrap().closed);
        let lost = parse(concat!(
            r#"<m:GetStreamingEventsResponse xmlns:m="m"><m:ResponseMessages>"#,
            r#"<m:GetStreamingEventsResponseMessage ResponseClass="Error"><m:MessageText>gone</m:MessageText>"#,
            r#"<m:ResponseCode>ErrorSubscriptionNotFound</m:ResponseCode><m:ErrorSubscriptionIds>"#,
            r#"<m:SubscriptionId>S1</m:SubscriptionId></m:ErrorSubscriptionIds></m:GetStreamingEventsResponseMessage>"#,
            r#"</m:ResponseMessages></m:GetStreamingEventsResponse>"#
        ))
        .unwrap();
        assert_eq!(
            parse_stream_batch(&lost).unwrap().broken_subscriptions,
            ["S1"]
        );
        let busy = parse(concat!(
            r#"<m:GetStreamingEventsResponse xmlns:m="m"><m:ResponseMessages>"#,
            r#"<m:GetStreamingEventsResponseMessage ResponseClass="Error"><m:ResponseCode>ErrorServerBusy</m:ResponseCode>"#,
            r#"</m:GetStreamingEventsResponseMessage></m:ResponseMessages></m:GetStreamingEventsResponse>"#
        ))
        .unwrap();
        assert!(matches!(
            parse_stream_batch(&busy),
            Err(EwsError::RateLimited(_))
        ));
        assert!(is_gone(&EwsError::Rejected {
            code: "ErrorInvalidSubscription".into(),
            message: String::new()
        }));
        assert!(!is_gone(&EwsError::Timeout));
    }
}
