//! The HTTP side of the mock: authentication, injected errors, a quota in
//! Gmail's units, the REST routes, media uploads and batch requests.

use std::fmt::Write as _;

use nail_testkit_net::api_mock::MockReply;
use nail_util::percent::decode as percent_decode;
use serde_json::{Value, json};

use super::mailbox::{Mailbox, MockError, MockResult};
use super::{InjectedError, RecordedRequest};
use crate::batch::boundary_of;
use crate::wire::{ErrorBody, Label, decode_base64url};

/// A quota in Gmail's units per user (`messages.get` 5, `send` 100, …).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Quota {
    pub limit: u64,
    pub used: u64,
    pub retry_after: u32,
}

/// The mock's mutable state.
#[derive(Debug)]
pub(crate) struct State {
    pub mailbox: Mailbox,
    pub token: Option<String>,
    pub errors: Vec<InjectedError>,
    pub requests: Vec<RecordedRequest>,
    pub quota: Option<Quota>,
}

/// An answer.
pub(crate) type Reply = MockReply;

/// Gmail's answers.
trait Replies {
    fn json(status: u16, value: &Value) -> Self;
    fn empty() -> Self;
    fn error(error: &MockError) -> Self;
}

impl Replies for Reply {
    fn json(status: u16, value: &Value) -> Self {
        Self::json_as(status, value, "application/json; charset=UTF-8")
    }

    fn empty() -> Self {
        Self::with_body(204, None, Vec::new())
    }

    fn error(error: &MockError) -> Self {
        Self::json(
            error.status,
            &serde_json::to_value(&error.body).unwrap_or(Value::Null),
        )
    }
}

fn status_text(status: u16) -> &'static str {
    match status {
        200 => "OK",
        204 => "No Content",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        409 => "Conflict",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        503 => "Service Unavailable",
        _ => "Status",
    }
}

struct Query(Vec<(String, String)>);

impl Query {
    fn parse(text: &str) -> Self {
        Self(
            url::form_urlencoded::parse(text.as_bytes())
                .map(|(k, v)| (k.into_owned(), v.into_owned()))
                .collect(),
        )
    }

    fn get(&self, name: &str) -> Option<&str> {
        self.0
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }

    fn all(&self, name: &str) -> Vec<String> {
        self.0
            .iter()
            .filter(|(k, _)| k == name)
            .map(|(_, v)| v.clone())
            .collect()
    }

    fn flag(&self, name: &str) -> bool {
        self.get(name).is_some_and(|v| v == "true")
    }

    fn page(&self) -> MockResult<(usize, usize)> {
        let max = match self.get("maxResults") {
            Some(v) => v
                .parse::<usize>()
                .map_err(|_| MockError::bad_request("Invalid maxResults"))?
                .clamp(1, 500),
            None => 100,
        };
        let offset = match self.get("pageToken") {
            Some(v) => v
                .parse::<usize>()
                .map_err(|_| MockError::bad_request("Invalid pageToken"))?,
            None => 0,
        };
        Ok((offset, max))
    }
}

use nail_util::named::first as header;

/// The ids of a thread's messages (`404` for an unknown thread).
fn thread_ids(mb: &Mailbox, thread: &str) -> Result<Vec<String>, MockError> {
    let ids: Vec<String> = mb
        .thread_messages(thread)
        .iter()
        .map(|m| m.id.clone())
        .collect();
    if ids.is_empty() {
        return Err(MockError::not_found());
    }
    Ok(ids)
}

/// Gmail's quota units of a call.
fn cost(method: &str, segments: &[&str]) -> u64 {
    match (method, segments) {
        (_, ["messages", "send"] | ["drafts", ..]) => 100,
        (_, ["messages", "batchModify" | "batchDelete"]) => 50,
        ("GET", ["history"]) => 2,
        (_, ["labels", ..]) | ("GET", ["profile"]) => 1,
        ("DELETE", ["messages", _]) | ("GET", ["threads", _]) => 10,
        _ => 5,
    }
}

