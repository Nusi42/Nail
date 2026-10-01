//! The mock server's operations: request parsing, faults, and one handler
//! per EWS operation, answering SOAP documents shaped like Exchange's.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use axum::body::{Body, Bytes};
use axum::http::{HeaderMap, Response, StatusCode, header};
use base64::Engine as _;
use chrono::NaiveDateTime;

use super::model::{
    Access, MeetingMessage, MockItem, Snapshot, State, Subscription, busy_periods, occurrence_id,
    occurrences, split_occurrence,
};
use super::{FaultKind, LoggedRequest, SentMessage, Shared, fixtures, zones};
use crate::soap::{MESSAGES_NS, SOAP_NS, TYPES_NS};
use crate::types::{format_utc, parse_time};
use crate::xml::{self, Element, el, escape};

/// What the HTTP layer sends.
pub(crate) enum Outcome {
    /// A SOAP document with a status.
    Soap(StatusCode, String),
    /// A bare HTTP status (with `Retry-After`).
    Http(StatusCode, Option<u32>),
    /// A `GetStreamingEvents` stream for these subscriptions.
    Stream(Vec<String>),
}

/// The request's context.
struct Ctx {
    zone: Option<zones::Zone>,
}

type OpResult = Result<String, Outcome>;

/// A sync change: its XML, the item id and what the snapshot records
/// (`None` = removed).
type PendingChange = (String, String, Option<(String, bool)>);

fn envelope(state: &State, body: &str) -> String {
    let (major, minor, build, revision) = state.version;
    format!(
        concat!(
            r#"<?xml version="1.0" encoding="utf-8"?>"#,
            r#"<s:Envelope xmlns:s="{soap}"><s:Header>"#,
            r#"<h:ServerVersionInfo MajorVersion="{major}" MinorVersion="{minor}" MajorBuildNumber="{build}" "#,
            r#"MinorBuildNumber="{revision}" Version="V2017_07_11" xmlns:h="{types}" "#,
            r#"xmlns="{types}" xmlns:xsd="http://www.w3.org/2001/XMLSchema" "#,
            r#"xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"/></s:Header>"#,
            r#"<s:Body xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" "#,
            r#"xmlns:xsd="http://www.w3.org/2001/XMLSchema" xmlns:m="{messages}" xmlns:t="{types}">"#,
            "{body}</s:Body></s:Envelope>"
        ),
        soap = SOAP_NS,
        types = TYPES_NS,
        messages = MESSAGES_NS,
        major = major,
        minor = minor,
        build = build,
        revision = revision,
        body = body
    )
}

/// A SOAP fault document.
pub(crate) fn fault(code: &str, message: &str, back_off_ms: Option<u64>) -> String {
    let back_off = back_off_ms
        .map(|ms| {
            format!(
                r#"<t:MessageXml xmlns:t="{TYPES_NS}"><t:Value Name="BackOffMilliseconds">{ms}</t:Value></t:MessageXml>"#
            )
        })
        .unwrap_or_default();
    format!(
        concat!(
            r#"<?xml version="1.0" encoding="utf-8"?>"#,
            r#"<s:Envelope xmlns:s="{soap}"><s:Body><s:Fault>"#,
            r#"<faultcode xmlns:a="{types}">a:{code}</faultcode>"#,
            r#"<faultstring xml:lang="en-US">{message}</faultstring><detail>"#,
            r#"<e:ResponseCode xmlns:e="http://schemas.microsoft.com/exchange/services/2006/errors">{code}</e:ResponseCode>"#,
            r#"<e:Message xmlns:e="http://schemas.microsoft.com/exchange/services/2006/errors">{message}</e:Message>"#,
            "{back_off}</detail></s:Fault></s:Body></s:Envelope>"
        ),
        soap = SOAP_NS,
        types = TYPES_NS,
        code = code,
        message = escape(message),
        back_off = back_off
    )
}

fn ok_message(op: &str, inner: &str) -> String {
    format!(
        r#"<m:{op}ResponseMessage ResponseClass="Success"><m:ResponseCode>NoError</m:ResponseCode>{inner}</m:{op}ResponseMessage>"#
    )
}

fn error_message(op: &str, code: &str, text: &str) -> String {
    format!(
        r#"<m:{op}ResponseMessage ResponseClass="Error"><m:MessageText>{}</m:MessageText><m:ResponseCode>{code}</m:ResponseCode><m:DescriptiveLinkKey>0</m:DescriptiveLinkKey></m:{op}ResponseMessage>"#,
        escape(text)
    )
}

fn messages(op: &str, list: &[String]) -> String {
    format!(
        "<m:{op}Response><m:ResponseMessages>{}</m:ResponseMessages></m:{op}Response>",
        list.concat()
    )
}

fn error_text(code: &str) -> &'static str {
    match code {
        "ErrorItemNotFound" => "The specified object was not found in the store.",
        "ErrorFolderNotFound" => "The specified folder could not be found in the store.",
        "ErrorAccessDenied" => "Access is denied. Check credentials and try again.",
        "ErrorNonExistentMailbox" => "The SMTP address has no mailbox associated with it.",
        "ErrorInvalidSyncStateData" => {
            "Synchronization state data is corrupt or otherwise invalid."
        }
        "ErrorIrresolvableConflict" => {
            "The send or update operation could not be performed because the change key passed in the request does not match the current change key for the item."
        }
        "ErrorSubscriptionNotFound" => "The specified subscription was not found.",
        _ => "The request failed.",
    }
}

/// Handles one request.
pub(crate) fn handle(
    state: &mut State,
    peer: SocketAddr,
    headers: &HeaderMap,
    body: &str,
) -> Outcome {
    let Ok(root) = xml::parse(body) else {
        return Outcome::Soap(
            StatusCode::BAD_REQUEST,
            fault(
                "ErrorSchemaValidation",
                "The request failed schema validation.",
                None,
            ),
        );
    };
    let version = root
        .path(&["Header", "RequestServerVersion"])
        .and_then(|v| v.attr("Version"))
        .unwrap_or("Exchange2007_SP1")
        .to_owned();
    let zone_id = root
        .path(&["Header", "TimeZoneContext", "TimeZoneDefinition"])
        .and_then(|z| z.attr("Id"))
        .map(str::to_owned);
    let Some(op) = root.child("Body").and_then(|b| b.children.first()) else {
        return Outcome::Soap(
            StatusCode::INTERNAL_SERVER_ERROR,
            fault(
                "ErrorSchemaValidation",
                "The request has no operation.",
                None,
            ),
        );
    };
    let name = op.name.clone();
    state.log.push(LoggedRequest {
        operation: name.clone(),
        version: version.clone(),
        anchor: headers
            .get("X-AnchorMailbox")
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned),
        time_zone: zone_id.clone(),
        auth_scheme: headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.split(' ').next())
            .map(str::to_owned),
        peer,
        body: op.to_xml(None),
    });
    if state.refused_versions.contains(&version) {
        return Outcome::Soap(
            StatusCode::INTERNAL_SERVER_ERROR,
            fault(
                "ErrorInvalidServerVersion",
                "The specified server version is invalid.",
                None,
            ),
        );
    }
    if let Some(i) = state
        .faults
        .iter()
        .position(|f| f.operation.as_deref().is_none_or(|o| o == name))
    {
        let fault_kind = state.faults[i].kind.clone();
        state.faults[i].times -= 1;
        if state.faults[i].times == 0 {
            state.faults.remove(i);
        }
        return match fault_kind {
            FaultKind::ServerBusy { back_off_ms } => Outcome::Soap(
                StatusCode::INTERNAL_SERVER_ERROR,
                fault(
                    "ErrorServerBusy",
                    "The server cannot service this request right now. Try again later.",
                    Some(back_off_ms),
                ),
            ),
            FaultKind::ServerBusyMessage { back_off_ms } => {
                let message = format!(
                    r#"<m:{name}ResponseMessage ResponseClass="Error"><m:MessageText>The server cannot service this request right now. Try again later.</m:MessageText><m:ResponseCode>ErrorServerBusy</m:ResponseCode><m:DescriptiveLinkKey>0</m:DescriptiveLinkKey><m:MessageXml><t:Value Name="BackOffMilliseconds">{back_off_ms}</t:Value></m:MessageXml></m:{name}ResponseMessage>"#
                );
                Outcome::Soap(
                    StatusCode::OK,
                    envelope(state, &messages(&name, &[message])),
                )
            }
            FaultKind::Http(status, retry_after) => Outcome::Http(
                StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
                retry_after,
            ),
            FaultKind::ResponseCode(code) => Outcome::Soap(
                StatusCode::OK,
                envelope(
                    state,
                    &messages(&name, &[error_message(&name, &code, error_text(&code))]),
                ),
            ),
        };
    }
    if let Some(i) = state.recorded.iter().position(|r| r.operation == name) {
        let document = state.recorded[i].document.clone();
        state.recorded[i].times -= 1;
        if state.recorded[i].times == 0 {
            state.recorded.remove(i);
        }
        let status = if document.contains(":Fault>") {
            StatusCode::INTERNAL_SERVER_ERROR
        } else {
            StatusCode::OK
        };
        return Outcome::Soap(status, document);
    }
    let ctx = Ctx {
        zone: zone_id.as_deref().and_then(zones::zone),
    };
    let result = match name.as_str() {
        "GetFolder" => get_folder(state, op),
        "SyncFolderHierarchy" => sync_folder_hierarchy(state, op),
        "SyncFolderItems" => sync_folder_items(state, op),
        "GetItem" => get_item(state, op),
        "CreateItem" => create_item(state, op, &ctx),
        "UpdateItem" => update_item(state, op, &ctx),
        "MoveItem" | "CopyItem" => move_or_copy(state, op, name == "CopyItem"),
        "DeleteItem" => delete_item(state, op),
        "FindItem" => find_item(state, op),
        "Subscribe" => subscribe(state, op),
        "Unsubscribe" => unsubscribe(state, op),
        "GetStreamingEvents" => get_streaming_events(state, op),
        "GetUserOofSettingsRequest" => get_oof(state, op),
        "SetUserOofSettingsRequest" => set_oof(state, op),
        "GetInboxRules" => get_inbox_rules(state),
        "UpdateInboxRules" => update_inbox_rules(state, op),
        "GetUserConfiguration" => get_user_configuration(state, op),
        "UpdateUserConfiguration" | "CreateUserConfiguration" => {
            put_user_configuration(state, op, name == "CreateUserConfiguration")
        }
        "GetRoomLists" => get_room_lists(state),
        "GetRooms" => get_rooms(state, op),
        "GetUserAvailabilityRequest" => get_user_availability(state, op),
        "ResolveNames" => resolve_names(state, op),
        "GetDelegate" => get_delegate(state),
        other => Err(Outcome::Soap(
            StatusCode::INTERNAL_SERVER_ERROR,
            fault(
                "ErrorInvalidRequest",
                &format!("The mock does not know {other}."),
                None,
            ),
        )),
    };
    match result {
        Ok(body) => Outcome::Soap(StatusCode::OK, envelope(state, &body)),
        Err(outcome) => outcome,
    }
}

