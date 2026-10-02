//! The mock's side of large messages (M4c): drafts created from JSON,
//! `fileAttachment` posts, attachment upload sessions with byte ranges on
//! a pre-authenticated URL (`/upload/{session}`, outside `/v1.0`, refusing
//! bearer tokens like Outlook's upload service), sending a draft, and the
//! 4 MB request limit Graph enforces.

use base64::Engine as _;
use serde_json::{Value, json};

use super::server::{Reply, Req, State};
use super::tenant::{
    CompletedUpload, JsonDraft, MockError, MockResult, MockUpload, SentMail, Tenant,
};

/// Graph's request size limit.
pub(crate) const MAX_REQUEST: usize = 4 * 1024 * 1024;

/// Upload ranges must be multiples of this (except the last).
pub(crate) const UPLOAD_UNIT: usize = 320 * 1024;

/// `PidTagMessageFlags`.
const MESSAGE_FLAGS_PROP: &str = "Integer 0x0E07";

/// The answer to a request over 4 MB.
pub(crate) fn too_large() -> MockError {
    MockError::new(
        413,
        "RequestBodyTooLarge",
        "The maximum request length supported is 4MB.",
    )
}

/// `raw` without its `Bcc` header (the Sent Items copy).
pub(crate) fn without_bcc(raw: &[u8]) -> Vec<u8> {
    let text = String::from_utf8_lossy(raw);
    let (head, body) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
    let mut kept = Vec::new();
    let mut skipping = false;
    for line in head.split("\r\n") {
        if line.starts_with([' ', '\t']) {
            if !skipping {
                kept.push(line);
            }
            continue;
        }
        skipping = line.to_ascii_lowercase().starts_with("bcc:");
        if !skipping {
            kept.push(line);
        }
    }
    format!("{}\r\n\r\n{body}", kept.join("\r\n")).into_bytes()
}

fn address_list(value: &Value) -> Option<String> {
    let list: Vec<String> = match value {
        Value::Array(items) => items.iter().filter_map(one_address).collect(),
        Value::Object(_) => one_address(value).into_iter().collect(),
        _ => Vec::new(),
    };
    (!list.is_empty()).then(|| list.join(", "))
}

fn one_address(value: &Value) -> Option<String> {
    let address = value.pointer("/emailAddress/address")?.as_str()?;
    Some(
        match value.pointer("/emailAddress/name").and_then(Value::as_str) {
            Some(name) if !name.is_empty() => format!("\"{name}\" <{address}>"),
            _ => format!("<{address}>"),
        },
    )
}

/// The header fields of `raw` as Graph's `internetMessageHeaders`
/// (original case, unfolded).
pub(crate) fn transport_headers(raw: &[u8]) -> Vec<Value> {
    let text = String::from_utf8_lossy(raw);
    let head = text
        .split("\r\n\r\n")
        .next()
        .unwrap_or_default()
        .replace("\r\n ", " ")
        .replace("\r\n\t", " ");
    head.split("\r\n")
        .filter_map(|line| line.split_once(':'))
        .map(|(n, v)| json!({ "name": n.trim(), "value": v.trim() }))
        .collect()
}