/// Splits a multipart body into (part headers, part content).
fn split_multipart<'a>(body: &'a [u8], boundary: &str) -> Vec<(String, &'a [u8])> {
    let delimiter = format!("--{boundary}");
    let needle = delimiter.as_bytes();
    let mut starts = Vec::new();
    let mut i = 0;
    while i + needle.len() <= body.len() {
        if &body[i..i + needle.len()] == needle {
            starts.push(i);
            i += needle.len();
        } else {
            i += 1;
        }
    }
    let mut out = Vec::new();
    for pair in starts.windows(2) {
        let mut part = &body[pair[0] + needle.len()..pair[1]];
        if part.starts_with(b"--") {
            break;
        }
        while part.first().is_some_and(|b| *b == b'\r' || *b == b'\n') {
            part = &part[1..];
        }
        // Content up to the CRLF before the next delimiter.
        if part.ends_with(b"\r\n") {
            part = &part[..part.len() - 2];
        } else if part.ends_with(b"\n") {
            part = &part[..part.len() - 1];
        }
        let (head, content) = match find(part, b"\r\n\r\n") {
            Some(at) => (&part[..at], &part[at + 4..]),
            None => match find(part, b"\n\n") {
                Some(at) => (&part[..at], &part[at + 2..]),
                None => (part, &part[part.len()..]),
            },
        };
        out.push((String::from_utf8_lossy(head).into_owned(), content));
    }
    out
}

use nail_util::mime_text::find;
use nail_util::named::in_lines as part_header;

/// The message resource of an insert / send: JSON `raw` or a media upload.
fn upload_message(
    upload: bool,
    query: &Query,
    headers: &[(String, String)],
    body: &[u8],
) -> MockResult<(Vec<u8>, Value)> {
    if !upload {
        let value: Value = serde_json::from_slice(body)
            .map_err(|_| MockError::bad_request("Invalid JSON payload received."))?;
        let raw = value
            .get("raw")
            .and_then(Value::as_str)
            .map(decode_base64url)
            .transpose()
            .map_err(|_| MockError::bad_request("Invalid value for ByteString"))?
            .unwrap_or_default();
        return Ok((raw, value));
    }
    match query.get("uploadType") {
        Some("media") => Ok((body.to_vec(), json!({}))),
        Some("multipart") => {
            let boundary = header(headers, "content-type")
                .and_then(boundary_of)
                .ok_or_else(|| MockError::bad_request("Missing boundary"))?;
            let parts = split_multipart(body, &boundary);
            let [(_, metadata), (_, raw)] = parts.as_slice() else {
                return Err(MockError::bad_request("Expected metadata and media parts"));
            };
            let metadata: Value = serde_json::from_slice(metadata)
                .map_err(|_| MockError::bad_request("Invalid metadata"))?;
            Ok((raw.to_vec(), metadata))
        }
        _ => Err(MockError::bad_request("Invalid uploadType")),
    }
}