// ---------------------------------------------------------------------------
// Folders

/// Resolves a `FolderId` / `DistinguishedFolderId` element to (mailbox,
/// folder index), checking access.
fn resolve_folder(state: &State, reference: &Element) -> Result<(usize, usize), &'static str> {
    let (m, f) = match reference.name.as_str() {
        "FolderId" => state
            .folder_location(reference.attr("Id").unwrap_or_default())
            .ok_or("ErrorFolderNotFound")?,
        "DistinguishedFolderId" => {
            let address = reference
                .path(&["Mailbox", "EmailAddress"])
                .map(|e| e.text.trim());
            let m = match state.mailbox_index(address) {
                Some(m) => m,
                None => return Err("ErrorNonExistentMailbox"),
            };
            let name = reference.attr("Id").unwrap_or_default();
            (
                m,
                state.distinguished(m, name).ok_or("ErrorFolderNotFound")?,
            )
        }
        _ => return Err("ErrorInvalidIdMalformed"),
    };
    let mailbox = &state.mailboxes[m];
    match mailbox.access {
        Access::Full => {}
        Access::Denied => return Err("ErrorAccessDenied"),
        Access::PimOnly => {
            let class = &mailbox.folders[f].class;
            if !(class.starts_with("IPF.Appointment")
                || class.starts_with("IPF.Contact")
                || class.starts_with("IPF.Task"))
            {
                return Err("ErrorAccessDenied");
            }
        }
    }
    Ok((m, f))
}