/// Renders a JSON draft with its attachments as MIME (what the mock stores
/// and what a sent draft "delivers").
pub(crate) fn render(draft: &JsonDraft, id: &str) -> Vec<u8> {
    let m = &draft.message;
    let mut out = String::new();
    for (header, field) in [
        ("From", "from"),
        ("Sender", "sender"),
        ("To", "toRecipients"),
        ("Cc", "ccRecipients"),
        ("Bcc", "bccRecipients"),
        ("Reply-To", "replyTo"),
    ] {
        if let Some(list) = address_list(&m[field]) {
            out.push_str(&format!("{header}: {list}\r\n"));
        }
    }
    out.push_str(&format!(
        "Subject: {}\r\n",
        m["subject"].as_str().unwrap_or_default()
    ));
    out.push_str(&format!(
        "Message-ID: {}\r\n",
        m["internetMessageId"]
            .as_str()
            .map_or_else(|| format!("<{id}@mock.graph.example>"), str::to_owned)
    ));
    out.push_str("Date: Thu, 01 Oct 2026 08:00:00 +0000\r\n");
    for prop in m["singleValueExtendedProperties"]
        .as_array()
        .cloned()
        .unwrap_or_default()
    {
        let value = prop["value"].as_str().unwrap_or_default();
        match prop["id"].as_str() {
            Some("String 0x1042") => out.push_str(&format!("In-Reply-To: {value}\r\n")),
            Some("String 0x1039") => out.push_str(&format!("References: {value}\r\n")),
            _ => {}
        }
    }
    for h in m["internetMessageHeaders"]
        .as_array()
        .cloned()
        .unwrap_or_default()
    {
        out.push_str(&format!(
            "{}: {}\r\n",
            h["name"].as_str().unwrap_or_default(),
            h["value"].as_str().unwrap_or_default()
        ));
    }
    if m["importance"] == "high" {
        out.push_str("Importance: high\r\n");
    }
    out.push_str("MIME-Version: 1.0\r\n");
    let body_type = if m["body"]["contentType"] == "html" {
        "text/html"
    } else {
        "text/plain"
    };
    let body = m["body"]["content"].as_str().unwrap_or_default();
    if draft.attachments.is_empty() {
        out.push_str(&format!(
            "Content-Type: {body_type}; charset=utf-8\r\n\r\n{body}\r\n"
        ));
        return out.into_bytes();
    }
    let boundary = "mock-graph-draft";
    out.push_str(&format!(
        "Content-Type: multipart/mixed; boundary=\"{boundary}\"\r\n\r\n--{boundary}\r\nContent-Type: {body_type}; charset=utf-8\r\n\r\n{body}\r\n"
    ));
    for (meta, bytes) in &draft.attachments {
        let name = meta["name"].as_str().unwrap_or("attachment");
        let content_type = meta["contentType"]
            .as_str()
            .unwrap_or("application/octet-stream");
        let disposition = if meta["isInline"] == true {
            "inline"
        } else {
            "attachment"
        };
        out.push_str(&format!(
            "--{boundary}\r\nContent-Type: {content_type}; name=\"{name}\"\r\nContent-Disposition: {disposition}; filename=\"{name}\"\r\n"
        ));
        if let Some(cid) = meta["contentId"].as_str() {
            out.push_str(&format!("Content-ID: <{cid}>\r\n"));
        }
        out.push_str("Content-Transfer-Encoding: base64\r\n\r\n");
        let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
        for line in encoded.as_bytes().chunks(76) {
            out.push_str(std::str::from_utf8(line).unwrap_or_default());
            out.push_str("\r\n");
        }
    }
    out.push_str(&format!("--{boundary}--\r\n"));
    out.into_bytes()
}

/// Re-renders a JSON draft into its stored message.
fn rerender(tenant: &mut Tenant, id: &str) -> MockResult<()> {
    let draft = tenant
        .json_drafts
        .get(id)
        .cloned()
        .ok_or_else(|| MockError::not_found(id))?;
    let raw = render(&draft, id);
    let mailbox = tenant.mailbox(&draft.mailbox)?;
    mailbox.change_message(id, |m| m.raw = raw)
}

/// `POST messages` / `POST mailFolders/{id}/messages` with a JSON body.
pub(crate) fn create_json(
    tenant: &mut Tenant,
    address: &str,
    folder: &str,
    req: &Req,
) -> MockResult<Reply> {
    let message = req.json()?;
    if !message.is_object() {
        return Err(MockError::bad(
            "ErrorInvalidRequest",
            "A message object is required.",
        ));
    }
    let flags = message["singleValueExtendedProperties"]
        .as_array()
        .and_then(|props| {
            props
                .iter()
                .find(|p| p["id"] == MESSAGE_FLAGS_PROP)
                .and_then(|p| p["value"].as_str())
                .and_then(|v| v.parse::<u32>().ok())
        });
    let draft = JsonDraft {
        mailbox: address.to_ascii_lowercase(),
        message,
        attachments: Vec::new(),
    };
    let raw = render(&draft, "pending");
    let ids = &mut tenant.ids;
    let mailbox = tenant
        .mailboxes
        .get_mut(&address.to_ascii_lowercase())
        .ok_or_else(|| MockError::not_found(address))?;
    // Without the unsent bit (8) the message is not a draft.
    let is_draft = flags.is_none_or(|f| f & 8 != 0);
    let is_read = flags.is_some_and(|f| f & 1 != 0);
    let id = mailbox.add_message(ids, folder, raw, is_read, is_draft)?;
    let reply = Reply::json(201, &mailbox.messages[&id].to_json());
    tenant.json_drafts.insert(id.clone(), draft);
    rerender(tenant, &id)?;
    Ok(reply)
}