fn labels_of(value: &Value, key: &str) -> Vec<String> {
    value
        .get(key)
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

fn body_json(body: &[u8]) -> MockResult<Value> {
    if body.is_empty() {
        return Ok(json!({}));
    }
    serde_json::from_slice(body)
        .map_err(|_| MockError::bad_request("Invalid JSON payload received."))
}

fn short(m: &super::mailbox::MockMessage) -> Value {
    json!({ "id": m.id, "threadId": m.thread_id, "labelIds": m.label_ids })
}

/// Handles one request (also each part of a batch).
pub(crate) fn dispatch(
    state: &mut State,
    method: &str,
    path: &str,
    query_text: &str,
    headers: &[(String, String)],
    body: &[u8],
    in_batch: bool,
) -> Reply {
    let authorization = header(headers, "authorization").map(str::to_owned);
    let token = authorization
        .as_deref()
        .and_then(|a| a.strip_prefix("Bearer "))
        .map(str::to_owned);
    state.requests.push(RecordedRequest {
        method: method.to_owned(),
        path: path.to_owned(),
        query: query_text.to_owned(),
        in_batch,
        token: token.clone(),
    });
    let authorized = match (&token, &state.token) {
        (None, _) => false,
        (Some(t), Some(expected)) => t == expected,
        (Some(t), None) => !t.is_empty(),
    };
    if !authorized {
        let body: Value = serde_json::from_str(include_str!(
            "../../fixtures/responses/error-unauthenticated-401.json"
        ))
        .unwrap_or(Value::Null);
        let mut reply = Reply::json(401, &body);
        reply.headers.push((
            "www-authenticate".to_owned(),
            "Bearer realm=\"https://accounts.google.com/\"".to_owned(),
        ));
        return reply;
    }
    let target = format!("{path}?{query_text}");
    if let Some(error) = InjectedError::take_due(&mut state.errors, method, &target) {
        let mut reply = Reply::json(error.status, &error.body);
        if let Some(r) = error.retry_after {
            reply.headers.push(("retry-after".to_owned(), r));
        }
        return reply;
    }

    if path == "/batch/gmail/v1" || path == "/batch" {
        if in_batch {
            return Reply::error(&MockError::bad_request("Nested batch"));
        }
        return batch(state, headers, body);
    }
    let base = nail_api::gmail::GMAIL_API_BASE_PATH;
    let (upload, rest) = if let Some(rest) = path.strip_prefix(&format!("/upload{base}")) {
        (true, rest)
    } else if let Some(rest) = path.strip_prefix(base) {
        (false, rest)
    } else {
        return Reply::error(&MockError::not_found());
    };
    let decoded: Vec<String> = rest
        .split('/')
        .filter(|s| !s.is_empty())
        .map(percent_decode)
        .collect();
    let segments: Vec<&str> = decoded.iter().map(String::as_str).collect();
    if let Some(quota) = state.quota.as_mut() {
        let units = cost(method, &segments);
        if quota.used + units > quota.limit {
            let body: Value = serde_json::from_str(include_str!(
                "../../fixtures/responses/error-rate-limit-429.json"
            ))
            .unwrap_or(Value::Null);
            let mut reply = Reply::json(429, &body);
            reply
                .headers
                .push(("retry-after".to_owned(), quota.retry_after.to_string()));
            return reply;
        }
        quota.used += units;
    }
    let query = Query::parse(query_text);
    match route(
        &mut state.mailbox,
        method,
        &segments,
        &query,
        headers,
        body,
        upload,
    ) {
        Ok(reply) => reply,
        Err(e) => Reply::error(&e),
    }
}

fn route(
    mb: &mut Mailbox,
    method: &str,
    segments: &[&str],
    query: &Query,
    headers: &[(String, String)],
    body: &[u8],
    upload: bool,
) -> MockResult<Reply> {
    let ok = |v: Value| Ok(Reply::json(200, &v));
    match (method, segments) {
        ("GET", ["profile"]) => {
            let threads: std::collections::BTreeSet<String> = mb
                .message_ids()
                .iter()
                .filter_map(|id| mb.message(id).map(|m| m.thread_id.clone()))
                .collect();
            ok(json!({
                "emailAddress": mb.email,
                "messagesTotal": mb.message_ids().len(),
                "threadsTotal": threads.len(),
                "historyId": mb.history_id().to_string(),
            }))
        }
        ("GET", ["messages"]) => {
            let found = mb.search(
                query.get("q"),
                &query.all("labelIds"),
                query.flag("includeSpamTrash"),
            )?;
            let (offset, max) = query.page()?;
            let total = found.len();
            let page: Vec<Value> = found
                .iter()
                .skip(offset)
                .take(max)
                .map(|m| json!({"id": m.id, "threadId": m.thread_id}))
                .collect();
            let mut out = json!({ "resultSizeEstimate": total });
            if !page.is_empty() {
                out["messages"] = Value::Array(page);
            }
            if offset + max < total {
                out["nextPageToken"] = json!((offset + max).to_string());
            }
            ok(out)
        }
        ("POST", ["messages"] | ["messages", "import"]) => {
            let (raw, metadata) = upload_message(upload, query, headers, body)?;
            let id = mb.add_message(
                raw,
                &labels_of(&metadata, "labelIds"),
                metadata.get("threadId").and_then(Value::as_str),
            )?;
            ok(short(mb.message(&id).ok_or_else(MockError::not_found)?))
        }
        ("POST", ["messages", "send"]) => {
            let (raw, metadata) = upload_message(upload, query, headers, body)?;
            let id = mb.send(raw, metadata.get("threadId").and_then(Value::as_str))?;
            ok(short(mb.message(&id).ok_or_else(MockError::not_found)?))
        }
        ("POST", ["messages", "batchModify"]) => {
            let v = body_json(body)?;
            let ids = labels_of(&v, "ids");
            if ids.len() > 1000 {
                return Err(MockError::bad_request("Too many ids"));
            }
            for id in ids {
                if mb.message(&id).is_some() {
                    mb.modify(
                        &id,
                        &labels_of(&v, "addLabelIds"),
                        &labels_of(&v, "removeLabelIds"),
                    )?;
                }
            }
            Ok(Reply::empty())
        }
        ("POST", ["messages", "batchDelete"]) => {
            let v = body_json(body)?;
            for id in labels_of(&v, "ids") {
                if mb.message(&id).is_some() {
                    mb.delete(&id)?;
                }
            }
            Ok(Reply::empty())
        }
        ("GET", ["messages", id]) => {
            let m = mb.message(id).ok_or_else(MockError::not_found)?.clone();
            let format = query.get("format").unwrap_or("full");
            if !["minimal", "metadata", "full", "raw"].contains(&format) {
                return Err(MockError::bad_request("Invalid format"));
            }
            ok(mb.message_json(&m, format, &query.all("metadataHeaders")))
        }
        ("DELETE", ["messages", id]) => {
            mb.delete(id)?;
            Ok(Reply::empty())
        }
        ("POST", ["messages", id, "modify"]) => {
            let v = body_json(body)?;
            let m = mb.modify(
                id,
                &labels_of(&v, "addLabelIds"),
                &labels_of(&v, "removeLabelIds"),
            )?;
            ok(short(m))
        }
        ("POST", ["messages", id, "trash"]) => ok(short(mb.trash(id)?)),
        ("POST", ["messages", id, "untrash"]) => ok(short(mb.untrash(id)?)),
        ("GET", ["messages", id, "attachments", att]) => ok(mb.attachment_json(id, att)?),
        ("POST", ["drafts"]) => {
            let v = body_json(body)?;
            let message = v.get("message").cloned().unwrap_or_default();
            let raw = message
                .get("raw")
                .and_then(Value::as_str)
                .map(decode_base64url)
                .transpose()
                .map_err(|_| MockError::bad_request("Invalid value for ByteString"))?
                .unwrap_or_default();
            let (draft, id) =
                mb.create_draft(raw, message.get("threadId").and_then(Value::as_str))?;
            let m = mb.message(&id).ok_or_else(MockError::not_found)?;
            ok(json!({ "id": draft, "message": short(m) }))
        }
        ("GET", ["history"]) => {
            let start: u64 = query
                .get("startHistoryId")
                .ok_or_else(|| MockError::bad_request("startHistoryId is required"))?
                .parse()
                .map_err(|_| MockError::bad_request("Invalid startHistoryId"))?;
            let records =
                mb.history_after(start, &query.all("historyTypes"), query.get("labelId"))?;
            let (offset, max) = query.page()?;
            let page: Vec<_> = records.iter().skip(offset).take(max).cloned().collect();
            let mut out = json!({ "historyId": mb.history_id().to_string() });
            if !page.is_empty() {
                out["history"] = serde_json::to_value(page).unwrap_or_default();
            }
            if offset + max < records.len() {
                out["nextPageToken"] = json!((offset + max).to_string());
            }
            ok(out)
        }
        ("GET", ["labels"]) => ok(json!({ "labels": mb.labels() })),
        ("POST", ["labels"]) => {
            let label: Label = serde_json::from_slice(body)
                .map_err(|_| MockError::bad_request("Invalid JSON payload received."))?;
            ok(serde_json::to_value(mb.create_label(label)?).unwrap_or_default())
        }
        ("GET", ["labels", id]) => {
            ok(serde_json::to_value(mb.label_with_counts(id)?).unwrap_or_default())
        }
        ("PATCH" | "PUT", ["labels", id]) => {
            let v = body_json(body)?;
            ok(serde_json::to_value(mb.update_label(id, &v, method == "PUT")?).unwrap_or_default())
        }
        ("DELETE", ["labels", id]) => {
            mb.delete_label(id)?;
            Ok(Reply::empty())
        }
        ("GET", ["threads"]) => {
            let found = mb.search(
                query.get("q"),
                &query.all("labelIds"),
                query.flag("includeSpamTrash"),
            )?;
            let threads = mb.thread_ids(&found);
            let (offset, max) = query.page()?;
            let page: Vec<Value> = threads
                .iter()
                .skip(offset)
                .take(max)
                .map(|t| {
                    let messages = mb.thread_messages(t);
                    let last = messages.last();
                    json!({
                        "id": t,
                        "snippet": last.map(|m| mb.message_json(m, "minimal", &[])["snippet"].clone()).unwrap_or_default(),
                        "historyId": last.map(|m| m.history_id.to_string()).unwrap_or_default(),
                    })
                })
                .collect();
            let mut out = json!({ "resultSizeEstimate": threads.len() });
            if !page.is_empty() {
                out["threads"] = Value::Array(page);
            }
            if offset + max < threads.len() {
                out["nextPageToken"] = json!((offset + max).to_string());
            }
            ok(out)
        }
        ("GET", ["threads", id]) => {
            let format = query.get("format").unwrap_or("full");
            let messages = mb.thread_messages(id);
            if messages.is_empty() {
                return Err(MockError::not_found());
            }
            let history = messages
                .iter()
                .map(|m| m.history_id)
                .max()
                .unwrap_or_default();
            let list: Vec<Value> = messages
                .iter()
                .map(|m| mb.message_json(m, format, &query.all("metadataHeaders")))
                .collect();
            ok(json!({ "id": id, "historyId": history.to_string(), "messages": list }))
        }
        ("POST", ["threads", id, action @ ("modify" | "trash" | "untrash")]) => {
            let ids = thread_ids(mb, id)?;
            let v = body_json(body)?;
            for m in &ids {
                match *action {
                    "trash" => {
                        mb.trash(m)?;
                    }
                    "untrash" => {
                        mb.untrash(m)?;
                    }
                    _ => {
                        mb.modify(
                            m,
                            &labels_of(&v, "addLabelIds"),
                            &labels_of(&v, "removeLabelIds"),
                        )?;
                    }
                }
            }
            ok(json!({ "id": id }))
        }
        ("DELETE", ["threads", id]) => {
            let ids = thread_ids(mb, id)?;
            for m in ids {
                mb.delete(&m)?;
            }
            Ok(Reply::empty())
        }
        _ => Err(MockError::not_found()),
    }
}

/// A batch request: each `application/http` part dispatched on its own.
fn batch(state: &mut State, headers: &[(String, String)], body: &[u8]) -> Reply {
    let Some(boundary) = header(headers, "content-type").and_then(boundary_of) else {
        return Reply::error(&MockError::bad_request("Missing batch boundary"));
    };
    let parts = split_multipart(body, &boundary);
    if parts.len() > crate::batch::MAX_BATCH {
        return Reply::error(&MockError::bad_request(
            "A batch request cannot contain more than 100 requests",
        ));
    }
    let outer_auth = header(headers, "authorization").map(str::to_owned);
    let response_boundary = "batch_mock_0123456789";
    let mut out = String::new();
    for (head, content) in parts {
        let content_id = part_header(&head, "content-id").map(|id| {
            let id = id.trim_matches(['<', '>']);
            format!("<response-{id}>")
        });
        let text = String::from_utf8_lossy(content).into_owned();
        let (request_head, request_body) = match text.find("\r\n\r\n") {
            Some(at) => (text[..at].to_owned(), text[at + 4..].to_owned()),
            None => (text.trim_end().to_owned(), String::new()),
        };
        let mut lines = request_head.lines();
        let request_line = lines.next().unwrap_or_default().to_owned();
        let mut inner_headers: Vec<(String, String)> = lines
            .filter_map(|l| l.split_once(':'))
            .map(|(n, v)| (n.trim().to_owned(), v.trim().to_owned()))
            .collect();
        if header(&inner_headers, "authorization").is_none()
            && let Some(auth) = &outer_auth
        {
            inner_headers.push(("authorization".to_owned(), auth.clone()));
        }
        let mut words = request_line.split_whitespace();
        let (Some(method), Some(target)) = (words.next(), words.next()) else {
            continue;
        };
        let (path, query) = target.split_once('?').unwrap_or((target, ""));
        let reply = dispatch(
            state,
            method,
            path,
            query,
            &inner_headers,
            request_body.as_bytes(),
            true,
        );
        let _ = write!(
            out,
            "--{response_boundary}\r\nContent-Type: application/http\r\n"
        );
        if let Some(id) = content_id {
            let _ = write!(out, "Content-ID: {id}\r\n");
        }
        let _ = write!(
            out,
            "\r\nHTTP/1.1 {} {}\r\n",
            reply.status,
            status_text(reply.status)
        );
        for (n, v) in &reply.headers {
            let _ = write!(out, "{n}: {v}\r\n");
        }
        out.push_str("\r\n");
        out.push_str(&String::from_utf8_lossy(&reply.body));
        out.push_str("\r\n");
    }
    let _ = write!(out, "--{response_boundary}--\r\n");
    Reply {
        status: 200,
        headers: vec![(
            "content-type".to_owned(),
            format!("multipart/mixed; boundary={response_boundary}"),
        )],
        body: out.into_bytes(),
    }
}

/// The JSON body of an injected Gmail error (recorded answers).
pub(crate) fn recorded_error(name: &str) -> Value {
    let text = match name {
        "429" => include_str!("../../fixtures/responses/error-rate-limit-429.json"),
        "403-rate" => include_str!("../../fixtures/responses/error-user-rate-403.json"),
        "500" => include_str!("../../fixtures/responses/error-backend-500.json"),
        "401" => include_str!("../../fixtures/responses/error-unauthenticated-401.json"),
        "403-scope" => include_str!("../../fixtures/responses/error-insufficient-scope-403.json"),
        "404" => include_str!("../../fixtures/responses/error-history-404.json"),
        _ => "null",
    };
    serde_json::from_str(text).unwrap_or(Value::Null)
}

/// An error body for an arbitrary status.
pub(crate) fn generic_error(status: u16) -> Value {
    serde_json::to_value(ErrorBody::new(
        status,
        "UNKNOWN",
        "unknown",
        status_text(status),
    ))
    .unwrap_or(Value::Null)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multipart_split_and_decode() {
        let body = b"--b\r\nContent-Type: application/json\r\n\r\n{\"threadId\":\"t\"}\r\n--b\r\nContent-Type: message/rfc822\r\n\r\nSubject: x\r\n\r\nbody-b-ish\r\n--b--\r\n";
        let parts = split_multipart(body, "b");
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].1, b"{\"threadId\":\"t\"}");
        assert_eq!(parts[1].1, b"Subject: x\r\n\r\nbody-b-ish");
        assert_eq!(percent_decode("a%2Fb%zz"), "a/b%zz");
        assert_eq!(cost("POST", &["messages", "send"]), 100);
        assert_eq!(cost("GET", &["messages", "x"]), 5);
        assert_eq!(recorded_error("429")["error"]["code"], 429);
        assert_eq!(generic_error(503)["error"]["code"], 503);
    }
}