fn folder_xml(state: &State, m: usize, f: usize) -> String {
    let mailbox = &state.mailboxes[m];
    let folder = &mailbox.folders[f];
    let items: Vec<&MockItem> = mailbox
        .items
        .iter()
        .filter(|i| i.folder == folder.id)
        .collect();
    let unread = items
        .iter()
        .filter(|i| !i.is_read() && i.tag == "Message")
        .count();
    let children = mailbox
        .folders
        .iter()
        .filter(|c| c.parent.as_deref() == Some(&folder.id))
        .count();
    let parent = folder
        .parent
        .as_ref()
        .map(|p| format!(r#"<t:ParentFolderId Id="{p}"/>"#))
        .unwrap_or_default();
    format!(
        r#"<t:{tag}><t:FolderId Id="{id}" ChangeKey="{key}"/>{parent}<t:FolderClass>{class}</t:FolderClass><t:DisplayName>{name}</t:DisplayName><t:TotalCount>{total}</t:TotalCount><t:ChildFolderCount>{children}</t:ChildFolderCount><t:UnreadCount>{unread}</t:UnreadCount></t:{tag}>"#,
        tag = folder.tag(),
        id = folder.id,
        key = folder.change_key,
        class = escape(&folder.class),
        name = escape(&folder.name),
        total = items.len(),
    )
}

fn get_folder(state: &State, op: &Element) -> OpResult {
    let mut out = Vec::new();
    for reference in op
        .child("FolderIds")
        .map(|f| f.children.as_slice())
        .unwrap_or_default()
    {
        out.push(match resolve_folder(state, reference) {
            Ok((m, f)) => ok_message(
                "GetFolder",
                &format!("<m:Folders>{}</m:Folders>", folder_xml(state, m, f)),
            ),
            Err(code) => error_message("GetFolder", code, error_text(code)),
        });
    }
    Ok(messages("GetFolder", &out))
}

fn sync_folder_hierarchy(state: &mut State, op: &Element) -> OpResult {
    const OP: &str = "SyncFolderHierarchy";
    let target = op
        .child("SyncFolderId")
        .and_then(|s| s.children.first())
        .cloned()
        .unwrap_or_else(|| {
            let mut root = Element::new("DistinguishedFolderId");
            root.set_attr("Id", "msgfolderroot");
            root
        });
    let (m, root) = match resolve_folder(state, &target) {
        Ok(found) => found,
        Err(code) => return Ok(messages(OP, &[error_message(OP, code, error_text(code))])),
    };
    if state.mailboxes[m].access != Access::Full {
        return Ok(messages(
            OP,
            &[error_message(
                OP,
                "ErrorAccessDenied",
                error_text("ErrorAccessDenied"),
            )],
        ));
    }
    let root_id = state.mailboxes[m].folders[root].id.clone();
    let known: HashMap<String, String> = match op.child_text("SyncState") {
        None | Some("") => HashMap::new(),
        Some(token) => match state.hierarchy_states.get(token) {
            Some((r, known)) if *r == root_id => known.clone(),
            _ => {
                return Ok(messages(
                    OP,
                    &[error_message(
                        OP,
                        "ErrorInvalidSyncStateData",
                        error_text("ErrorInvalidSyncStateData"),
                    )],
                ));
            }
        },
    };
    // Every folder below the root.
    let mut below = vec![root_id.clone()];
    let mut i = 0;
    while i < below.len() {
        let parent = below[i].clone();
        below.extend(
            state.mailboxes[m]
                .folders
                .iter()
                .filter(|f| f.parent.as_deref() == Some(&parent))
                .map(|f| f.id.clone()),
        );
        i += 1;
    }
    let mut changes = String::new();
    let mut now: HashMap<String, String> = HashMap::new();
    for id in below.iter().skip(1) {
        let f = state.mailboxes[m]
            .folders
            .iter()
            .position(|x| &x.id == id)
            .unwrap_or_default();
        let folder = &state.mailboxes[m].folders[f];
        now.insert(id.clone(), folder.change_key.clone());
        match known.get(id) {
            None => changes.push_str(&format!("<t:Create>{}</t:Create>", folder_xml(state, m, f))),
            Some(key) if *key != folder.change_key => {
                changes.push_str(&format!("<t:Update>{}</t:Update>", folder_xml(state, m, f)));
            }
            Some(_) => {}
        }
    }
    let mut gone: Vec<&String> = known.keys().filter(|id| !now.contains_key(*id)).collect();
    gone.sort();
    for id in gone {
        changes.push_str(&format!(r#"<t:Delete><t:FolderId Id="{id}"/></t:Delete>"#));
    }
    let token = state.new_id("H4sIHierarchy");
    state.hierarchy_states.insert(token.clone(), (root_id, now));
    Ok(messages(
        OP,
        &[ok_message(
            OP,
            &format!(
                "<m:SyncState>{token}</m:SyncState><m:IncludesLastFolderInRange>true</m:IncludesLastFolderInRange><m:Changes>{changes}</m:Changes>"
            ),
        )],
    ))
}

fn sync_folder_items(state: &mut State, op: &Element) -> OpResult {
    const OP: &str = "SyncFolderItems";
    let Some(target) = op.child("SyncFolderId").and_then(|s| s.children.first()) else {
        return Ok(messages(
            OP,
            &[error_message(OP, "ErrorInvalidRequest", "no folder")],
        ));
    };
    let (m, f) = match resolve_folder(state, target) {
        Ok(found) => found,
        Err(code) => return Ok(messages(OP, &[error_message(OP, code, error_text(code))])),
    };
    let folder_id = state.mailboxes[m].folders[f].id.clone();
    let mut snapshot = match op.child_text("SyncState") {
        None | Some("") => Snapshot {
            folder: folder_id.clone(),
            ..Snapshot::default()
        },
        Some(token) => match state.sync_states.get(token) {
            Some(s) if s.folder == folder_id => s.clone(),
            _ => {
                return Ok(messages(
                    OP,
                    &[error_message(
                        OP,
                        "ErrorInvalidSyncStateData",
                        error_text("ErrorInvalidSyncStateData"),
                    )],
                ));
            }
        },
    };
    let max: usize = op.child_number("MaxChangesReturned").unwrap_or(512);
    let mut items: Vec<&MockItem> = state.mailboxes[m]
        .items
        .iter()
        .filter(|i| i.folder == folder_id)
        .collect();
    items.sort_by(|a, b| a.id.cmp(&b.id));
    let mut pending: Vec<PendingChange> = Vec::new();
    let mut gone: Vec<String> = snapshot
        .items
        .keys()
        .filter(|id| !items.iter().any(|i| &i.id == *id))
        .cloned()
        .collect();
    gone.sort();
    for id in gone {
        pending.push((
            format!(r#"<t:Delete><t:ItemId Id="{id}"/></t:Delete>"#),
            id,
            None,
        ));
    }
    for item in &items {
        let read = item.is_read();
        let entry = Some((item.change_key.clone(), read));
        match snapshot.items.get(&item.id) {
            None => pending.push((
                format!("<t:Create>{}</t:Create>", state.render_item(item, false)),
                item.id.clone(),
                entry,
            )),
            Some((key, _)) if *key != item.change_key => pending.push((
                format!("<t:Update>{}</t:Update>", state.render_item(item, false)),
                item.id.clone(),
                entry,
            )),
            Some((_, was_read)) if *was_read != read => pending.push((
                format!(
                    r#"<t:ReadFlagChange><t:ItemId Id="{}" ChangeKey="{}"/><t:IsRead>{read}</t:IsRead></t:ReadFlagChange>"#,
                    item.id, item.change_key
                ),
                item.id.clone(),
                entry,
            )),
            Some(_) => {}
        }
    }
    let complete = pending.len() <= max;
    let mut changes = String::new();
    for (xml, id, entry) in pending.into_iter().take(max) {
        changes.push_str(&xml);
        match entry {
            Some(entry) => {
                snapshot.items.insert(id, entry);
            }
            None => {
                snapshot.items.remove(&id);
            }
        }
    }
    let token = state.new_id("H4sIItems");
    state.sync_states.insert(token.clone(), snapshot);
    Ok(messages(
        OP,
        &[ok_message(
            OP,
            &format!(
                "<m:SyncState>{token}</m:SyncState><m:IncludesLastItemInRange>{complete}</m:IncludesLastItemInRange><m:Changes>{changes}</m:Changes>"
            ),
        )],
    ))
}

// ---------------------------------------------------------------------------
// Items

fn mailbox_allows(state: &State, m: usize, folder: &str) -> bool {
    let mailbox = &state.mailboxes[m];
    match mailbox.access {
        Access::Full => true,
        Access::Denied => false,
        Access::PimOnly => mailbox
            .folders
            .iter()
            .find(|f| f.id == folder)
            .is_some_and(|f| !f.class.starts_with("IPF.Note")),
    }
}

/// An item as a client sees it: a stored item, an occurrence or an
/// exception of a series.
fn render_any(state: &State, id: &str, mime: bool) -> Result<String, &'static str> {
    if let Some((m, i)) = state.item_location(id) {
        let item = &state.mailboxes[m].items[i];
        if !mailbox_allows(state, m, &item.folder) {
            return Err("ErrorAccessDenied");
        }
        return Ok(state.render_item(item, mime));
    }
    let (master, index) = split_occurrence(id).ok_or("ErrorItemNotFound")?;
    let (m, i) = state.item_location(master).ok_or("ErrorItemNotFound")?;
    let item = occurrence_item(&state.mailboxes[m].items[i], index).ok_or("ErrorItemNotFound")?;
    Ok(state.render_item(&item, false))
}

/// The occurrence `index` of a series as an item of its own.
fn occurrence_item(master: &MockItem, index: u32) -> Option<MockItem> {
    if master.deleted.contains(&index) {
        return None;
    }
    let (_, start, end) = occurrences(master, 1000)
        .into_iter()
        .find(|(i, _, _)| *i == index)?;
    let mut props = master.exceptions.get(&index).cloned().unwrap_or_else(|| {
        let mut p = master.props.clone();
        p.children
            .retain(|c| !matches!(c.name.as_str(), "Recurrence" | "CalendarItemType"));
        p
    });
    let mut item = MockItem {
        id: occurrence_id(&master.id, index),
        change_key: master.change_key.clone(),
        folder: master.folder.clone(),
        tag: "CalendarItem".into(),
        props: Element::new("CalendarItem"),
        mime: None,
        exceptions: Default::default(),
        deleted: Default::default(),
    };
    std::mem::swap(&mut item.props, &mut props);
    let exception = master.exceptions.contains_key(&index);
    if !exception {
        item.set("Start", &start);
        item.set("End", &end);
    }
    item.set("OriginalStart", &start);
    item.set(
        "CalendarItemType",
        if exception { "Exception" } else { "Occurrence" },
    );
    Some(item)
}

fn get_item(state: &State, op: &Element) -> OpResult {
    let mime = op
        .path(&["ItemShape", "IncludeMimeContent"])
        .is_some_and(|m| m.text.trim() == "true");
    let mut out = Vec::new();
    for id in op
        .child("ItemIds")
        .map(|f| f.children.as_slice())
        .unwrap_or_default()
    {
        let id = id.attr("Id").unwrap_or_default();
        out.push(match render_any(state, id, mime) {
            Ok(item) => ok_message("GetItem", &format!("<m:Items>{item}</m:Items>")),
            Err(code) => error_message("GetItem", code, error_text(code)),
        });
    }
    Ok(messages("GetItem", &out))
}

/// Converts wall times of a written item to UTC (the request's
/// `TimeZoneContext`, else UTC).
fn normalize_times(props: &mut Element, zone: Option<zones::Zone>) {
    for child in &mut props.children {
        if matches!(
            child.name.as_str(),
            "Start"
                | "End"
                | "OriginalStart"
                | "DueDate"
                | "StartDate"
                | "CompleteDate"
                | "ReminderDueBy"
        ) {
            let text = child.text.trim().to_owned();
            let has_offset = text.ends_with('Z')
                || text
                    .get(19..)
                    .is_some_and(|rest| rest.contains('+') || rest.contains('-'));
            let utc = if has_offset {
                parse_time(&text).map(format_utc)
            } else {
                NaiveDateTime::parse_from_str(text.get(..19).unwrap_or(&text), "%Y-%m-%dT%H:%M:%S")
                    .ok()
                    .map(|local| match zone {
                        Some(z) => format_utc(z.to_utc(local).and_utc()),
                        None => format_utc(local.and_utc()),
                    })
            };
            if let Some(utc) = utc {
                child.text = utc;
            }
        }
    }
}

fn attendees(props: &Element) -> Vec<String> {
    ["RequiredAttendees", "OptionalAttendees", "Resources"]
        .iter()
        .filter_map(|l| props.child(l))
        .flat_map(|l| l.children_named("Attendee"))
        .filter_map(|a| {
            a.path(&["Mailbox", "EmailAddress"])
                .map(|e| e.text.trim().to_ascii_lowercase())
        })
        .collect()
}

/// Sends meeting messages for a calendar item and lets rooms accept.
fn send_meeting(state: &mut State, m: usize, i: usize, kind: &str) {
    let rooms: Vec<String> = state
        .room_lists
        .iter()
        .flat_map(|(_, _, rooms)| rooms.iter().map(|(a, _)| a.clone()))
        .collect();
    let item = state.mailboxes[m].items[i].clone();
    let to = attendees(&item.props);
    if to.is_empty() {
        return;
    }
    state.meeting_messages.push(MeetingMessage {
        kind: kind.to_owned(),
        subject: item.text("Subject").unwrap_or_default().to_owned(),
        to: to.clone(),
        comment: None,
    });
    if kind == "Cancellation" {
        for room in to.iter().filter(|a| rooms.contains(a)) {
            let (start, end) = (
                item.text("Start").unwrap_or_default().to_owned(),
                item.text("End").unwrap_or_default().to_owned(),
            );
            if let Some(list) = state.free_busy.get_mut(room) {
                list.retain(|(s, e, _)| !(s == &start && e == &end));
            }
        }
        return;
    }
    for room in to.iter().filter(|a| rooms.contains(a)) {
        for (start, end, kind) in busy_periods(&item, "0000", "9999") {
            let list = state.free_busy.entry(room.clone()).or_default();
            if !list.contains(&(start.clone(), end.clone(), kind.clone())) {
                list.push((start, end, kind));
            }
        }
        // The room accepts.
        if let Some(resources) = state.mailboxes[m].items[i].props.child_mut("Resources") {
            for attendee in resources.children.iter_mut() {
                let matches = attendee
                    .path(&["Mailbox", "EmailAddress"])
                    .is_some_and(|e| e.text.trim().eq_ignore_ascii_case(room));
                if matches {
                    attendee.children.retain(|c| c.name != "ResponseType");
                    let mut response = Element::new("ResponseType");
                    response.text = "Accept".into();
                    attendee.children.push(response);
                }
            }
        }
    }
}

fn create_item(state: &mut State, op: &Element, ctx: &Ctx) -> OpResult {
    const OP: &str = "CreateItem";
    let disposition = op.attr("MessageDisposition").unwrap_or("SaveOnly");
    let send_invitations = op.attr("SendMeetingInvitations").unwrap_or("SendToNone");
    let saved_folder = op
        .child("SavedItemFolderId")
        .and_then(|s| s.children.first());
    let mut out = Vec::new();
    for item in op
        .child("Items")
        .map(|i| i.children.as_slice())
        .unwrap_or_default()
    {
        let message = match item.name.as_str() {
            "Message" => create_message(state, item, disposition, saved_folder),
            "CalendarItem" | "Task" | "Contact" | "DistributionList" => {
                create_pim(state, item, saved_folder, send_invitations, ctx)
            }
            "AcceptItem" | "TentativelyAcceptItem" | "DeclineItem" => {
                respond(state, item, disposition)
            }
            _ => Err("ErrorInvalidRequest"),
        };
        out.push(match message {
            Ok(inner) => ok_message(OP, &inner),
            Err(code) => error_message(OP, code, error_text(code)),
        });
    }
    Ok(messages(OP, &out))
}

fn default_folder(state: &State, m: usize, name: &str) -> Option<String> {
    state
        .distinguished(m, name)
        .map(|f| state.mailboxes[m].folders[f].id.clone())
}

fn create_message(
    state: &mut State,
    item: &Element,
    disposition: &str,
    saved_folder: Option<&Element>,
) -> Result<String, &'static str> {
    let raw = item
        .child("MimeContent")
        .map(|m| crate::mail::decode_base64(&m.text))
        .transpose()
        .map_err(|_| "ErrorMimeContentInvalid")?
        .ok_or("ErrorMimeContentInvalid")?;
    let own = state.mailbox_index(None).ok_or("ErrorNonExistentMailbox")?;
    if disposition != "SaveOnly" {
        let save = disposition == "SendAndSaveCopy";
        state.sent.push(SentMessage {
            raw: raw.clone(),
            saved: save,
        });
        if save {
            let (m, folder) = match saved_folder {
                Some(reference) => {
                    let (m, f) = resolve_folder(state, reference)?;
                    (m, state.mailboxes[m].folders[f].id.clone())
                }
                None => (
                    own,
                    default_folder(state, own, "sentitems").ok_or("ErrorFolderNotFound")?,
                ),
            };
            let props = fixtures::message_props(&raw, true, &[]);
            state.add_item(m, &folder, "Message", props, Some(raw));
        }
        return Ok("<m:Items/>".to_owned());
    }
    let (m, folder) = match saved_folder {
        Some(reference) => {
            let (m, f) = resolve_folder(state, reference)?;
            (m, state.mailboxes[m].folders[f].id.clone())
        }
        None => (
            own,
            default_folder(state, own, "drafts").ok_or("ErrorFolderNotFound")?,
        ),
    };
    let read = item.child_bool("IsRead").unwrap_or(false);
    let mut props = fixtures::message_props(&raw, read, &item.strings("Categories", "String"));
    let has_flags = crate::types::extended_property(item, "0x0E07").is_some();
    props.children.retain(|c| c.name != "IsDraft");
    let mut draft = Element::new("IsDraft");
    draft.text = (!has_flags).to_string();
    props.children.push(draft);
    for child in &item.children {
        let message_flags = child.name == "ExtendedProperty"
            && child
                .child("ExtendedFieldURI")
                .and_then(|u| u.attr("PropertyTag"))
                .is_some_and(|t| same_tag(t, "0x0E07"));
        if child.name == "Flag" || (child.name == "ExtendedProperty" && !message_flags) {
            props.children.push(child.clone());
        }
    }
    let id = state.add_item(m, &folder, "Message", props, Some(raw));
    let key = state.mailboxes[m]
        .items
        .last()
        .map(|i| i.change_key.clone())
        .unwrap_or_default();
    Ok(format!(
        r#"<m:Items><t:Message><t:ItemId Id="{id}" ChangeKey="{key}"/></t:Message></m:Items>"#
    ))
}

fn create_pim(
    state: &mut State,
    item: &Element,
    saved_folder: Option<&Element>,
    send: &str,
    ctx: &Ctx,
) -> Result<String, &'static str> {
    let own = state.mailbox_index(None).ok_or("ErrorNonExistentMailbox")?;
    let fallback = match item.name.as_str() {
        "CalendarItem" => "calendar",
        "Task" => "tasks",
        _ => "contacts",
    };
    let (m, folder) = match saved_folder {
        Some(reference) => {
            let (m, f) = resolve_folder(state, reference)?;
            (m, state.mailboxes[m].folders[f].id.clone())
        }
        None => (
            own,
            default_folder(state, own, fallback).ok_or("ErrorFolderNotFound")?,
        ),
    };
    let mut props = item.clone();
    normalize_times(&mut props, ctx.zone);
    if item.name == "CalendarItem" {
        let recurring = props.child("Recurrence").is_some();
        props.children.retain(|c| c.name != "CalendarItemType");
        let mut kind = Element::new("CalendarItemType");
        kind.text = if recurring {
            "RecurringMaster"
        } else {
            "Single"
        }
        .into();
        props.children.push(kind);
        if props.child("UID").is_none() {
            let mut uid = Element::new("UID");
            uid.text = format!("040000008200E00074C5B7101A82E008{:08}", state.next_id);
            props.children.push(uid);
        }
        if props.child("StartTimeZone").is_none()
            && let Some(zone) = ctx.zone
        {
            let mut tz = Element::new("StartTimeZone");
            tz.set_attr("Id", zone.id);
            props.children.push(tz);
        }
        let owner = state.mailboxes[m].clone();
        let has_attendees = !attendees(&props).is_empty();
        let mut organizer = Element::new("Organizer");
        let mut mailbox = Element::new("Mailbox");
        for (name, value) in [
            ("Name", owner.name.as_str()),
            ("EmailAddress", owner.address.as_str()),
        ] {
            let mut e = Element::new(name);
            e.text = value.to_owned();
            mailbox.children.push(e);
        }
        organizer.children.push(mailbox);
        props.children.push(organizer);
        for (name, value) in [
            ("MyResponseType", "Organizer"),
            ("IsOrganizer", "true"),
            ("IsMeeting", if has_attendees { "true" } else { "false" }),
            ("IsCancelled", "false"),
        ] {
            let mut e = Element::new(name);
            e.text = value.to_owned();
            props.children.push(e);
        }
    }
    let tag = item.name.clone();
    let id = state.add_item(m, &folder, &tag, props, None);
    let i = state.mailboxes[m].items.len() - 1;
    if tag == "CalendarItem" && send != "SendToNone" {
        send_meeting(state, m, i, "Request");
    }
    let key = state.mailboxes[m].items[i].change_key.clone();
    Ok(format!(
        r#"<m:Items><t:{tag}><t:ItemId Id="{id}" ChangeKey="{key}"/></t:{tag}></m:Items>"#
    ))
}

fn respond(state: &mut State, item: &Element, disposition: &str) -> Result<String, &'static str> {
    let reference = item.child("ReferenceItemId").ok_or("ErrorInvalidRequest")?;
    let id = reference.attr("Id").unwrap_or_default();
    let (m, i) = state.item_location(id).ok_or("ErrorItemNotFound")?;
    if let Some(key) = reference.attr("ChangeKey")
        && key != state.mailboxes[m].items[i].change_key
    {
        return Err("ErrorIrresolvableConflict");
    }
    let (response, kind) = match item.name.as_str() {
        "AcceptItem" => ("Accept", "Accept"),
        "TentativelyAcceptItem" => ("Tentative", "Tentative"),
        _ => ("Decline", "Decline"),
    };
    let key = state.new_key();
    let target = &mut state.mailboxes[m].items[i];
    target.set("MyResponseType", response);
    target.change_key = key;
    let subject = target.text("Subject").unwrap_or_default().to_owned();
    let organizer = target
        .props
        .path(&["Organizer", "Mailbox", "EmailAddress"])
        .map(|e| e.text.trim().to_owned())
        .unwrap_or_default();
    let folder = target.folder.clone();
    let comment = item.child_value("Body");
    state.item_event("ModifiedEvent", id, &folder, None);
    if response == "Decline"
        && let Some(trash) = default_folder(state, m, "deleteditems")
    {
        let _ = move_item(state, id, &trash, false);
    }
    if disposition == "SaveOnly" {
        let drafts = default_folder(state, m, "drafts").ok_or("ErrorFolderNotFound")?;
        let raw = format!(
            "Subject: {kind}: {subject}\r\nTo: {organizer}\r\n\r\n{}\r\n",
            comment.clone().unwrap_or_default()
        );
        let props = fixtures::message_props(raw.as_bytes(), true, &[]);
        let draft = state.add_item(m, &drafts, "Message", props, Some(raw.into_bytes()));
        return Ok(format!(
            r#"<m:Items><t:Message><t:ItemId Id="{draft}"/></t:Message></m:Items>"#
        ));
    }
    state.meeting_messages.push(MeetingMessage {
        kind: kind.to_owned(),
        subject,
        to: vec![organizer],
        comment,
    });
    Ok("<m:Items/>".to_owned())
}

/// The element name a field path writes.
fn field_name(path: &Element) -> (String, Option<String>, Option<String>) {
    match path.name.as_str() {
        "FieldURI" => {
            let uri = path.attr("FieldURI").unwrap_or_default();
            (uri.rsplit(':').next().unwrap_or(uri).to_owned(), None, None)
        }
        "IndexedFieldURI" => {
            let uri = path.attr("FieldURI").unwrap_or_default();
            let index = path.attr("FieldIndex").map(str::to_owned);
            let dictionary = match uri {
                "contacts:EmailAddress" => "EmailAddresses".to_owned(),
                "contacts:PhoneNumber" => "PhoneNumbers".to_owned(),
                "contacts:ImAddress" => "ImAddresses".to_owned(),
                _ => "PhysicalAddresses".to_owned(),
            };
            let part = uri
                .strip_prefix("contacts:PhysicalAddress:")
                .map(str::to_owned);
            (dictionary, index, part)
        }
        _ => (
            "ExtendedProperty".to_owned(),
            path.attr("PropertyTag").map(str::to_owned),
            None,
        ),
    }
}

fn same_tag(a: &str, b: &str) -> bool {
    let parse = |t: &str| match t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        Some(hex) => u32::from_str_radix(hex, 16).ok(),
        None => t.parse().ok(),
    };
    parse(a).is_some() && parse(a) == parse(b)
}