/// `POST messages/{id}/attachments` (a `fileAttachment`).
pub(crate) fn add_file_attachment(tenant: &mut Tenant, id: &str, req: &Req) -> MockResult<Reply> {
    let body = req.json()?;
    if body["@odata.type"] != "#microsoft.graph.fileAttachment" {
        return Err(MockError::bad(
            "ErrorInvalidRequest",
            "This mock takes file attachments only.",
        ));
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(body["contentBytes"].as_str().unwrap_or_default())
        .map_err(|_| MockError::bad("ErrorInvalidRequest", "Invalid contentBytes."))?;
    let draft = tenant.json_drafts.get_mut(id).ok_or_else(|| {
        MockError::bad(
            "ErrorInvalidOperation",
            "Attachments can only be added to drafts created from JSON in this mock.",
        )
    })?;
    let index = draft.attachments.len() + 1;
    let meta = json!({
        "@odata.type": "#microsoft.graph.fileAttachment",
        "id": format!("att{index}"),
        "name": body["name"],
        "contentType": body["contentType"],
        "size": bytes.len(),
        "isInline": body["isInline"].as_bool().unwrap_or(false),
        "contentId": body["contentId"],
    });
    draft.attachments.push((meta.clone(), bytes));
    rerender(tenant, id)?;
    Ok(Reply::json(201, &meta))
}

/// `POST messages/{id}/attachments/createUploadSession`.
pub(crate) fn create_upload_session(
    tenant: &mut Tenant,
    address: &str,
    id: &str,
    req: &Req,
    origin: &str,
) -> MockResult<Reply> {
    let body = req.json()?;
    let item = body["AttachmentItem"].clone();
    if item["attachmentType"] != "file" {
        return Err(MockError::bad(
            "ErrorInvalidRequest",
            "AttachmentItem.attachmentType must be file.",
        ));
    }
    let size = item["size"].as_u64().unwrap_or(0) as usize;
    if size == 0 || size > 150 * 1024 * 1024 {
        return Err(MockError::bad(
            "ErrorAttachmentSizeLimitExceeded",
            "Invalid size.",
        ));
    }
    if !tenant.json_drafts.contains_key(id) {
        return Err(MockError::not_found(id));
    }
    let session = tenant.ids.make("upl");
    let session = session.trim_end_matches('=').replace(['-', '_'], "");
    tenant.uploads.insert(
        session.clone(),
        MockUpload {
            mailbox: address.to_ascii_lowercase(),
            message_id: id.to_owned(),
            item,
            size,
            received: Vec::new(),
            ranges: Vec::new(),
        },
    );
    Ok(Reply::json(
        201,
        &json!({
            "@odata.context": "https://graph.microsoft.com/v1.0/$metadata#microsoft.graph.uploadSession",
            "uploadUrl": format!("{origin}/upload/{session}?authtoken=mock-{session}"),
            "expirationDateTime": "2026-10-02T12:00:00Z",
            "nextExpectedRanges": ["0-"],
        }),
    ))
}

fn parse_range(value: &str) -> Option<(u64, u64, u64)> {
    let rest = value.trim().strip_prefix("bytes ")?;
    let (range, total) = rest.split_once('/')?;
    let (start, end) = range.split_once('-')?;
    Some((
        start.trim().parse().ok()?,
        end.trim().parse().ok()?,
        total.trim().parse().ok()?,
    ))
}

/// `PUT /upload/{session}`: one byte range (no bearer token allowed).
pub(crate) fn put_range(state: &mut State, req: &Req) -> Reply {
    match put_range_inner(state, req) {
        Ok(reply) => reply,
        Err(error) => Reply::json(error.status, &error.body),
    }
}

fn put_range_inner(state: &mut State, req: &Req) -> MockResult<Reply> {
    if req.method != "PUT" {
        return Err(MockError::new(405, "MethodNotAllowed", "Use PUT."));
    }
    if req.header("authorization").is_some() {
        return Err(MockError::new(
            401,
            "InvalidAuthenticationToken",
            "An Authorization header is not allowed on the upload URL.",
        ));
    }
    let session_id = req.segments.get(1).cloned().unwrap_or_default();
    let tenant = &mut state.tenant;
    let upload = tenant
        .uploads
        .get_mut(&session_id)
        .ok_or_else(|| MockError::not_found("upload session"))?;
    let (start, end, total) = req
        .header("content-range")
        .and_then(parse_range)
        .ok_or_else(|| MockError::bad("InvalidContentRange", "A Content-Range is required."))?;
    let len = req.body.len() as u64;
    if total as usize != upload.size || end < start || end - start + 1 != len || end >= total {
        return Err(MockError::bad(
            "InvalidContentRange",
            "The Content-Range does not match the body or the session.",
        ));
    }
    let have = upload.received.len() as u64;
    if start > have {
        return Err(MockError::new(416, "InvalidRange", "A range is missing."));
    }
    let last = end + 1 == total;
    if !last && !(len as usize).is_multiple_of(UPLOAD_UNIT) {
        return Err(MockError::bad(
            "InvalidChunkSize",
            "Ranges must be multiples of 320 KiB.",
        ));
    }
    upload.ranges.push((start, end));
    if end + 1 > have {
        let skip = (have - start) as usize;
        upload.received.extend_from_slice(&req.body[skip..]);
    }
    if (upload.received.len() as u64) < total {
        return Ok(Reply::json(
            200,
            &json!({
                "expirationDateTime": "2026-10-02T12:00:00Z",
                "nextExpectedRanges": [format!("{}-", upload.received.len())],
            }),
        ));
    }
    let upload = tenant
        .uploads
        .remove(&session_id)
        .ok_or_else(|| MockError::not_found("upload session"))?;
    let message_id = upload.message_id.clone();
    let draft = tenant
        .json_drafts
        .get_mut(&message_id)
        .ok_or_else(|| MockError::not_found(&message_id))?;
    let index = draft.attachments.len() + 1;
    let meta = json!({
        "@odata.type": "#microsoft.graph.fileAttachment",
        "id": format!("att{index}"),
        "name": upload.item["name"],
        "contentType": upload.item["contentType"],
        "size": upload.size,
        "isInline": upload.item["isInline"].as_bool().unwrap_or(false),
        "contentId": upload.item["contentId"],
    });
    draft.attachments.push((meta.clone(), upload.received));
    tenant.completed_uploads.push(CompletedUpload {
        message_id: message_id.clone(),
        name: upload.item["name"].as_str().unwrap_or_default().to_owned(),
        size: upload.size,
        ranges: upload.ranges.len(),
    });
    rerender(tenant, &message_id)?;
    Ok(Reply::json(201, &meta))
}

/// `POST messages/{id}/send`: the draft goes out and is filed in Sent Items.
pub(crate) fn send_draft(tenant: &mut Tenant, address: &str, id: &str) -> MockResult<Reply> {
    let address = address.to_ascii_lowercase();
    let draft = tenant
        .json_drafts
        .remove(id)
        .ok_or_else(|| MockError::not_found(id))?;
    if draft.mailbox != address {
        return Err(MockError::not_found(id));
    }
    if address_list(&draft.message["toRecipients"]).is_none()
        && address_list(&draft.message["ccRecipients"]).is_none()
        && address_list(&draft.message["bccRecipients"]).is_none()
    {
        return Err(MockError::bad(
            "ErrorInvalidRecipients",
            "At least one recipient isn't valid.",
        ));
    }
    let raw = render(&draft, id);
    let ids = &mut tenant.ids;
    let mailbox = tenant
        .mailboxes
        .get_mut(&address)
        .ok_or_else(|| MockError::not_found(&address))?;
    mailbox.remove_message(id)?;
    mailbox.add_message(ids, "sentitems", without_bcc(&raw), true, false)?;
    tenant.sent.push(SentMail {
        mailbox: address,
        raw,
        via: "send".into(),
    });
    Ok(Reply::empty(202))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_ranges_parse() {
        assert_eq!(parse_range("bytes 0-99/1000"), Some((0, 99, 1000)));
        assert_eq!(parse_range("bytes x-1/2"), None);
        assert_eq!(parse_range("0-1/2"), None);
    }

    #[test]
    fn drafts_render_as_mime() {
        let draft = JsonDraft {
            mailbox: "ada@contoso.example".into(),
            message: json!({
                "subject": "Hi",
                "from": {"emailAddress": {"address": "team@contoso.example", "name": "Team"}},
                "toRecipients": [{"emailAddress": {"address": "bob@x.example"}}],
                "body": {"contentType": "html", "content": "<p>x</p>"},
                "singleValueExtendedProperties": [{"id": "String 0x1042", "value": "<p@x>"}],
            }),
            attachments: vec![(
                json!({"name": "a.bin", "contentType": "application/octet-stream"}),
                vec![1, 2, 3],
            )],
        };
        let raw = String::from_utf8(render(&draft, "m1")).unwrap();
        assert!(raw.contains("From: \"Team\" <team@contoso.example>\r\n"));
        assert!(raw.contains("To: <bob@x.example>\r\n"));
        assert!(raw.contains("In-Reply-To: <p@x>\r\n"));
        assert!(raw.contains("Message-ID: <m1@mock.graph.example>\r\n"));
        assert!(raw.contains("filename=\"a.bin\"") && raw.contains("AQID"));
        assert_eq!(
            without_bcc(b"Bcc: x@y\r\n z@y\r\nTo: a@b\r\n\r\nbody"),
            b"To: a@b\r\n\r\nbody"
        );
    }
}
