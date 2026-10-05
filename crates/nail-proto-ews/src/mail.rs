//! Folders and items: `GetFolder`, `SyncFolderHierarchy`,
//! `SyncFolderItems` (the sync state is the cursor), `GetItem` (properties
//! and `MimeContent`), `CreateItem` (`MimeContent` appends, sending with
//! `SendAndSaveCopy`), `UpdateItem` (read, flag, answered, categories;
//! change keys), `MoveItem` / `CopyItem` (new ids), `DeleteItem`, and
//! `FindItem` by `InternetMessageId` (idempotent appends).

use base64::Engine as _;
use nail_api::accounts::SendingMailbox;
use nail_api::domain::Flags;
use nail_api::engine::{RemoteMessage, RemoteMessageChange};
use nail_api::exchange::engine::{ExchangeFolder, ExchangeFolderClass};
use nail_api::exchange::{ExchangeMessageInfo, ExchangeWellKnownFolder, InferenceClassification};
use nail_api::ops::FlagChange;

use crate::client::{EwsClient, Operation};
use crate::error::EwsError;
use crate::soap::{self, ResponseMessage};
use crate::types::{
    Field, FolderRef, ItemRef, LAST_VERB, MESSAGE_FLAGS, Update, extended_property, extended_value,
    unix,
};
use crate::xml::{Element, el, escape};

/// Most changes one `SyncFolderItems` call returns (the server's limit).
pub const MAX_SYNC_CHANGES: u32 = 512;

/// Most items per `GetItem` call.
pub const GET_ITEM_BATCH: usize = 50;

/// The well-known folders looked up by their distinguished ids (Archive
/// has none in EWS; it is found by name below the root).
pub const DISTINGUISHED: [ExchangeWellKnownFolder; 10] = [
    ExchangeWellKnownFolder::Root,
    ExchangeWellKnownFolder::Inbox,
    ExchangeWellKnownFolder::SentItems,
    ExchangeWellKnownFolder::Drafts,
    ExchangeWellKnownFolder::DeletedItems,
    ExchangeWellKnownFolder::JunkEmail,
    ExchangeWellKnownFolder::Outbox,
    ExchangeWellKnownFolder::Calendar,
    ExchangeWellKnownFolder::Contacts,
    ExchangeWellKnownFolder::Tasks,
];

/// The properties Nail reads of a message.
pub fn message_shape(focused_inbox: bool) -> String {
    let mut fields = String::from(concat!(
        r#"<t:FieldURI FieldURI="item:ParentFolderId"/>"#,
        r#"<t:FieldURI FieldURI="item:ItemClass"/>"#,
        r#"<t:FieldURI FieldURI="item:DateTimeReceived"/>"#,
        r#"<t:FieldURI FieldURI="item:Size"/>"#,
        r#"<t:FieldURI FieldURI="item:Categories"/>"#,
        r#"<t:FieldURI FieldURI="item:IsDraft"/>"#,
        r#"<t:FieldURI FieldURI="item:Flag"/>"#,
        r#"<t:FieldURI FieldURI="item:ConversationId"/>"#,
        r#"<t:FieldURI FieldURI="message:IsRead"/>"#,
    ));
    if focused_inbox {
        fields.push_str(r#"<t:FieldURI FieldURI="item:InferenceClassification"/>"#);
    }
    fields.push_str(&LAST_VERB.xml());
    format!(
        "<m:ItemShape><t:BaseShape>IdOnly</t:BaseShape><t:AdditionalProperties>{fields}</t:AdditionalProperties></m:ItemShape>"
    )
}

/// The folder properties Nail reads.
pub const FOLDER_SHAPE: &str = concat!(
    "<m:FolderShape><t:BaseShape>IdOnly</t:BaseShape><t:AdditionalProperties>",
    r#"<t:FieldURI FieldURI="folder:ParentFolderId"/>"#,
    r#"<t:FieldURI FieldURI="folder:DisplayName"/>"#,
    r#"<t:FieldURI FieldURI="folder:FolderClass"/>"#,
    r#"<t:FieldURI FieldURI="folder:TotalCount"/>"#,
    r#"<t:FieldURI FieldURI="folder:UnreadCount"/>"#,
    r#"<t:FieldURI FieldURI="folder:ChildFolderCount"/>"#,
    "</t:AdditionalProperties></m:FolderShape>"
);

/// Folder changes of a `SyncFolderHierarchy` call.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FolderChanges {
    /// The new sync state.
    pub sync_state: String,
    /// Whether every change is included (`IncludesLastFolderInRange`).
    pub complete: bool,
    /// New or changed folders (`t:Folder`, `t:CalendarFolder`, …).
    pub upserted: Vec<Element>,
    /// Ids of deleted folders.
    pub deleted: Vec<String>,
}

/// Item changes of a `SyncFolderItems` call.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ItemChanges {
    /// The new sync state.
    pub sync_state: String,
    /// Whether every change is included (`IncludesLastItemInRange`).
    pub complete: bool,
    /// New items.
    pub created: Vec<Element>,
    /// Changed items.
    pub updated: Vec<Element>,
    /// Ids of deleted items (also items moved away).
    pub deleted: Vec<String>,
    /// Read-state changes (id, read).
    pub read_changes: Vec<(String, bool)>,
}

/// The answer to a sync call: changes, or the sync state is no longer
/// valid (`ErrorInvalidSyncStateData`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncOutcome<T> {
    /// Changes.
    Changes(T),
    /// Start again without a sync state.
    Expired,
}

fn sync_expired(message: &ResponseMessage<'_>) -> bool {
    matches!(
        message.code,
        "ErrorInvalidSyncStateData" | "ErrorInvalidSyncStateDataVersion" | "ErrorSyncStateExpired"
    )
}

/// The id of a `t:Folder`'s / item's id element.
fn element_id(element: &Element, id_tag: &str) -> Option<ItemRef> {
    element.child(id_tag).and_then(ItemRef::from_element)
}

