//! Graph JSON resources as Nail reads them (`v1.0`): users, mail folders,
//! messages, attachments, master categories, mailbox settings, message
//! rules, places, calendars, events, contact folders, contacts, To Do lists
//! and tasks, schedules, collections with paging / delta links, and errors.
//!
//! Every field is optional or defaulted: Graph leaves out what a `$select`
//! did not ask for, and delta pages carry only ids for removed items.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Graph's error answer (`{"error": {"code", "message"}}`).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ErrorBody {
    /// The error.
    #[serde(default)]
    pub error: ErrorDetail,
}

/// The inner error object.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ErrorDetail {
    /// Graph's code (`ErrorItemNotFound`, `syncStateNotFound`, …).
    #[serde(default)]
    pub code: String,
    /// Text.
    #[serde(default)]
    pub message: String,
}

/// A page of a collection (`value` plus paging / delta links).
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Page<T> {
    /// The items.
    #[serde(default = "Vec::new")]
    pub value: Vec<T>,
    /// The next page.
    #[serde(rename = "@odata.nextLink", default)]
    pub next_link: Option<String>,
    /// The delta link of the last page (the cursor).
    #[serde(rename = "@odata.deltaLink", default)]
    pub delta_link: Option<String>,
}

/// The `@removed` marker of delta pages.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Removed {
    /// `changed` (out of scope, e.g. moved away) or `deleted`.
    #[serde(default)]
    pub reason: String,
}

/// A user (`/me`, `/users/{upn}`).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct User {
    /// The directory id.
    #[serde(default)]
    pub id: String,
    /// Display name.
    #[serde(default)]
    pub display_name: Option<String>,
    /// Primary SMTP address.
    #[serde(default)]
    pub mail: Option<String>,
    /// Sign-in name.
    #[serde(default)]
    pub user_principal_name: Option<String>,
}

/// An address with a name (`emailAddress`).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct EmailAddress {
    /// Display name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Address.
    #[serde(default)]
    pub address: String,
}

/// A recipient (`{"emailAddress": …}`).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Recipient {
    /// The address.
    #[serde(default)]
    pub email_address: EmailAddress,
}

impl Recipient {
    /// A recipient of `address`.
    pub fn of(address: &str) -> Self {
        Self {
            email_address: EmailAddress {
                name: None,
                address: address.to_owned(),
            },
        }
    }
}

/// A mail folder.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MailFolder {
    /// Id.
    #[serde(default)]
    pub id: String,
    /// Display name.
    #[serde(default)]
    pub display_name: Option<String>,
    /// Parent folder id.
    #[serde(default)]
    pub parent_folder_id: Option<String>,
    /// Child folders.
    #[serde(default)]
    pub child_folder_count: Option<u32>,
    /// Items.
    #[serde(default)]
    pub total_item_count: Option<u32>,
    /// Unread items.
    #[serde(default)]
    pub unread_item_count: Option<u32>,
    /// Hidden folder.
    #[serde(default)]
    pub is_hidden: Option<bool>,
    /// Gone (delta pages).
    #[serde(rename = "@removed", default)]
    pub removed: Option<Removed>,
}

/// A follow-up flag.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FollowupFlag {
    /// `notFlagged`, `flagged` or `complete`.
    #[serde(default)]
    pub flag_status: String,
}

/// A message (delta / list entries; the content comes as MIME).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    /// Id.
    #[serde(default)]
    pub id: String,
    /// Version.
    #[serde(default)]
    pub change_key: Option<String>,
    /// Conversation.
    #[serde(default)]
    pub conversation_id: Option<String>,
    /// Folder.
    #[serde(default)]
    pub parent_folder_id: Option<String>,
    /// Read.
    #[serde(default)]
    pub is_read: Option<bool>,
    /// Draft.
    #[serde(default)]
    pub is_draft: Option<bool>,
    /// Flag.
    #[serde(default)]
    pub flag: Option<FollowupFlag>,
    /// Categories.
    #[serde(default)]
    pub categories: Option<Vec<String>>,
    /// Focused Inbox (`focused` / `other`).
    #[serde(default)]
    pub inference_classification: Option<String>,
    /// Received (RFC 3339 UTC).
    #[serde(default)]
    pub received_date_time: Option<String>,
    /// `Message-ID`.
    #[serde(default)]
    pub internet_message_id: Option<String>,
    /// Subject.
    #[serde(default)]
    pub subject: Option<String>,
    /// Sender.
    #[serde(default)]
    pub from: Option<Recipient>,
    /// Has attachments.
    #[serde(default)]
    pub has_attachments: Option<bool>,
    /// Gone (delta pages).
    #[serde(rename = "@removed", default)]
    pub removed: Option<Removed>,
}