/// Applies one `SetItemField` / `DeleteItemField` to an item element.
fn apply_update(props: &mut Element, update: &Element) {
    let Some(path) = update.children.first() else {
        return;
    };
    let (name, index, part) = field_name(path);
    let value = update
        .children
        .get(1)
        .and_then(|item| item.children.first())
        .cloned();
    let delete = update.name == "DeleteItemField";
    match (name.as_str(), index) {
        ("ExtendedProperty", Some(tag)) => {
            props.children.retain(|c| {
                !(c.name == "ExtendedProperty"
                    && c.child("ExtendedFieldURI")
                        .and_then(|u| u.attr("PropertyTag"))
                        .is_some_and(|t| same_tag(t, &tag)))
            });
            if let (false, Some(value)) = (delete, value) {
                props.children.push(value);
            }
        }
        (dictionary, Some(key)) => {
            if props.child(dictionary).is_none() {
                props.children.push(Element::new(dictionary));
            }
            let Some(dict) = props.child_mut(dictionary) else {
                return;
            };
            let existing = dict
                .children
                .iter()
                .position(|e| e.attr("Key") == Some(key.as_str()));
            match (delete, part) {
                (true, None) => {
                    if let Some(i) = existing {
                        dict.children.remove(i);
                    }
                }
                (true, Some(part)) => {
                    if let Some(i) = existing {
                        dict.children[i].children.retain(|c| c.name != part);
                    }
                }
                (false, part) => {
                    let Some(entry) = value.and_then(|v| v.children.into_iter().next()) else {
                        return;
                    };
                    match (existing, part) {
                        (Some(i), Some(_)) => {
                            for child in entry.children {
                                dict.children[i].children.retain(|c| c.name != child.name);
                                dict.children[i].children.push(child);
                            }
                        }
                        (Some(i), None) => dict.children[i] = entry,
                        (None, _) => dict.children.push(entry),
                    }
                }
            }
        }
        (name, None) => {
            props.children.retain(|c| c.name != name);
            if let (false, Some(value)) = (delete, value) {
                props.children.push(value);
            }
        }
    }
}