impl EwsClient {
    /// `GetFolder` for several folders; one result per folder in order
    /// (the folder element, or that folder's error).
    pub async fn get_folders(
        &self,
        mailbox: Option<&str>,
        folders: &[FolderRef<'_>],
    ) -> Result<Vec<Result<Element, EwsError>>, EwsError> {
        if folders.is_empty() {
            return Ok(Vec::new());
        }
        let ids: String = folders.iter().map(FolderRef::xml).collect();
        let body =
            format!("<m:GetFolder>{FOLDER_SHAPE}<m:FolderIds>{ids}</m:FolderIds></m:GetFolder>");
        let response = self
            .call(&Operation::read("GetFolder", body).anchored(mailbox))
            .await?;
        Ok(soap::response_messages(&response.body)
            .into_iter()
            .map(|m| {
                m.check()?;
                m.element
                    .child("Folders")
                    .and_then(|f| f.children.first())
                    .cloned()
                    .ok_or_else(|| EwsError::Protocol("GetFolder without a folder".to_owned()))
            })
            .collect())
    }

    /// `SyncFolderHierarchy` below the mailbox root (`msgfolderroot`).
    pub async fn sync_folder_hierarchy(
        &self,
        mailbox: Option<&str>,
        sync_state: Option<&str>,
    ) -> Result<SyncOutcome<FolderChanges>, EwsError> {
        let state = sync_state.map_or_else(String::new, |s| el("m:SyncState", s));
        let body = format!(
            "<m:SyncFolderHierarchy>{FOLDER_SHAPE}<m:SyncFolderId>{}</m:SyncFolderId>{state}</m:SyncFolderHierarchy>",
            FolderRef::distinguished("msgfolderroot", mailbox).xml()
        );
        let response = self
            .call(&Operation::read("SyncFolderHierarchy", body).anchored(mailbox))
            .await?;
        let messages = soap::response_messages(&response.body);
        let message = messages
            .first()
            .ok_or_else(|| EwsError::Protocol("SyncFolderHierarchy without an answer".into()))?;
        if message.is_error() && sync_expired(message) {
            return Ok(SyncOutcome::Expired);
        }
        message.check()?;
        let element = message.element;
        let mut out = FolderChanges {
            sync_state: element.child_value("SyncState").unwrap_or_default(),
            complete: element
                .child_bool("IncludesLastFolderInRange")
                .unwrap_or(true),
            ..FolderChanges::default()
        };
        if let Some(changes) = element.child("Changes") {
            for change in &changes.children {
                match change.name.as_str() {
                    "Create" | "Update" => out.upserted.extend(change.children.first().cloned()),
                    "Delete" => out
                        .deleted
                        .extend(element_id(change, "FolderId").map(|r| r.id)),
                    _ => {}
                }
            }
        }
        Ok(SyncOutcome::Changes(out))
    }

    /// `SyncFolderItems` of one folder with the item shape `shape`.
    pub async fn sync_folder_items(
        &self,
        mailbox: Option<&str>,
        folder: FolderRef<'_>,
        sync_state: Option<&str>,
        shape: &str,
        max_changes: u32,
    ) -> Result<SyncOutcome<ItemChanges>, EwsError> {
        let state = sync_state.map_or_else(String::new, |s| el("m:SyncState", s));
        let body = format!(
            "<m:SyncFolderItems>{shape}<m:SyncFolderId>{}</m:SyncFolderId>{state}<m:MaxChangesReturned>{}</m:MaxChangesReturned><m:SyncScope>NormalItems</m:SyncScope></m:SyncFolderItems>",
            folder.xml(),
            max_changes.clamp(1, MAX_SYNC_CHANGES)
        );
        let response = self
            .call(&Operation::read("SyncFolderItems", body).anchored(mailbox))
            .await?;
        let messages = soap::response_messages(&response.body);
        let message = messages
            .first()
            .ok_or_else(|| EwsError::Protocol("SyncFolderItems without an answer".into()))?;
        if message.is_error() && sync_expired(message) {
            return Ok(SyncOutcome::Expired);
        }
        message.check()?;
        let element = message.element;
        let mut out = ItemChanges {
            sync_state: element.child_value("SyncState").unwrap_or_default(),
            complete: element
                .child_bool("IncludesLastItemInRange")
                .unwrap_or(true),
            ..ItemChanges::default()
        };
        if let Some(changes) = element.child("Changes") {
            for change in &changes.children {
                match change.name.as_str() {
                    "Create" => out.created.extend(change.children.first().cloned()),
                    "Update" => out.updated.extend(change.children.first().cloned()),
                    "Delete" => out
                        .deleted
                        .extend(element_id(change, "ItemId").map(|r| r.id)),
                    "ReadFlagChange" => {
                        if let Some(id) = element_id(change, "ItemId") {
                            out.read_changes
                                .push((id.id, change.child_bool("IsRead").unwrap_or(false)));
                        }
                    }
                    _ => {}
                }
            }
        }
        Ok(SyncOutcome::Changes(out))
    }

    /// `GetItem` of several items with `shape` (batched); one result per id
    /// in order (the item element, or that item's error).
    pub async fn get_items(
        &self,
        mailbox: Option<&str>,
        ids: &[ItemRef],
        shape: &str,
    ) -> Result<Vec<Result<Element, EwsError>>, EwsError> {
        let mut out = Vec::with_capacity(ids.len());
        for chunk in ids.chunks(GET_ITEM_BATCH) {
            let list: String = chunk.iter().map(ItemRef::xml).collect();
            let body = format!("<m:GetItem>{shape}<m:ItemIds>{list}</m:ItemIds></m:GetItem>");
            let response = self
                .call(&Operation::read("GetItem", body).anchored(mailbox))
                .await?;
            let messages = soap::response_messages(&response.body);
            if messages.len() != chunk.len() {
                return Err(EwsError::Protocol(format!(
                    "GetItem answered {} of {} items",
                    messages.len(),
                    chunk.len()
                )));
            }
            out.extend(messages.into_iter().map(|m| {
                m.check()?;
                first_item(m.element)
            }));
        }
        Ok(out)
    }

    /// One item with `shape`.
    pub async fn get_item(
        &self,
        mailbox: Option<&str>,
        id: &ItemRef,
        shape: &str,
    ) -> Result<Element, EwsError> {
        self.get_items(mailbox, std::slice::from_ref(id), shape)
            .await?
            .pop()
            .ok_or_else(|| EwsError::Protocol("GetItem without an answer".into()))?
    }

    /// The raw RFC 5322 bytes of a message (`MimeContent`, base64).
    pub async fn get_mime(&self, mailbox: Option<&str>, id: &str) -> Result<Vec<u8>, EwsError> {
        let shape = "<m:ItemShape><t:BaseShape>IdOnly</t:BaseShape><t:IncludeMimeContent>true</t:IncludeMimeContent></m:ItemShape>";
        let item = self.get_item(mailbox, &ItemRef::new(id), shape).await?;
        let mime = item
            .child("MimeContent")
            .ok_or_else(|| EwsError::Protocol("GetItem without MimeContent".into()))?;
        decode_base64(&mime.text)
    }

    /// `CreateItem` of a message from raw MIME, saved in `folder`
    /// (`SaveOnly`): answers the new item. A message that is not a draft
    /// gets `PR_MESSAGE_FLAGS` so Exchange does not show it as unsent.
    pub async fn create_mime_item(
        &self,
        mailbox: Option<&str>,
        folder: FolderRef<'_>,
        raw: &[u8],
        flags: Flags,
    ) -> Result<ItemRef, EwsError> {
        let mime = base64::engine::general_purpose::STANDARD.encode(raw);
        let mut item = format!(r#"<t:MimeContent CharacterSet="UTF-8">{mime}</t:MimeContent>"#);
        if !flags.draft {
            item.push_str(&extended_value(
                &MESSAGE_FLAGS,
                if flags.seen { "1" } else { "0" },
            ));
        }
        if flags.answered {
            item.push_str(&extended_value(&LAST_VERB, "102"));
        }
        if flags.flagged {
            item.push_str("<t:Flag><t:FlagStatus>Flagged</t:FlagStatus></t:Flag>");
        }
        item.push_str(&el("t:IsRead", if flags.seen { "true" } else { "false" }));
        let body = format!(
            r#"<m:CreateItem MessageDisposition="SaveOnly"><m:SavedItemFolderId>{}</m:SavedItemFolderId><m:Items><t:Message>{item}</t:Message></m:Items></m:CreateItem>"#,
            folder.xml()
        );
        let response = self
            .call(&Operation::write("CreateItem", body).anchored(mailbox))
            .await?;
        let message = soap::single_message(&response.body)?;
        first_item(message).and_then(|i| {
            element_id(&i, "ItemId")
                .ok_or_else(|| EwsError::Protocol("CreateItem without an item id".into()))
        })
    }

    /// Sends raw MIME (`CreateItem` `SendAndSaveCopy` into Sent Items, or
    /// `SendOnly`) from the user's own mailbox.
    pub async fn send_mime(&self, raw: &[u8], save_copy: bool) -> Result<(), EwsError> {
        self.send_mime_from(raw, save_copy, &SendingMailbox::Own)
            .await
    }

    /// Sends raw MIME as the user, as another mailbox (Send As: `From` is
    /// that mailbox, the copy goes to *its* Sent Items) or on its behalf
    /// (`From` is that mailbox, the message's `Sender` the user, the copy
    /// stays in the user's Sent Items). Exchange checks the permission
    /// (`ErrorSendAsDenied` → [`EwsError::Forbidden`]).
    pub async fn send_mime_from(
        &self,
        raw: &[u8],
        save_copy: bool,
        sending: &SendingMailbox,
    ) -> Result<(), EwsError> {
        let mime = base64::engine::general_purpose::STANDARD.encode(raw);
        let (from, saved_in) = match sending {
            SendingMailbox::Own => (String::new(), None),
            SendingMailbox::SendAs { mailbox } => (from_xml(mailbox), Some(mailbox.as_str())),
            SendingMailbox::OnBehalf { mailbox } => (from_xml(mailbox), None),
        };
        let body = if save_copy {
            format!(
                r#"<m:CreateItem MessageDisposition="SendAndSaveCopy"><m:SavedItemFolderId>{}</m:SavedItemFolderId><m:Items><t:Message><t:MimeContent CharacterSet="UTF-8">{mime}</t:MimeContent>{from}</t:Message></m:Items></m:CreateItem>"#,
                FolderRef::distinguished("sentitems", saved_in).xml()
            )
        } else {
            format!(
                r#"<m:CreateItem MessageDisposition="SendOnly"><m:Items><t:Message><t:MimeContent CharacterSet="UTF-8">{mime}</t:MimeContent>{from}</t:Message></m:Items></m:CreateItem>"#
            )
        };
        let response = self.call(&Operation::write("CreateItem", body)).await?;
        soap::single_message(&response.body)?;
        Ok(())
    }

    /// The transport headers of a message (`PR_TRANSPORT_MESSAGE_HEADERS`,
    /// `0x007D`); `None` when it has none (sent mail, drafts).
    pub async fn transport_headers(
        &self,
        mailbox: Option<&str>,
        id: &str,
    ) -> Result<Option<String>, EwsError> {
        let shape = r#"<m:ItemShape><t:BaseShape>IdOnly</t:BaseShape><t:AdditionalProperties><t:ExtendedFieldURI PropertyTag="0x007D" PropertyType="String"/></t:AdditionalProperties></m:ItemShape>"#;
        let item = self.get_item(mailbox, &ItemRef::new(id), shape).await?;
        Ok(extended_property(&item, "0x007D")
            .map(str::to_owned)
            .filter(|h| !h.trim().is_empty()))
    }

    /// `UpdateItem` of one item. With a change key the write fails with
    /// [`EwsError::Conflict`] when the item changed since
    /// (`NeverOverwrite`); without one it overwrites. Answers the item's
    /// id and new change key.
    pub async fn update_item(
        &self,
        mailbox: Option<&str>,
        item: &ItemRef,
        item_tag: &str,
        updates: &[Update],
        extra_attributes: &str,
    ) -> Result<ItemRef, EwsError> {
        let conflict = if item.change_key.is_some() {
            "NeverOverwrite"
        } else {
            "AlwaysOverwrite"
        };
        let changes: String = updates.iter().map(|u| u.xml(item_tag)).collect();
        let body = format!(
            r#"<m:UpdateItem ConflictResolution="{conflict}" MessageDisposition="SaveOnly"{extra_attributes}><m:ItemChanges><t:ItemChange>{}<t:Updates>{changes}</t:Updates></t:ItemChange></m:ItemChanges></m:UpdateItem>"#,
            item.xml()
        );
        let mut op = Operation::write("UpdateItem", body);
        // Absolute values (and change-key-guarded writes) may be repeated.
        op.idempotent = item.change_key.is_some() || updates_are_absolute(updates);
        let response = self.call(&op.anchored(mailbox)).await?;
        let message = soap::single_message(&response.body)?;
        if let Some(conflicts) = message
            .child("ConflictResults")
            .and_then(|c| c.child_number::<u32>("Count"))
            && conflicts > 0
        {
            return Err(EwsError::Conflict("UpdateItem reported conflicts".into()));
        }
        let new_ref = message
            .child("Items")
            .and_then(|items| items.children.first())
            .and_then(|i| element_id(i, "ItemId"));
        Ok(new_ref.unwrap_or_else(|| item.clone()))
    }

    /// Applies a flag / tag change to a message (tags are categories),
    /// re-reading the categories and retrying when the item changed in
    /// between. Answers the new change key.
    pub async fn apply_flag_change(
        &self,
        mailbox: Option<&str>,
        id: &str,
        change: &FlagChange,
    ) -> Result<ItemRef, EwsError> {
        let mut updates = flag_updates(change);
        if change.add_tags.is_empty() && change.remove_tags.is_empty() {
            if updates.is_empty() {
                return Ok(ItemRef::new(id));
            }
            return self
                .update_item(
                    mailbox,
                    &ItemRef::new(id),
                    "Message",
                    &updates,
                    r#" SuppressReadReceipts="true""#,
                )
                .await;
        }
        let shape = r#"<m:ItemShape><t:BaseShape>IdOnly</t:BaseShape><t:AdditionalProperties><t:FieldURI FieldURI="item:Categories"/></t:AdditionalProperties></m:ItemShape>"#;
        let mut attempt = 0;
        loop {
            let item = self.get_item(mailbox, &ItemRef::new(id), shape).await?;
            let current = element_id(&item, "ItemId").unwrap_or_else(|| ItemRef::new(id));
            let categories = merge_categories(
                &item.strings("Categories", "String"),
                &change.add_tags,
                &change.remove_tags,
            );
            updates.retain(|u| !is_categories(u));
            updates.push(categories_update(&categories));
            match self
                .update_item(
                    mailbox,
                    &current,
                    "Message",
                    &updates,
                    r#" SuppressReadReceipts="true""#,
                )
                .await
            {
                Err(EwsError::Conflict(_)) if attempt < 2 => attempt += 1,
                other => return other,
            }
        }
    }

    /// `MoveItem` (or `CopyItem`) into `folder`; answers the new item id.
    pub async fn move_item(
        &self,
        mailbox: Option<&str>,
        id: &str,
        folder: FolderRef<'_>,
        copy: bool,
    ) -> Result<ItemRef, EwsError> {
        let (op, what) = if copy {
            ("CopyItem", "CopyItem")
        } else {
            ("MoveItem", "MoveItem")
        };
        let body = format!(
            "<m:{op}><m:ToFolderId>{}</m:ToFolderId><m:ItemIds>{}</m:ItemIds><m:ReturnNewItemIds>true</m:ReturnNewItemIds></m:{op}>",
            folder.xml(),
            ItemRef::new(id).xml()
        );
        let response = self
            .call(&Operation::write(what, body).anchored(mailbox))
            .await?;
        let message = soap::single_message(&response.body)?;
        first_item(message).and_then(|i| {
            element_id(&i, "ItemId")
                .ok_or_else(|| EwsError::Protocol(format!("{op} without the new item id")))
        })
    }

    /// `UpdateItem` of many messages with the same absolute `updates` and
    /// no change keys (P3, ADR 0164: a conversation's read state or flag),
    /// [`MANY_ITEMS`] per call. One result per id, in order: the item's
    /// new reference, or its own error (`ErrorItemNotFound` →
    /// [`EwsError::NotFound`]). `Err` when a call as a whole failed.
    pub async fn update_items(
        &self,
        mailbox: Option<&str>,
        ids: &[&str],
        updates: &[Update],
    ) -> Result<Vec<Result<ItemRef, EwsError>>, EwsError> {
        let changes: String = updates.iter().map(|u| u.xml("Message")).collect();
        let mut out = Vec::with_capacity(ids.len());
        for chunk in ids.chunks(MANY_ITEMS) {
            let mut items = String::new();
            for id in chunk {
                items.push_str("<t:ItemChange>");
                items.push_str(&ItemRef::new(*id).xml());
                items.push_str("<t:Updates>");
                items.push_str(&changes);
                items.push_str("</t:Updates></t:ItemChange>");
            }
            let body = format!(
                r#"<m:UpdateItem ConflictResolution="AlwaysOverwrite" MessageDisposition="SaveOnly" SuppressReadReceipts="true"><m:ItemChanges>{items}</m:ItemChanges></m:UpdateItem>"#
            );
            let mut op = Operation::write("UpdateItem", body);
            op.idempotent = updates_are_absolute(updates);
            let response = self.call(&op.anchored(mailbox)).await?;
            let answers = soap::response_messages(&response.body);
            check_count("UpdateItem", answers.len(), chunk.len())?;
            for (id, answer) in chunk.iter().zip(answers) {
                out.push(answer.check().map(|()| {
                    answer
                        .element
                        .child("Items")
                        .and_then(|items| items.children.first())
                        .and_then(|i| element_id(i, "ItemId"))
                        .unwrap_or_else(|| ItemRef::new(*id))
                }));
            }
        }
        Ok(out)
    }

    /// `MoveItem` of many items into one folder ([`MANY_ITEMS`] per call).
    /// One result per id, in order: the new item id, or the item's own
    /// error. `Err` when a call as a whole failed (moves are not repeated).
    pub async fn move_items(
        &self,
        mailbox: Option<&str>,
        ids: &[&str],
        folder: FolderRef<'_>,
    ) -> Result<Vec<Result<ItemRef, EwsError>>, EwsError> {
        let mut out = Vec::with_capacity(ids.len());
        for chunk in ids.chunks(MANY_ITEMS) {
            let items: String = chunk.iter().map(|id| ItemRef::new(*id).xml()).collect();
            let body = format!(
                "<m:MoveItem><m:ToFolderId>{}</m:ToFolderId><m:ItemIds>{items}</m:ItemIds><m:ReturnNewItemIds>true</m:ReturnNewItemIds></m:MoveItem>",
                folder.xml()
            );
            let response = self
                .call(&Operation::write("MoveItem", body).anchored(mailbox))
                .await?;
            let answers = soap::response_messages(&response.body);
            check_count("MoveItem", answers.len(), chunk.len())?;
            for answer in answers {
                out.push(answer.check().and_then(|()| {
                    first_item(answer.element).and_then(|i| {
                        element_id(&i, "ItemId").ok_or_else(|| {
                            EwsError::Protocol("MoveItem without the new item id".into())
                        })
                    })
                }));
            }
        }
        Ok(out)
    }

    /// `DeleteItem` of many items (`HardDelete`, …; [`MANY_ITEMS`] per
    /// call). One result per id, in order.
    pub async fn delete_items(
        &self,
        mailbox: Option<&str>,
        ids: &[&str],
        delete_type: &str,
    ) -> Result<Vec<Result<(), EwsError>>, EwsError> {
        let mut out = Vec::with_capacity(ids.len());
        for chunk in ids.chunks(MANY_ITEMS) {
            let items: String = chunk.iter().map(|id| ItemRef::new(*id).xml()).collect();
            let body = format!(
                r#"<m:DeleteItem DeleteType="{delete_type}"><m:ItemIds>{items}</m:ItemIds></m:DeleteItem>"#
            );
            let mut op = Operation::write("DeleteItem", body);
            op.idempotent = true;
            let response = self.call(&op.anchored(mailbox)).await?;
            let answers = soap::response_messages(&response.body);
            check_count("DeleteItem", answers.len(), chunk.len())?;
            out.extend(answers.iter().map(ResponseMessage::check));
        }
        Ok(out)
    }

    /// `DeleteItem` (`HardDelete`, `SoftDelete` or `MoveToDeletedItems`),
    /// with `extra_attributes` for meetings and tasks.
    pub async fn delete_item(
        &self,
        mailbox: Option<&str>,
        id: &ItemRef,
        delete_type: &str,
        extra_attributes: &str,
    ) -> Result<(), EwsError> {
        let body = format!(
            r#"<m:DeleteItem DeleteType="{delete_type}"{extra_attributes}><m:ItemIds>{}</m:ItemIds></m:DeleteItem>"#,
            ItemRef::new(id.id.clone()).xml()
        );
        let mut op = Operation::write("DeleteItem", body);
        op.idempotent = true;
        let response = self.call(&op.anchored(mailbox)).await?;
        soap::single_message(&response.body)?;
        Ok(())
    }

    /// The id of a message in `folder` with this `Message-ID` (for
    /// idempotent appends), if any.
    pub async fn find_by_message_id(
        &self,
        mailbox: Option<&str>,
        folder: FolderRef<'_>,
        message_id: &str,
    ) -> Result<Option<ItemRef>, EwsError> {
        let body = format!(
            concat!(
                r#"<m:FindItem Traversal="Shallow"><m:ItemShape><t:BaseShape>IdOnly</t:BaseShape></m:ItemShape>"#,
                r#"<m:IndexedPageItemView MaxEntriesReturned="1" Offset="0" BasePoint="Beginning"/>"#,
                r#"<m:Restriction><t:IsEqualTo><t:FieldURI FieldURI="message:InternetMessageId"/>"#,
                r#"<t:FieldURIOrConstant><t:Constant Value="{}"/></t:FieldURIOrConstant></t:IsEqualTo></m:Restriction>"#,
                r#"<m:ParentFolderIds>{}</m:ParentFolderIds></m:FindItem>"#
            ),
            escape(message_id),
            folder.xml()
        );
        let response = self
            .call(&Operation::read("FindItem", body).anchored(mailbox))
            .await?;
        let message = soap::single_message(&response.body)?;
        Ok(message
            .path(&["RootFolder", "Items"])
            .and_then(|items| items.children.first())
            .and_then(|i| element_id(i, "ItemId")))
    }
}

/// Most items one multi-item request names ([`EwsClient::update_items`],
/// [`EwsClient::move_items`], [`EwsClient::delete_items`]).
pub const MANY_ITEMS: usize = 100;

/// A multi-item answer must answer every item.
fn check_count(what: &str, answered: usize, asked: usize) -> Result<(), EwsError> {
    if answered == asked {
        Ok(())
    } else {
        Err(EwsError::Protocol(format!(
            "{what} answered {answered} of {asked} items"
        )))
    }
}

fn updates_are_absolute(updates: &[Update]) -> bool {
    updates.iter().all(|u| match u {
        Update::Set(Field::Uri(uri), _) | Update::Delete(Field::Uri(uri)) => {
            matches!(*uri, "message:IsRead" | "item:Flag" | "item:Categories")
        }
        Update::Set(f, _) | Update::Delete(f) => *f == LAST_VERB,
    })
}

fn is_categories(update: &Update) -> bool {
    matches!(
        update,
        Update::Set(Field::Uri("item:Categories"), _)
            | Update::Delete(Field::Uri("item:Categories"))
    )
}

/// The first item element of a response message's `Items`.
pub(crate) fn first_item(message: &Element) -> Result<Element, EwsError> {
    message
        .child("Items")
        .and_then(|items| items.children.first())
        .cloned()
        .ok_or_else(|| EwsError::Protocol(format!("{} without an item", message.name)))
}

/// Decodes base64 content (line breaks and spaces allowed).
pub fn decode_base64(text: &str) -> Result<Vec<u8>, EwsError> {
    let clean: String = text.chars().filter(|c| !c.is_ascii_whitespace()).collect();
    base64::engine::general_purpose::STANDARD
        .decode(clean.as_bytes())
        .map_err(|e| EwsError::Protocol(format!("invalid base64: {e}")))
}

/// The system-flag part of a change as updates.
pub fn flag_updates(change: &FlagChange) -> Vec<Update> {
    let mut out = Vec::new();
    if let Some(seen) = change.seen {
        out.push(Update::Set(
            Field::Uri("message:IsRead"),
            el("t:IsRead", if seen { "true" } else { "false" }),
        ));
    }
    if let Some(flagged) = change.flagged {
        let status = if flagged { "Flagged" } else { "NotFlagged" };
        out.push(Update::Set(
            Field::Uri("item:Flag"),
            format!("<t:Flag><t:FlagStatus>{status}</t:FlagStatus></t:Flag>"),
        ));
    }
    if let Some(answered) = change.answered {
        out.push(if answered {
            Update::Set(LAST_VERB, extended_value(&LAST_VERB, "102"))
        } else {
            Update::Delete(LAST_VERB)
        });
    }
    out
}

/// The categories after adding and removing tags (case-insensitive, the
/// existing spelling kept, order kept).
pub fn merge_categories(current: &[String], add: &[String], remove: &[String]) -> Vec<String> {
    let mut out: Vec<String> = current
        .iter()
        .filter(|c| !remove.iter().any(|r| r.eq_ignore_ascii_case(c)))
        .cloned()
        .collect();
    for tag in add {
        if !out.iter().any(|c| c.eq_ignore_ascii_case(tag))
            && !remove.iter().any(|r| r.eq_ignore_ascii_case(tag))
        {
            out.push(tag.clone());
        }
    }
    out
}

/// The update that sets the categories (empty = remove the field).
pub fn categories_update(categories: &[String]) -> Update {
    if categories.is_empty() {
        Update::Delete(Field::Uri("item:Categories"))
    } else {
        Update::Set(Field::Uri("item:Categories"), categories_xml(categories))
    }
}

/// `<t:Categories><t:String>…</t:String>…</t:Categories>`.
pub fn categories_xml(categories: &[String]) -> String {
    let inner: String = categories.iter().map(|c| el("t:String", c)).collect();
    format!("<t:Categories>{inner}</t:Categories>")
}

/// The flags of a message element.
pub fn message_flags(item: &Element) -> Flags {
    let verb = extended_property(item, "0x1081").and_then(|v| v.parse::<i32>().ok());
    Flags {
        seen: item.child_bool("IsRead").unwrap_or(false),
        flagged: item
            .path(&["Flag", "FlagStatus"])
            .is_some_and(|s| s.text.trim() == "Flagged"),
        answered: matches!(verb, Some(102 | 103)),
        draft: item.child_bool("IsDraft").unwrap_or(false),
    }
}

/// The Focused Inbox classification of a message element.
pub fn inference(item: &Element) -> Option<InferenceClassification> {
    match item.child_text("InferenceClassification")? {
        t if t.eq_ignore_ascii_case("Focused") => Some(InferenceClassification::Focused),
        t if t.eq_ignore_ascii_case("Other") => Some(InferenceClassification::Other),
        _ => None,
    }
}

/// The Exchange data of a message element.
pub fn message_info(item: &Element, mailbox: Option<&str>) -> Option<ExchangeMessageInfo> {
    let id = element_id(item, "ItemId")?;
    Some(ExchangeMessageInfo {
        item_id: id.id,
        conversation_id: item
            .child("ConversationId")
            .and_then(|c| c.attr("Id"))
            .map(str::to_owned),
        inference: inference(item),
        mailbox: mailbox.map(str::to_ascii_lowercase),
    })
}

/// A message element as a [`RemoteMessage`] in `folder`.
pub fn remote_message(
    item: &Element,
    folder: &str,
    mailbox: Option<&str>,
) -> Option<RemoteMessage> {
    let id = element_id(item, "ItemId")?;
    let exchange = message_info(item, mailbox);
    Some(RemoteMessage {
        remote_id: id.id,
        thread_id: exchange.as_ref().and_then(|e| e.conversation_id.clone()),
        folder: item
            .child("ParentFolderId")
            .and_then(|p| p.attr("Id"))
            .map_or_else(|| folder.to_owned(), str::to_owned),
        flags: message_flags(item),
        tags: item.strings("Categories", "String"),
        internal_date: item.child_text("DateTimeReceived").and_then(unix),
        size: item.child_number("Size"),
        raw: item
            .child("MimeContent")
            .and_then(|m| decode_base64(&m.text).ok()),
        gmail: None,
        change_key: id.change_key,
        exchange,
    })
}

/// A changed message element as a [`RemoteMessageChange`].
pub fn remote_change(item: &Element, mailbox: Option<&str>) -> Option<RemoteMessageChange> {
    let id = element_id(item, "ItemId")?;
    Some(RemoteMessageChange {
        remote_id: id.id,
        folder: None,
        flags: Some(message_flags(item)),
        tags: Some(item.strings("Categories", "String")),
        gmail: None,
        change_key: id.change_key,
        exchange: message_info(item, mailbox),
    })
}

/// What a folder holds, from its element name and `FolderClass`.
pub fn folder_class(folder: &Element) -> ExchangeFolderClass {
    let class = folder.child_text("FolderClass").unwrap_or("");
    let by_class = |c: &str| {
        if c == "IPF.Note" || c.starts_with("IPF.Note.") || c.is_empty() {
            ExchangeFolderClass::Mail
        } else if c.starts_with("IPF.Appointment") {
            ExchangeFolderClass::Calendar
        } else if c.starts_with("IPF.Contact") {
            ExchangeFolderClass::Contacts
        } else if c.starts_with("IPF.Task") {
            ExchangeFolderClass::Tasks
        } else {
            ExchangeFolderClass::Other
        }
    };
    match folder.name.as_str() {
        "CalendarFolder" => ExchangeFolderClass::Calendar,
        "ContactsFolder" => ExchangeFolderClass::Contacts,
        "TasksFolder" => ExchangeFolderClass::Tasks,
        "SearchFolder" => ExchangeFolderClass::Other,
        _ => by_class(class),
    }
}

/// A folder element as an [`ExchangeFolder`]: `root` is the mailbox
/// root's id (children of the root have no parent), `well_known` names the
/// distinguished folders by id.
pub fn exchange_folder(
    folder: &Element,
    root: Option<&str>,
    well_known: &dyn Fn(&str) -> Option<ExchangeWellKnownFolder>,
) -> Option<ExchangeFolder> {
    let id = element_id(folder, "FolderId")?;
    let parent = folder
        .child("ParentFolderId")
        .and_then(|p| p.attr("Id"))
        .filter(|p| Some(*p) != root)
        .map(str::to_owned);
    let name = folder
        .child_text("DisplayName")
        .unwrap_or_default()
        .to_owned();
    let class = folder_class(folder);
    let known = well_known(&id.id).or_else(|| {
        (parent.is_none()
            && class == ExchangeFolderClass::Mail
            && name.eq_ignore_ascii_case("Archive"))
        .then_some(ExchangeWellKnownFolder::Archive)
    });
    Some(ExchangeFolder {
        remote_id: id.id,
        change_key: id.change_key,
        parent,
        name,
        well_known: known,
        class,
        total: folder.child_number("TotalCount"),
        unread: folder.child_number("UnreadCount"),
    })
}

/// Recipients of the address headers of a raw message (`To`, `Cc`,
/// `Bcc`), lowercase.
pub fn header_recipients(raw: &[u8]) -> Vec<String> {
    ["to", "cc", "bcc"]
        .iter()
        .flat_map(|h| header_addresses(raw, h))
        .collect()
}

/// The addresses of one address header (`from`, `sender`, `to`, …; every
/// occurrence), lowercase.
pub fn header_addresses(raw: &[u8], wanted: &str) -> Vec<String> {
    let head_end = find_header_end(raw);
    let head = String::from_utf8_lossy(&raw[..head_end]);
    let mut unfolded: Vec<String> = Vec::new();
    for line in head.split('\n') {
        let line = line.trim_end_matches('\r');
        if (line.starts_with(' ') || line.starts_with('\t')) && !unfolded.is_empty() {
            if let Some(last) = unfolded.last_mut() {
                last.push(' ');
                last.push_str(line.trim());
            }
        } else {
            unfolded.push(line.to_owned());
        }
    }
    let mut out = Vec::new();
    for header in unfolded {
        let Some((name, value)) = header.split_once(':') else {
            continue;
        };
        if !name.trim().eq_ignore_ascii_case(wanted) {
            continue;
        }
        out.extend(addresses(value));
    }
    out
}

/// Addresses in an address-list header value.
fn addresses(value: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = String::new();
    let mut chars = value.chars().peekable();
    let mut quoted = false;
    while let Some(c) = chars.next() {
        match c {
            '"' => quoted = !quoted,
            '\\' if quoted => {
                chars.next();
            }
            '<' if !quoted => {
                let mut address = String::new();
                for c in chars.by_ref() {
                    if c == '>' {
                        break;
                    }
                    address.push(c);
                }
                if address.contains('@') {
                    out.push(address.trim().to_ascii_lowercase());
                }
                rest.push(',');
            }
            '(' if !quoted => {
                for c in chars.by_ref() {
                    if c == ')' {
                        break;
                    }
                }
            }
            _ if !quoted => rest.push(c),
            _ => {}
        }
    }
    for part in rest.split([',', ';', ':']) {
        let part = part.trim();
        if part.contains('@') && !part.contains(' ') {
            out.push(part.to_ascii_lowercase());
        }
    }
    out
}

fn find_header_end(raw: &[u8]) -> usize {
    raw.windows(4)
        .position(|w| w == b"\r\n\r\n")
        .or_else(|| raw.windows(2).position(|w| w == b"\n\n"))
        .unwrap_or(raw.len())
}

/// The `Message-ID` of a raw message (without angle brackets).
pub fn message_id(raw: &[u8]) -> Option<String> {
    let head = String::from_utf8_lossy(&raw[..find_header_end(raw)]);
    let mut lines = head.split('\n').peekable();
    while let Some(line) = lines.next() {
        let line = line.trim_end_matches('\r');
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        if !name.trim().eq_ignore_ascii_case("message-id") {
            continue;
        }
        let mut value = value.trim().to_owned();
        while let Some(next) = lines.peek() {
            if next.starts_with(' ') || next.starts_with('\t') {
                value.push_str(next.trim());
                lines.next();
            } else {
                break;
            }
        }
        let id = value
            .trim()
            .trim_start_matches('<')
            .trim_end_matches('>')
            .trim();
        return (!id.is_empty()).then(|| id.to_owned());
    }
    None
}

/// Adds a `Bcc` header for envelope recipients the headers do not name
/// (Exchange sends to the MIME recipients, `Bcc` included).
pub fn with_envelope_recipients(raw: &[u8], recipients: &[String]) -> Result<Vec<u8>, EwsError> {
    let known = header_recipients(raw);
    let mut missing: Vec<String> = Vec::new();
    for r in recipients {
        let lower = r.trim().to_ascii_lowercase();
        if lower.contains(['\r', '\n', ',', '<', '>', '"']) || !lower.contains('@') {
            return Err(EwsError::Rejected {
                code: "InvalidRecipient".into(),
                message: "an envelope recipient is not a plain address".into(),
            });
        }
        if !known.contains(&lower) && !missing.contains(&lower) {
            missing.push(lower);
        }
    }
    if missing.is_empty() {
        return Ok(raw.to_vec());
    }
    let mut out = format!("Bcc: {}\r\n", missing.join(", ")).into_bytes();
    out.extend_from_slice(raw);
    Ok(out)
}

/// `<t:From>` naming `address`.
fn from_xml(address: &str) -> String {
    format!(
        "<t:From><t:Mailbox>{}</t:Mailbox></t:From>",
        el("t:EmailAddress", address)
    )
}

/// The header section of transport headers or a raw message: CRLF line
/// ends, unsafe lines dropped, ending with the empty line.
pub fn header_block(text: &[u8]) -> Vec<u8> {
    let end = text
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map(|i| i + 2)
        .or_else(|| text.windows(2).position(|w| w == b"\n\n").map(|i| i + 1))
        .unwrap_or(text.len());
    let head = String::from_utf8_lossy(&text[..end]);
    let mut out = String::new();
    for line in head.split('\n') {
        let line = line.trim_end_matches('\r');
        if line.is_empty() {
            continue;
        }
        let continuation = line.starts_with([' ', '\t']);
        // A header line needs a name: a stray line would end the headers.
        if !continuation
            && !line
                .split_once(':')
                .is_some_and(|(n, _)| !n.is_empty() && !n.contains(' '))
        {
            continue;
        }
        out.push_str(line);
        out.push_str("\r\n");
    }
    out.push_str("\r\n");
    out.into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xml::parse;
    use pretty_assertions::assert_eq;

    #[test]
    fn messages_map_to_remote_messages() {
        let item = parse(concat!(
            r#"<t:Message xmlns:t="t"><t:ItemId Id="M1" ChangeKey="CK1"/><t:ParentFolderId Id="F-inbox"/>"#,
            r#"<t:ItemClass>IPM.Note</t:ItemClass><t:DateTimeReceived>2026-09-30T08:15:00Z</t:DateTimeReceived>"#,
            r#"<t:Size>2048</t:Size><t:Categories><t:String>Work</t:String><t:String>Blue</t:String></t:Categories>"#,
            r#"<t:IsDraft>false</t:IsDraft><t:Flag><t:FlagStatus>Flagged</t:FlagStatus></t:Flag>"#,
            r#"<t:ConversationId Id="C1"/><t:IsRead>true</t:IsRead>"#,
            r#"<t:ExtendedProperty><t:ExtendedFieldURI PropertyTag="0x1081" PropertyType="Integer"/><t:Value>102</t:Value></t:ExtendedProperty>"#,
            r#"<t:InferenceClassification>Other</t:InferenceClassification></t:Message>"#
        ))
        .unwrap();
        let message = remote_message(&item, "F-x", Some("Shared@Corp.Example")).unwrap();
        assert_eq!(message.remote_id, "M1");
        assert_eq!(message.folder, "F-inbox");
        assert_eq!(message.thread_id.as_deref(), Some("C1"));
        assert_eq!(
            message.flags,
            Flags {
                seen: true,
                flagged: true,
                answered: true,
                draft: false
            }
        );
        assert_eq!(message.tags, ["Work", "Blue"]);
        assert_eq!(message.internal_date, unix("2026-09-30T08:15:00Z"));
        assert!(message.internal_date.is_some());
        assert_eq!(message.size, Some(2048));
        assert_eq!(message.change_key.as_deref(), Some("CK1"));
        let info = message.exchange.unwrap();
        assert_eq!(info.inference, Some(InferenceClassification::Other));
        assert_eq!(info.mailbox.as_deref(), Some("shared@corp.example"));
        let change = remote_change(&item, None).unwrap();
        assert_eq!(change.tags.unwrap().len(), 2);
        assert!(change.flags.unwrap().seen);
        assert!(change.folder.is_none());
    }

    #[test]
    fn flag_changes_become_updates() {
        let change = FlagChange {
            seen: Some(true),
            flagged: Some(false),
            answered: Some(true),
            ..FlagChange::default()
        };
        let updates = flag_updates(&change);
        assert_eq!(updates.len(), 3);
        let xml: String = updates.iter().map(|u| u.xml("Message")).collect();
        assert!(xml.contains("<t:IsRead>true</t:IsRead>"));
        assert!(xml.contains("<t:FlagStatus>NotFlagged</t:FlagStatus>"));
        assert!(xml.contains("<t:Value>102</t:Value>"));
        let off = flag_updates(&FlagChange {
            answered: Some(false),
            ..FlagChange::default()
        });
        assert_eq!(off, [Update::Delete(LAST_VERB)]);
        assert!(updates_are_absolute(&updates));
    }

    #[test]
    fn categories_merge_case_insensitively() {
        let current = vec!["Work".to_owned(), "Blue".to_owned()];
        assert_eq!(
            merge_categories(&current, &["work".into(), "New".into()], &["blue".into()]),
            ["Work", "New"]
        );
        assert_eq!(
            merge_categories(&[], &["a".into()], &["A".into()]),
            Vec::<String>::new()
        );
        assert_eq!(
            categories_update(&[]),
            Update::Delete(Field::Uri("item:Categories"))
        );
        assert_eq!(
            categories_xml(&["a&b".into()]),
            "<t:Categories><t:String>a&amp;b</t:String></t:Categories>"
        );
    }

    #[test]
    fn folders_map_with_classes_and_roles() {
        let inbox = parse(concat!(
            r#"<Folder><FolderId Id="F-inbox" ChangeKey="k"/><ParentFolderId Id="F-root"/>"#,
            r#"<FolderClass>IPF.Note</FolderClass><DisplayName>Inbox</DisplayName>"#,
            r#"<TotalCount>3</TotalCount><UnreadCount>1</UnreadCount></Folder>"#
        ))
        .unwrap();
        let known = |id: &str| (id == "F-inbox").then_some(ExchangeWellKnownFolder::Inbox);
        let folder = exchange_folder(&inbox, Some("F-root"), &known).unwrap();
        assert_eq!(folder.parent, None);
        assert_eq!(folder.well_known, Some(ExchangeWellKnownFolder::Inbox));
        assert_eq!(folder.class, ExchangeFolderClass::Mail);
        assert_eq!((folder.total, folder.unread), (Some(3), Some(1)));
        let archive = parse(
            r#"<Folder><FolderId Id="F-a"/><ParentFolderId Id="F-root"/><DisplayName>Archive</DisplayName></Folder>"#,
        )
        .unwrap();
        assert_eq!(
            exchange_folder(&archive, Some("F-root"), &known)
                .unwrap()
                .well_known,
            Some(ExchangeWellKnownFolder::Archive)
        );
        let sub = parse(
            r#"<CalendarFolder><FolderId Id="F-c"/><ParentFolderId Id="F-inbox"/><DisplayName>Team</DisplayName></CalendarFolder>"#,
        )
        .unwrap();
        let sub = exchange_folder(&sub, Some("F-root"), &known).unwrap();
        assert_eq!(sub.parent.as_deref(), Some("F-inbox"));
        assert_eq!(sub.class, ExchangeFolderClass::Calendar);
        for (class, want) in [
            ("IPF.Contact", ExchangeFolderClass::Contacts),
            ("IPF.Task", ExchangeFolderClass::Tasks),
            ("IPF.StickyNote", ExchangeFolderClass::Other),
            ("IPF.Note.OutlookHomepage", ExchangeFolderClass::Mail),
        ] {
            let f = parse(&format!(
                "<Folder><FolderClass>{class}</FolderClass></Folder>"
            ))
            .unwrap();
            assert_eq!(folder_class(&f), want, "{class}");
        }
    }

    #[test]
    fn header_recipients_and_bcc() {
        let raw = b"From: a@x\r\nTo: \"Doe, John\" <John@Example.org>, c@x\r\nCc: d@x (comment),\r\n\tE <e@x>\r\nMessage-ID:\r\n <m1@x>\r\nSubject: s\r\n\r\nTo: body@x\r\n";
        let found = header_recipients(raw);
        for a in ["john@example.org", "c@x", "d@x", "e@x"] {
            assert!(found.contains(&a.to_owned()), "{a}");
        }
        assert!(!found.contains(&"body@x".to_owned()));
        assert_eq!(message_id(raw).as_deref(), Some("m1@x"));
        let same = with_envelope_recipients(raw, &["C@x".into()]).unwrap();
        assert_eq!(same, raw);
        let out = with_envelope_recipients(raw, &["hidden@x".into(), "hidden@x".into()]).unwrap();
        assert!(out.starts_with(b"Bcc: hidden@x\r\nFrom: a@x"));
        assert!(with_envelope_recipients(raw, &["x@y\r\nX-Evil: 1".into()]).is_err());
        assert_eq!(message_id(b"Subject: x\r\n\r\n"), None);
    }

    #[test]
    fn base64_with_line_breaks() {
        assert_eq!(decode_base64("SGVs\r\nbG8=").unwrap(), b"Hello");
        assert!(decode_base64("@@@").is_err());
    }
}