/// An attachment of a message (metadata; content through `$value`).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Attachment {
    /// Id.
    #[serde(default)]
    pub id: String,
    /// `#microsoft.graph.fileAttachment`, `itemAttachment`,
    /// `referenceAttachment`.
    #[serde(rename = "@odata.type", default)]
    pub odata_type: Option<String>,
    /// File name.
    #[serde(default)]
    pub name: Option<String>,
    /// MIME type.
    #[serde(default)]
    pub content_type: Option<String>,
    /// Size in bytes.
    #[serde(default)]
    pub size: Option<u64>,
    /// Inline (`Content-ID` referenced).
    #[serde(default)]
    pub is_inline: Option<bool>,
    /// Content id.
    #[serde(default)]
    pub content_id: Option<String>,
}

/// A master category.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutlookCategory {
    /// Id.
    #[serde(default)]
    pub id: String,
    /// Name (cannot change after creation).
    #[serde(default)]
    pub display_name: String,
    /// `none`, `preset0` … `preset24`.
    #[serde(default)]
    pub color: Option<String>,
}

/// `dateTimeTimeZone`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DateTimeTimeZone {
    /// Wall time (`2026-10-05T09:00:00.0000000`).
    #[serde(default)]
    pub date_time: String,
    /// Zone (Windows, IANA or `UTC`).
    #[serde(default)]
    pub time_zone: Option<String>,
}

/// `automaticRepliesSetting`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutomaticRepliesSetting {
    /// `disabled`, `alwaysEnabled`, `scheduled`.
    #[serde(default)]
    pub status: Option<String>,
    /// `none`, `contactsOnly`, `all`.
    #[serde(default)]
    pub external_audience: Option<String>,
    /// Start of a schedule.
    #[serde(default)]
    pub scheduled_start_date_time: Option<DateTimeTimeZone>,
    /// End of a schedule.
    #[serde(default)]
    pub scheduled_end_date_time: Option<DateTimeTimeZone>,
    /// Reply inside the organization (HTML).
    #[serde(default)]
    pub internal_reply_message: Option<String>,
    /// Reply to outside senders (HTML).
    #[serde(default)]
    pub external_reply_message: Option<String>,
}

/// `mailboxSettings` (the parts Nail reads).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MailboxSettings {
    /// Automatic replies.
    #[serde(default)]
    pub automatic_replies_setting: Option<AutomaticRepliesSetting>,
    /// The mailbox's zone.
    #[serde(default)]
    pub time_zone: Option<String>,
}

/// A size range of a rule (KB).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SizeRange {
    /// Minimum.
    #[serde(default)]
    pub minimum_size: Option<u32>,
    /// Maximum.
    #[serde(default)]
    pub maximum_size: Option<u32>,
}

/// A place (room or room list).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Place {
    /// Id.
    #[serde(default)]
    pub id: String,
    /// Display name.
    #[serde(default)]
    pub display_name: Option<String>,
    /// Address.
    #[serde(default)]
    pub email_address: Option<String>,
    /// Seats.
    #[serde(default)]
    pub capacity: Option<u32>,
    /// Building.
    #[serde(default)]
    pub building: Option<String>,
    /// Floor label.
    #[serde(default)]
    pub floor_label: Option<String>,
    /// Floor number.
    #[serde(default)]
    pub floor_number: Option<i32>,
    /// Accessible.
    #[serde(default)]
    pub is_wheel_chair_accessible: Option<bool>,
    /// Audio device.
    #[serde(default)]
    pub audio_device_name: Option<String>,
    /// Video device.
    #[serde(default)]
    pub video_device_name: Option<String>,
    /// Display device.
    #[serde(default)]
    pub display_device_name: Option<String>,
    /// Tags.
    #[serde(default)]
    pub tags: Vec<String>,
}