fn update_item(state: &mut State, op: &Element, ctx: &Ctx) -> OpResult {
    const OP: &str = "UpdateItem";
    let conflict = op.attr("ConflictResolution").unwrap_or("AutoResolve");
    let send = op
        .attr("SendMeetingInvitationsOrCancellations")
        .unwrap_or("SendToNone")
        .to_owned();
    let mut out = Vec::new();
    for change in op
        .child("ItemChanges")
        .map(|c| c.children.as_slice())
        .unwrap_or_default()
    {
        let Some(reference) = change.child("ItemId") else {
            out.push(error_message(OP, "ErrorInvalidRequest", "no item id"));
            continue;
        };
        let id = reference.attr("Id").unwrap_or_default().to_owned();
        let updates: Vec<Element> = change
            .child("Updates")
            .map(|u| u.children.clone())
            .unwrap_or_default();
        let result = apply_item_change(
            state,
            &id,
            reference.attr("ChangeKey"),
            conflict,
            &updates,
            ctx,
            &send,
        );
        out.push(match result {
            Ok((tag, new_id, key)) => ok_message(
                OP,
                &format!(
                    r#"<m:Items><t:{tag}><t:ItemId Id="{new_id}" ChangeKey="{key}"/></t:{tag}></m:Items><m:ConflictResults><t:Count>0</t:Count></m:ConflictResults>"#
                ),
            ),
            Err(code) => error_message(OP, code, error_text(code)),
        });
    }
    Ok(messages(OP, &out))
}

fn apply_item_change(
    state: &mut State,
    id: &str,
    change_key: Option<&str>,
    conflict: &str,
    updates: &[Element],
    ctx: &Ctx,
    send: &str,
) -> Result<(String, String, String), &'static str> {
    let key = state.new_key();
    if let Some((m, i)) = state.item_location(id) {
        if !mailbox_allows(state, m, &state.mailboxes[m].items[i].folder.clone()) {
            return Err("ErrorAccessDenied");
        }
        let item = &mut state.mailboxes[m].items[i];
        if conflict == "NeverOverwrite" && change_key.is_some_and(|k| k != item.change_key) {
            return Err("ErrorIrresolvableConflict");
        }
        for update in updates {
            apply_update(&mut item.props, update);
        }
        normalize_times(&mut item.props, ctx.zone);
        if item.tag == "CalendarItem" {
            let recurring = item.props.child("Recurrence").is_some();
            if item
                .text("CalendarItemType")
                .is_some_and(|t| t == "Single" || t == "RecurringMaster")
            {
                item.set(
                    "CalendarItemType",
                    if recurring {
                        "RecurringMaster"
                    } else {
                        "Single"
                    },
                );
            }
        }
        item.change_key = key.clone();
        let tag = item.tag.clone();
        let folder = item.folder.clone();
        state.item_event("ModifiedEvent", id, &folder, None);
        if tag == "CalendarItem" && send != "SendToNone" {
            send_meeting(state, m, i, "Update");
        }
        return Ok((tag, id.to_owned(), key));
    }
    // An occurrence: it becomes an exception of its series.
    let (master, index) = split_occurrence(id).ok_or("ErrorItemNotFound")?;
    let (m, i) = state.item_location(master).ok_or("ErrorItemNotFound")?;
    let occurrence =
        occurrence_item(&state.mailboxes[m].items[i], index).ok_or("ErrorItemNotFound")?;
    let mut props = occurrence.props;
    for update in updates {
        apply_update(&mut props, update);
    }
    normalize_times(&mut props, ctx.zone);
    let series = &mut state.mailboxes[m].items[i];
    if conflict == "NeverOverwrite" && change_key.is_some_and(|k| k != series.change_key) {
        return Err("ErrorIrresolvableConflict");
    }
    series.exceptions.insert(index, props);
    series.change_key = key.clone();
    let folder = series.folder.clone();
    state.item_event("ModifiedEvent", master, &folder, None);
    Ok(("CalendarItem".to_owned(), id.to_owned(), key))
}

/// Moves (or copies) an item; answers the new id.
pub(crate) fn move_item(
    state: &mut State,
    id: &str,
    folder: &str,
    copy: bool,
) -> Result<String, &'static str> {
    let (m, i) = state.item_location(id).ok_or("ErrorItemNotFound")?;
    let (target_m, _) = state.folder_location(folder).ok_or("ErrorFolderNotFound")?;
    let new_id = state.new_id("AAMkItem");
    let key = state.new_key();
    let mut item = if copy {
        state.mailboxes[m].items[i].clone()
    } else {
        state.mailboxes[m].items.remove(i)
    };
    let old_folder = item.folder.clone();
    item.id = new_id.clone();
    item.change_key = key;
    item.folder = folder.to_owned();
    state.mailboxes[target_m].items.push(item);
    if copy {
        state.item_event("CopiedEvent", &new_id, folder, Some(&old_folder));
    } else {
        state.item_event("MovedEvent", &new_id, folder, Some(&old_folder));
    }
    Ok(new_id)
}

fn move_or_copy(state: &mut State, op: &Element, copy: bool) -> OpResult {
    let name = if copy { "CopyItem" } else { "MoveItem" };
    let target = op.child("ToFolderId").and_then(|t| t.children.first());
    let folder = match target.map(|t| resolve_folder(state, t)) {
        Some(Ok((m, f))) => state.mailboxes[m].folders[f].id.clone(),
        Some(Err(code)) => {
            return Ok(messages(
                name,
                &[error_message(name, code, error_text(code))],
            ));
        }
        None => {
            return Ok(messages(
                name,
                &[error_message(name, "ErrorInvalidRequest", "no folder")],
            ));
        }
    };
    let mut out = Vec::new();
    for reference in op
        .child("ItemIds")
        .map(|i| i.children.as_slice())
        .unwrap_or_default()
    {
        let id = reference.attr("Id").unwrap_or_default();
        out.push(match move_item(state, id, &folder, copy) {
            Ok(new_id) => {
                let (m, i) = state.item_location(&new_id).unwrap_or_default();
                let item = &state.mailboxes[m].items[i];
                ok_message(
                    name,
                    &format!(
                        r#"<m:Items><t:{tag}><t:ItemId Id="{new_id}" ChangeKey="{key}"/></t:{tag}></m:Items>"#,
                        tag = item.tag,
                        key = item.change_key
                    ),
                )
            }
            Err(code) => error_message(name, code, error_text(code)),
        });
    }
    Ok(messages(name, &out))
}

fn delete_item(state: &mut State, op: &Element) -> OpResult {
    const OP: &str = "DeleteItem";
    let delete_type = op.attr("DeleteType").unwrap_or("HardDelete").to_owned();
    let cancellations = op
        .attr("SendMeetingCancellations")
        .unwrap_or("SendToNone")
        .to_owned();
    let mut out = Vec::new();
    for reference in op
        .child("ItemIds")
        .map(|i| i.children.as_slice())
        .unwrap_or_default()
    {
        let id = reference.attr("Id").unwrap_or_default();
        let result: Result<(), &'static str> = if let Some((m, i)) = state.item_location(id) {
            let item = state.mailboxes[m].items[i].clone();
            if item.tag == "CalendarItem"
                && cancellations != "SendToNone"
                && item.props.child_bool("IsOrganizer") == Some(true)
            {
                send_meeting(state, m, i, "Cancellation");
            }
            let trash = default_folder(state, m, "deleteditems");
            if delete_type == "MoveToDeletedItems" && trash.as_deref() != Some(&item.folder) {
                move_item(state, id, &trash.unwrap_or_default(), false).map(|_| ())
            } else {
                state.mailboxes[m].items.remove(i);
                state.item_event("DeletedEvent", id, &item.folder, None);
                Ok(())
            }
        } else if let Some((master, index)) = split_occurrence(id) {
            match state.item_location(master) {
                Some((m, i)) => {
                    let key = state.new_key();
                    let series = &mut state.mailboxes[m].items[i];
                    series.deleted.insert(index);
                    series.exceptions.remove(&index);
                    series.change_key = key;
                    let folder = series.folder.clone();
                    state.item_event("ModifiedEvent", master, &folder, None);
                    Ok(())
                }
                None => Err("ErrorItemNotFound"),
            }
        } else {
            Err("ErrorItemNotFound")
        };
        out.push(match result {
            Ok(()) => ok_message(OP, ""),
            Err(code) => error_message(OP, code, error_text(code)),
        });
    }
    Ok(messages(OP, &out))
}

fn find_item(state: &State, op: &Element) -> OpResult {
    const OP: &str = "FindItem";
    let Some(reference) = op.child("ParentFolderIds").and_then(|p| p.children.first()) else {
        return Ok(messages(
            OP,
            &[error_message(OP, "ErrorInvalidRequest", "no folder")],
        ));
    };
    let (m, f) = match resolve_folder(state, reference) {
        Ok(found) => found,
        Err(code) => return Ok(messages(OP, &[error_message(OP, code, error_text(code))])),
    };
    let folder = state.mailboxes[m].folders[f].id.clone();
    let mut found: Vec<String> = Vec::new();
    if let Some(view) = op.child("CalendarView") {
        let from = view
            .attr("StartDate")
            .and_then(parse_time)
            .map(format_utc)
            .unwrap_or_default();
        let to = view
            .attr("EndDate")
            .and_then(parse_time)
            .map(format_utc)
            .unwrap_or_default();
        let items: Vec<MockItem> = state.mailboxes[m]
            .items
            .iter()
            .filter(|i| i.folder == folder && i.tag == "CalendarItem")
            .cloned()
            .collect();
        for item in items {
            if item.props.child("Recurrence").is_none() {
                let (s, e) = (
                    item.text("Start").unwrap_or(""),
                    item.text("End").unwrap_or(""),
                );
                if s < to.as_str() && e > from.as_str() {
                    found.push(state.render_item(&item, false));
                }
                continue;
            }
            for (index, _, _) in occurrences(&item, 1000) {
                if let Some(occurrence) = occurrence_item(&item, index) {
                    let (s, e) = (
                        occurrence.text("Start").unwrap_or("").to_owned(),
                        occurrence.text("End").unwrap_or("").to_owned(),
                    );
                    if s.as_str() < to.as_str() && e.as_str() > from.as_str() {
                        found.push(state.render_item(&occurrence, false));
                    }
                }
            }
        }
    } else {
        let wanted = op
            .path(&["Restriction", "IsEqualTo"])
            .filter(|r| {
                r.child("FieldURI").and_then(|u| u.attr("FieldURI"))
                    == Some("message:InternetMessageId")
            })
            .and_then(|r| r.path(&["FieldURIOrConstant", "Constant"]))
            .and_then(|c| c.attr("Value"))
            .map(|v| v.trim_matches(['<', '>']).to_owned());
        for item in state.mailboxes[m]
            .items
            .iter()
            .filter(|i| i.folder == folder)
        {
            let matches = match &wanted {
                Some(id) => item
                    .text("InternetMessageId")
                    .is_some_and(|v| v.trim_matches(['<', '>']) == id),
                None => true,
            };
            if matches {
                found.push(format!(
                    r#"<t:{tag}><t:ItemId Id="{id}" ChangeKey="{key}"/></t:{tag}>"#,
                    tag = item.tag,
                    id = item.id,
                    key = item.change_key
                ));
            }
        }
    }
    Ok(messages(
        OP,
        &[ok_message(
            OP,
            &format!(
                r#"<m:RootFolder TotalItemsInView="{}" IncludesLastItemInRange="true"><t:Items>{}</t:Items></m:RootFolder>"#,
                found.len(),
                found.concat()
            ),
        )],
    ))
}

// ---------------------------------------------------------------------------
// Notifications

fn subscribe(state: &mut State, op: &Element) -> OpResult {
    const OP: &str = "Subscribe";
    let Some(request) = op.child("StreamingSubscriptionRequest") else {
        return Ok(messages(
            OP,
            &[error_message(
                OP,
                "ErrorInvalidSubscriptionRequest",
                "only streaming",
            )],
        ));
    };
    let mut folders = std::collections::HashSet::new();
    let mut mailbox = None;
    for reference in request
        .child("FolderIds")
        .map(|f| f.children.as_slice())
        .unwrap_or_default()
    {
        match resolve_folder(state, reference) {
            Ok((m, f)) => {
                mailbox = Some(state.mailboxes[m].address.clone());
                folders.insert(state.mailboxes[m].folders[f].id.clone());
            }
            Err(code) => return Ok(messages(OP, &[error_message(OP, code, error_text(code))])),
        }
    }
    let id = state.new_id("SUBjWVhNTUFDSE");
    state.subscriptions.insert(
        id.clone(),
        Subscription {
            mailbox: mailbox.unwrap_or_default(),
            folders,
            pending: Vec::new(),
            notify: Arc::new(tokio::sync::Notify::new()),
        },
    );
    Ok(messages(
        OP,
        &[ok_message(OP, &el("m:SubscriptionId", &id))],
    ))
}

fn unsubscribe(state: &mut State, op: &Element) -> OpResult {
    const OP: &str = "Unsubscribe";
    let id = op.child_text("SubscriptionId").unwrap_or_default();
    Ok(messages(
        OP,
        &[if state.subscriptions.remove(id).is_some() {
            ok_message(OP, "")
        } else {
            error_message(
                OP,
                "ErrorSubscriptionNotFound",
                error_text("ErrorSubscriptionNotFound"),
            )
        }],
    ))
}

fn missing_subscriptions(op_ids: &[String]) -> String {
    const OP: &str = "GetStreamingEvents";
    let ids: String = op_ids.iter().map(|i| el("m:SubscriptionId", i)).collect();
    format!(
        r#"<m:{OP}ResponseMessage ResponseClass="Error"><m:MessageText>{}</m:MessageText><m:ResponseCode>ErrorSubscriptionNotFound</m:ResponseCode><m:DescriptiveLinkKey>0</m:DescriptiveLinkKey><m:ErrorSubscriptionIds>{ids}</m:ErrorSubscriptionIds></m:{OP}ResponseMessage>"#,
        error_text("ErrorSubscriptionNotFound")
    )
}

fn get_streaming_events(state: &State, op: &Element) -> OpResult {
    let ids: Vec<String> = op
        .child("SubscriptionIds")
        .map(|l| {
            l.children
                .iter()
                .map(|c| c.text.trim().to_owned())
                .collect()
        })
        .unwrap_or_default();
    let missing: Vec<String> = ids
        .iter()
        .filter(|i| !state.subscriptions.contains_key(*i))
        .cloned()
        .collect();
    if !missing.is_empty() || ids.is_empty() {
        return Ok(messages(
            "GetStreamingEvents",
            &[missing_subscriptions(&missing)],
        ));
    }
    Err(Outcome::Stream(ids))
}

fn stream_envelope(state: &State, inner: &str) -> String {
    envelope(state, &messages("GetStreamingEvents", &[inner.to_owned()]))
}