/// A calendar.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Calendar {
    /// Id.
    #[serde(default)]
    pub id: String,
    /// Version.
    #[serde(default)]
    pub change_key: Option<String>,
    /// Name.
    #[serde(default)]
    pub name: String,
    /// `auto`, `lightBlue`, ….
    #[serde(default)]
    pub color: Option<String>,
    /// The user may write.
    #[serde(default)]
    pub can_edit: Option<bool>,
    /// The default calendar.
    #[serde(default)]
    pub is_default_calendar: Option<bool>,
}

/// A contact folder.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContactFolder {
    /// Id.
    #[serde(default)]
    pub id: String,
    /// Name.
    #[serde(default)]
    pub display_name: Option<String>,
    /// Parent (the default contacts folder for top-level folders).
    #[serde(default)]
    pub parent_folder_id: Option<String>,
}

/// A To Do list.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TodoTaskList {
    /// Id.
    #[serde(default)]
    pub id: String,
    /// Name.
    #[serde(default)]
    pub display_name: String,
    /// `none`, `defaultList`, `flaggedEmails`, `unknownFutureValue`.
    #[serde(default)]
    pub wellknown_list_name: Option<String>,
    /// The user owns it.
    #[serde(default)]
    pub is_owner: Option<bool>,
    /// Version.
    #[serde(rename = "@odata.etag", default)]
    pub etag: Option<String>,
}

/// A free/busy answer of `getSchedule`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleInformation {
    /// The address asked.
    #[serde(default)]
    pub schedule_id: String,
    /// Busy items.
    #[serde(default)]
    pub schedule_items: Vec<ScheduleItem>,
    /// Asking failed for this address.
    #[serde(default)]
    pub error: Option<FreeBusyError>,
}

/// One busy item of a schedule.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleItem {
    /// `free`, `tentative`, `busy`, `oof`, `workingElsewhere`, `unknown`.
    #[serde(default)]
    pub status: String,
    /// Start.
    #[serde(default)]
    pub start: DateTimeTimeZone,
    /// End.
    #[serde(default)]
    pub end: DateTimeTimeZone,
}

/// The error of one schedule.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FreeBusyError {
    /// Text.
    #[serde(default)]
    pub message: Option<String>,
    /// Code (`ErrorMailRecipientNotFound`, …).
    #[serde(default)]
    pub response_code: Option<String>,
}

/// The id of an object in a raw JSON value.
pub(crate) fn id_of(value: &Value) -> &str {
    value.get("id").and_then(Value::as_str).unwrap_or_default()
}

/// Whether a raw delta entry is a removal.
pub(crate) fn is_removed(value: &Value) -> bool {
    value.get("@removed").is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn pages_and_removed_entries() {
        let page: Page<Message> = serde_json::from_value(json!({
            "@odata.context": "x",
            "@odata.deltaLink": "https://graph.microsoft.com/v1.0/me/mailFolders/i/messages/delta?$deltatoken=a",
            "value": [
                {"id": "m1", "isRead": true, "flag": {"flagStatus": "flagged"}},
                {"id": "m2", "@removed": {"reason": "deleted"}}
            ]
        }))
        .unwrap();
        assert_eq!(page.value.len(), 2);
        assert!(page.next_link.is_none() && page.delta_link.is_some());
        assert_eq!(page.value[0].flag.as_ref().unwrap().flag_status, "flagged");
        assert_eq!(page.value[1].removed.as_ref().unwrap().reason, "deleted");
        let empty: Page<Message> = serde_json::from_value(json!({})).unwrap();
        assert!(empty.value.is_empty());
        assert!(is_removed(&json!({"id": "x", "@removed": {}})));
        assert_eq!(id_of(&json!({"id": "x"})), "x");
        assert_eq!(id_of(&json!({})), "");
    }

    #[test]
    fn errors_and_recipients() {
        let body: ErrorBody = serde_json::from_value(json!({
            "error": {"code": "ErrorItemNotFound", "message": "The specified object was not found in the store.",
                      "innerError": {"date": "2026-10-01T10:00:00", "request-id": "r"}}
        }))
        .unwrap();
        assert_eq!(body.error.code, "ErrorItemNotFound");
        assert_eq!(
            serde_json::to_value(Recipient::of("a@b.example")).unwrap(),
            json!({"emailAddress": {"address": "a@b.example"}})
        );
    }
}