/// The streaming answer: envelopes as events arrive, `Closed` after the
/// close time or [`super::MockEwsServer::close_streams`], a cut
/// connection for [`super::MockEwsServer::break_streams`].
pub(crate) fn stream(shared: Arc<Shared>, ids: Vec<String>) -> Response<Body> {
    let (tx, rx) = tokio::sync::mpsc::channel::<Result<Bytes, std::io::Error>>(16);
    let generation = shared.close_generation.load(Ordering::SeqCst);
    let close_after = shared.lock().stream_close_after;
    tokio::spawn(async move {
        let deadline = tokio::time::Instant::now() + close_after;
        // Exchange sends a first envelope right away.
        let first = {
            let state = shared.lock();
            stream_envelope(
                &state,
                &ok_message(
                    "GetStreamingEvents",
                    "<m:ConnectionStatus>OK</m:ConnectionStatus>",
                ),
            )
        };
        if tx.send(Ok(Bytes::from(first))).await.is_err() {
            return;
        }
        loop {
            let wake = shared.wake.notified();
            let (document, done, broken) = {
                let mut state = shared.lock();
                if state.break_streams > 0 {
                    state.break_streams -= 1;
                    (None, true, true)
                } else if ids.iter().any(|i| !state.subscriptions.contains_key(i)) {
                    let missing: Vec<String> = ids
                        .iter()
                        .filter(|i| !state.subscriptions.contains_key(*i))
                        .cloned()
                        .collect();
                    let doc = stream_envelope(&state, &missing_subscriptions(&missing));
                    (Some(doc), true, false)
                } else if shared.close_generation.load(Ordering::SeqCst) != generation
                    || tokio::time::Instant::now() >= deadline
                {
                    let doc = stream_envelope(
                        &state,
                        &ok_message(
                            "GetStreamingEvents",
                            "<m:ConnectionStatus>Closed</m:ConnectionStatus>",
                        ),
                    );
                    (Some(doc), true, false)
                } else {
                    let mut notifications = String::new();
                    for id in &ids {
                        if let Some(subscription) = state.subscriptions.get_mut(id)
                            && !subscription.pending.is_empty()
                        {
                            let events: String = subscription.pending.drain(..).collect();
                            notifications.push_str(&format!(
                                "<m:Notification>{}{events}</m:Notification>",
                                el("t:SubscriptionId", id)
                            ));
                        }
                    }
                    if notifications.is_empty() {
                        (None, false, false)
                    } else {
                        let doc = stream_envelope(
                            &state,
                            &ok_message(
                                "GetStreamingEvents",
                                &format!(
                                    "<m:Notifications>{notifications}</m:Notifications><m:ConnectionStatus>OK</m:ConnectionStatus>"
                                ),
                            ),
                        );
                        (Some(doc), false, false)
                    }
                }
            };
            if broken {
                let _ = tx
                    .send(Err(std::io::Error::other("the mock cut the connection")))
                    .await;
                return;
            }
            if let Some(document) = document
                && tx.send(Ok(Bytes::from(document))).await.is_err()
            {
                return;
            }
            if done {
                return;
            }
            tokio::select! {
                () = wake => {}
                () = tokio::time::sleep(Duration::from_millis(100)) => {}
                () = tokio::time::sleep_until(deadline) => {}
            }
        }
    });
    let body = futures_util::stream::unfold(rx, |mut rx| async move {
        rx.recv().await.map(|item| (item, rx))
    });
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/xml; charset=utf-8")
        .body(Body::from_stream(body))
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Settings

fn availability_response(op: &str, class_ok: bool, code: &str, inner: &str) -> String {
    let class = if class_ok { "Success" } else { "Error" };
    format!(
        r#"<{op}Response xmlns="{MESSAGES_NS}"><ResponseMessage ResponseClass="{class}"><ResponseCode>{code}</ResponseCode></ResponseMessage>{inner}</{op}Response>"#
    )
}

fn get_oof(state: &State, op: &Element) -> OpResult {
    let address = op
        .path(&["Mailbox", "Address"])
        .map(|a| a.text.trim().to_ascii_lowercase());
    if address.as_deref() != Some(state.own.as_str()) {
        return Ok(availability_response(
            "GetUserOofSettings",
            false,
            "ErrorAccessDenied",
            "",
        ));
    }
    let oof = &state.oof;
    let duration = oof
        .duration
        .as_ref()
        .map(|(s, e)| {
            format!(
                "<Duration>{}{}</Duration>",
                el("StartTime", s),
                el("EndTime", e)
            )
        })
        .unwrap_or_default();
    let inner = format!(
        r#"<OofSettings xmlns="{TYPES_NS}">{}{}{duration}<InternalReply>{}</InternalReply><ExternalReply>{}</ExternalReply></OofSettings><AllowExternalOof>All</AllowExternalOof>"#,
        el("OofState", &oof.state),
        el("ExternalAudience", &oof.audience),
        el("Message", &oof.internal),
        el("Message", &oof.external)
    );
    Ok(availability_response(
        "GetUserOofSettings",
        true,
        "NoError",
        &inner,
    ))
}

fn set_oof(state: &mut State, op: &Element) -> OpResult {
    let address = op
        .path(&["Mailbox", "Address"])
        .map(|a| a.text.trim().to_ascii_lowercase());
    if address.as_deref() != Some(state.own.as_str()) {
        return Ok(availability_response(
            "SetUserOofSettings",
            false,
            "ErrorAccessDenied",
            "",
        ));
    }
    let Some(settings) = op.child("UserOofSettings") else {
        return Ok(availability_response(
            "SetUserOofSettings",
            false,
            "ErrorInvalidRequest",
            "",
        ));
    };
    let state_value = settings
        .child_text("OofState")
        .unwrap_or("Disabled")
        .to_owned();
    if state_value == "Scheduled" && settings.child("Duration").is_none() {
        return Ok(availability_response(
            "SetUserOofSettings",
            false,
            "ErrorOofDurationInvalid",
            "",
        ));
    }
    state.oof = super::Oof {
        state: state_value,
        audience: settings
            .child_text("ExternalAudience")
            .unwrap_or("All")
            .to_owned(),
        duration: settings.child("Duration").and_then(|d| {
            Some((
                d.child_text("StartTime")?.to_owned(),
                d.child_text("EndTime")?.to_owned(),
            ))
        }),
        internal: settings
            .path(&["InternalReply", "Message"])
            .map(|m| m.text.clone())
            .unwrap_or_default(),
        external: settings
            .path(&["ExternalReply", "Message"])
            .map(|m| m.text.clone())
            .unwrap_or_default(),
    };
    Ok(availability_response(
        "SetUserOofSettings",
        true,
        "NoError",
        "",
    ))
}

fn get_inbox_rules(state: &State) -> OpResult {
    let rules: String = state.rules.iter().map(|r| r.to_xml(Some("t"))).collect();
    Ok(format!(
        r#"<m:GetInboxRulesResponse ResponseClass="Success"><m:ResponseCode>NoError</m:ResponseCode><m:OutlookRuleBlobExists>{}</m:OutlookRuleBlobExists><m:InboxRules>{rules}</m:InboxRules></m:GetInboxRulesResponse>"#,
        state.outlook_rule_blob
    ))
}

fn rules_error(code: &str, index: usize, message: &str) -> String {
    format!(
        r#"<m:UpdateInboxRulesResponse ResponseClass="Error"><m:MessageText>{}</m:MessageText><m:ResponseCode>{code}</m:ResponseCode><m:DescriptiveLinkKey>0</m:DescriptiveLinkKey><m:RuleOperationErrors><t:RuleOperationError><t:OperationIndex>{index}</t:OperationIndex><t:ValidationErrors><t:Error><t:FieldURI>RuleId</t:FieldURI><t:ErrorCode>RuleNotFound</t:ErrorCode><t:ErrorMessage>{}</t:ErrorMessage></t:Error></t:ValidationErrors></t:RuleOperationError></m:RuleOperationErrors></m:UpdateInboxRulesResponse>"#,
        escape(message),
        escape(message)
    )
}

fn update_inbox_rules(state: &mut State, op: &Element) -> OpResult {
    let remove_blob = op.child_bool("RemoveOutlookRuleBlob").unwrap_or(false);
    if state.outlook_rule_blob && !remove_blob {
        return Ok(format!(
            r#"<m:UpdateInboxRulesResponse ResponseClass="Error"><m:MessageText>{}</m:MessageText><m:ResponseCode>ErrorOutlookRuleBlobExists</m:ResponseCode><m:DescriptiveLinkKey>0</m:DescriptiveLinkKey></m:UpdateInboxRulesResponse>"#,
            "Outlook rules exist. Set RemoveOutlookRuleBlob to true to overwrite them."
        ));
    }
    if remove_blob {
        state.outlook_rule_blob = false;
    }
    let mut rules = state.rules.clone();
    for (index, operation) in op
        .child("Operations")
        .map(|o| o.children.as_slice())
        .unwrap_or_default()
        .iter()
        .enumerate()
    {
        match operation.name.as_str() {
            "CreateRuleOperation" => {
                let Some(mut rule) = operation.child("Rule").cloned() else {
                    continue;
                };
                if rule.child("Actions").is_none_or(|a| a.children.is_empty()) {
                    return Ok(rules_error(
                        "ErrorInboxRulesValidationError",
                        index,
                        "A rule needs an action.",
                    ));
                }
                let id = state.new_id("AQAAAJ5R");
                rule.children.retain(|c| c.name != "RuleId");
                let mut rule_id = Element::new("RuleId");
                rule_id.text = id;
                rule.children.insert(0, rule_id);
                for (name, value) in [("IsNotSupported", "false"), ("IsInError", "false")] {
                    let mut e = Element::new(name);
                    e.text = value.into();
                    rule.children.push(e);
                }
                rules.push(rule);
            }
            "SetRuleOperation" => {
                let Some(rule) = operation.child("Rule") else {
                    continue;
                };
                let id = rule.child_text("RuleId").unwrap_or_default();
                let Some(slot) = rules
                    .iter_mut()
                    .find(|r| r.child_text("RuleId") == Some(id))
                else {
                    return Ok(rules_error(
                        "ErrorInboxRulesValidationError",
                        index,
                        "The rule was not found.",
                    ));
                };
                let mut rule = rule.clone();
                for name in ["IsNotSupported", "IsInError"] {
                    if let Some(old) = slot.child(name).cloned() {
                        rule.children.push(old);
                    }
                }
                *slot = rule;
            }
            "DeleteRuleOperation" => {
                let id = operation.child_text("RuleId").unwrap_or_default();
                let before = rules.len();
                rules.retain(|r| r.child_text("RuleId") != Some(id));
                if rules.len() == before {
                    return Ok(rules_error(
                        "ErrorInboxRulesValidationError",
                        index,
                        "The rule was not found.",
                    ));
                }
            }
            _ => {}
        }
    }
    rules.sort_by_key(|r| r.child_number::<u32>("Priority").unwrap_or(1));
    state.rules = rules;
    Ok(r#"<m:UpdateInboxRulesResponse ResponseClass="Success"><m:ResponseCode>NoError</m:ResponseCode></m:UpdateInboxRulesResponse>"#.to_owned())
}

fn get_user_configuration(state: &State, op: &Element) -> OpResult {
    const OP: &str = "GetUserConfiguration";
    let name = op
        .child("UserConfigurationName")
        .and_then(|n| n.attr("Name"));
    match (name, &state.categories) {
        (Some("CategoryList"), Some(document)) => {
            let data = base64::engine::general_purpose::STANDARD.encode(document);
            Ok(messages(
                OP,
                &[ok_message(
                    OP,
                    &format!(
                        r#"<m:UserConfiguration><t:UserConfigurationName Name="CategoryList"><t:DistinguishedFolderId Id="calendar"/></t:UserConfigurationName><t:XmlData>{data}</t:XmlData></m:UserConfiguration>"#
                    ),
                )],
            ))
        }
        _ => Ok(messages(
            OP,
            &[error_message(
                OP,
                "ErrorItemNotFound",
                error_text("ErrorItemNotFound"),
            )],
        )),
    }
}

fn put_user_configuration(state: &mut State, op: &Element, create: bool) -> OpResult {
    let name = if create {
        "CreateUserConfiguration"
    } else {
        "UpdateUserConfiguration"
    };
    let config = op.child("UserConfiguration");
    let is_categories = config
        .and_then(|c| c.child("UserConfigurationName"))
        .and_then(|n| n.attr("Name"))
        == Some("CategoryList");
    if !is_categories {
        return Ok(messages(
            name,
            &[error_message(
                name,
                "ErrorItemNotFound",
                error_text("ErrorItemNotFound"),
            )],
        ));
    }
    if create == state.categories.is_some() {
        let code = if create {
            "ErrorItemSave"
        } else {
            "ErrorItemNotFound"
        };
        return Ok(messages(
            name,
            &[error_message(name, code, error_text(code))],
        ));
    }
    let data = config
        .and_then(|c| c.child_text("XmlData"))
        .and_then(|d| crate::mail::decode_base64(d).ok())
        .map(|b| String::from_utf8_lossy(&b).into_owned());
    match data {
        Some(document) if xml::parse(&document).is_ok() => {
            state.categories = Some(document);
            Ok(messages(name, &[ok_message(name, "")]))
        }
        _ => Ok(messages(
            name,
            &[error_message(
                name,
                "ErrorInvalidRequest",
                "invalid XmlData",
            )],
        )),
    }
}

// ---------------------------------------------------------------------------
// Directory

fn get_room_lists(state: &State) -> OpResult {
    if state.rooms_denied {
        return Ok(r#"<m:GetRoomListsResponse ResponseClass="Error"><m:MessageText>Access is denied.</m:MessageText><m:ResponseCode>ErrorAccessDenied</m:ResponseCode><m:DescriptiveLinkKey>0</m:DescriptiveLinkKey></m:GetRoomListsResponse>"#.to_owned());
    }
    let lists: String = state
        .room_lists
        .iter()
        .map(|(address, name, _)| {
            format!(
                "<t:Address>{}{}<t:RoutingType>SMTP</t:RoutingType><t:MailboxType>PublicDL</t:MailboxType></t:Address>",
                el("t:Name", name),
                el("t:EmailAddress", address)
            )
        })
        .collect();
    Ok(format!(
        r#"<m:GetRoomListsResponse ResponseClass="Success"><m:ResponseCode>NoError</m:ResponseCode><m:RoomLists>{lists}</m:RoomLists></m:GetRoomListsResponse>"#
    ))
}

fn get_rooms(state: &State, op: &Element) -> OpResult {
    let list = op
        .path(&["RoomList", "EmailAddress"])
        .map(|e| e.text.trim().to_ascii_lowercase())
        .unwrap_or_default();
    if state.rooms_denied {
        return Ok(r#"<m:GetRoomsResponse ResponseClass="Error"><m:MessageText>Access is denied.</m:MessageText><m:ResponseCode>ErrorAccessDenied</m:ResponseCode><m:DescriptiveLinkKey>0</m:DescriptiveLinkKey></m:GetRoomsResponse>"#.to_owned());
    }
    let Some((_, _, rooms)) = state.room_lists.iter().find(|(a, _, _)| *a == list) else {
        return Ok(r#"<m:GetRoomsResponse ResponseClass="Error"><m:MessageText>No results were found.</m:MessageText><m:ResponseCode>ErrorNameResolutionNoResults</m:ResponseCode><m:DescriptiveLinkKey>0</m:DescriptiveLinkKey></m:GetRoomsResponse>"#.to_owned());
    };
    let rooms: String = rooms
        .iter()
        .map(|(address, name)| {
            format!(
                "<t:Room><t:Id>{}{}<t:RoutingType>SMTP</t:RoutingType><t:MailboxType>Mailbox</t:MailboxType></t:Id></t:Room>",
                el("t:Name", name),
                el("t:EmailAddress", address)
            )
        })
        .collect();
    Ok(format!(
        r#"<m:GetRoomsResponse ResponseClass="Success"><m:ResponseCode>NoError</m:ResponseCode><m:Rooms>{rooms}</m:Rooms></m:GetRoomsResponse>"#
    ))
}

fn known_address(state: &State, address: &str) -> bool {
    state.mailbox_index(Some(address)).is_some()
        || state.directory.iter().any(|(a, _)| a == address)
        || state.free_busy.contains_key(address)
        || state
            .room_lists
            .iter()
            .any(|(_, _, rooms)| rooms.iter().any(|(a, _)| a == address))
}

fn get_user_availability(state: &State, op: &Element) -> OpResult {
    let window = op.path(&["FreeBusyViewOptions", "TimeWindow"]);
    let from = window
        .and_then(|w| w.child_text("StartTime"))
        .and_then(parse_time)
        .map(format_utc)
        .unwrap_or_default();
    let to = window
        .and_then(|w| w.child_text("EndTime"))
        .and_then(parse_time)
        .map(format_utc)
        .unwrap_or_default();
    let mut responses = String::new();
    for data in op
        .child("MailboxDataArray")
        .map(|a| a.children.as_slice())
        .unwrap_or_default()
    {
        let address = data
            .path(&["Email", "Address"])
            .map(|a| a.text.trim().to_ascii_lowercase())
            .unwrap_or_default();
        if !known_address(state, &address) {
            responses.push_str(r#"<FreeBusyResponse><ResponseMessage ResponseClass="Error"><MessageText>No mailbox with such guid.</MessageText><ResponseCode>ErrorMailRecipientNotFound</ResponseCode></ResponseMessage><FreeBusyView><FreeBusyViewType xmlns="http://schemas.microsoft.com/exchange/services/2006/types">None</FreeBusyViewType></FreeBusyView></FreeBusyResponse>"#);
            continue;
        }
        if let Some(m) = state.mailbox_index(Some(&address))
            && state.mailboxes[m].access == Access::Denied
            && address != state.own
        {
            responses.push_str(r#"<FreeBusyResponse><ResponseMessage ResponseClass="Error"><MessageText>Access denied.</MessageText><ResponseCode>ErrorNoFreeBusyAccess</ResponseCode></ResponseMessage><FreeBusyView/></FreeBusyResponse>"#);
            continue;
        }
        let mut periods: Vec<(String, String, String)> = state
            .free_busy
            .get(&address)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter(|(s, e, _)| s.as_str() < to.as_str() && e.as_str() > from.as_str())
            .collect();
        if let Some(m) = state.mailbox_index(Some(&address))
            && let Some(calendar) = default_folder(state, m, "calendar")
        {
            for item in state.mailboxes[m]
                .items
                .iter()
                .filter(|i| i.folder == calendar)
            {
                periods.extend(busy_periods(item, &from, &to));
            }
        }
        periods.sort();
        let events: String = periods
            .iter()
            .map(|(s, e, k)| {
                format!(
                    "<CalendarEvent>{}{}{}</CalendarEvent>",
                    el("StartTime", s.trim_end_matches('Z')),
                    el("EndTime", e.trim_end_matches('Z')),
                    el("BusyType", k)
                )
            })
            .collect();
        responses.push_str(&format!(
            r#"<FreeBusyResponse><ResponseMessage ResponseClass="Success"><ResponseCode>NoError</ResponseCode></ResponseMessage><FreeBusyView><FreeBusyViewType xmlns="{TYPES_NS}">FreeBusy</FreeBusyViewType><CalendarEventArray xmlns="{TYPES_NS}">{events}</CalendarEventArray></FreeBusyView></FreeBusyResponse>"#
        ));
    }
    Ok(format!(
        r#"<GetUserAvailabilityResponse xmlns="{MESSAGES_NS}"><FreeBusyResponseArray>{responses}</FreeBusyResponseArray></GetUserAvailabilityResponse>"#
    ))
}

fn resolve_names(state: &State, op: &Element) -> OpResult {
    const OP: &str = "ResolveNames";
    let entry = op
        .child_text("UnresolvedEntry")
        .unwrap_or_default()
        .to_ascii_lowercase();
    let mut people: Vec<(String, String)> = state
        .mailboxes
        .iter()
        .map(|m| (m.address.clone(), m.name.clone()))
        .chain(state.directory.iter().cloned())
        .chain(
            state
                .room_lists
                .iter()
                .flat_map(|(_, _, rooms)| rooms.iter().cloned()),
        )
        .filter(|(a, n)| *a == entry || n.to_ascii_lowercase().contains(&entry))
        .collect();
    people.dedup();
    if people.is_empty() {
        return Ok(messages(
            OP,
            &[error_message(
                OP,
                "ErrorNameResolutionNoResults",
                "No results were found.",
            )],
        ));
    }
    let resolutions: String = people
        .iter()
        .map(|(address, name)| {
            format!(
                "<t:Resolution><t:Mailbox>{}{}<t:RoutingType>SMTP</t:RoutingType><t:MailboxType>Mailbox</t:MailboxType></t:Mailbox></t:Resolution>",
                el("t:Name", name),
                el("t:EmailAddress", address)
            )
        })
        .collect();
    let message = if people.len() > 1 {
        format!(
            r#"<m:{OP}ResponseMessage ResponseClass="Warning"><m:MessageText>Multiple results were found.</m:MessageText><m:ResponseCode>ErrorNameResolutionMultipleResults</m:ResponseCode><m:DescriptiveLinkKey>0</m:DescriptiveLinkKey><m:ResolutionSet TotalItemsInView="{n}" IncludesLastItemInRange="true">{resolutions}</m:ResolutionSet></m:{OP}ResponseMessage>"#,
            n = people.len()
        )
    } else {
        ok_message(
            OP,
            &format!(
                r#"<m:ResolutionSet TotalItemsInView="1" IncludesLastItemInRange="true">{resolutions}</m:ResolutionSet>"#
            ),
        )
    };
    Ok(messages(OP, &[message]))
}

fn get_delegate(state: &State) -> OpResult {
    let users: String = state
        .delegates
        .iter()
        .map(|(address, name, level)| {
            format!(
                r#"<m:DelegateUserResponseMessageType ResponseClass="Success"><m:ResponseCode>NoError</m:ResponseCode><m:DelegateUser><t:UserId>{}{}</t:UserId><t:DelegatePermissions><t:CalendarFolderPermissionLevel>{}</t:CalendarFolderPermissionLevel><t:InboxFolderPermissionLevel>None</t:InboxFolderPermissionLevel></t:DelegatePermissions><t:ReceiveCopiesOfMeetingMessages>true</t:ReceiveCopiesOfMeetingMessages><t:ViewPrivateItems>false</t:ViewPrivateItems></m:DelegateUser></m:DelegateUserResponseMessageType>"#,
                el("t:PrimarySmtpAddress", address),
                el("t:DisplayName", name),
                escape(level)
            )
        })
        .collect();
    Ok(format!(
        r#"<m:GetDelegateResponse ResponseClass="Success"><m:ResponseCode>NoError</m:ResponseCode><m:ResponseMessages>{users}</m:ResponseMessages><m:DeliverMeetingRequests>DelegatesAndMe</m:DeliverMeetingRequests></m:GetDelegateResponse>"#
    ))
}
