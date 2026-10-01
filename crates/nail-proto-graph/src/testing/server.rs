//! The HTTP side of the mock: sign-in, injected errors, a request budget
//! (throttling), the REST routes, delta paging and `$batch`.

use std::collections::BTreeSet;

use base64::Engine as _;
use serde_json::{Value, json};

use super::tenant::{Mailbox, MeetingAction, MockError, MockResult, SentMail, Tenant, merge};
use super::{InjectedError, RecordedRequest};
use crate::mime;

/// A request budget: beyond `limit` requests the server answers `429`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Budget {
    pub limit: u64,
    pub used: u64,
    pub retry_after: u32,
}

/// The mock's mutable state.
#[derive(Debug)]
pub(crate) struct State {
    pub tenant: Tenant,
    pub token: Option<String>,
    pub errors: Vec<InjectedError>,
    pub requests: Vec<RecordedRequest>,
    pub budget: Option<Budget>,
    pub origin: String,
}

/// A request as the router sees it.
#[derive(Debug, Clone)]
pub(crate) struct Req {
    pub method: String,
    /// The path as sent (`/v1.0/me/…`), percent-encoded.
    pub path: String,
    /// The decoded segments after `/v1.0`.
    pub segments: Vec<String>,
    pub query: Vec<(String, String)>,
    pub raw_query: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Req {
    pub fn new(
        method: &str,
        path: &str,
        query: &str,
        headers: Vec<(String, String)>,
        body: Vec<u8>,
    ) -> Self {
        let rest = path
            .strip_prefix(nail_api::exchange::GRAPH_BASE_PATH)
            .unwrap_or(path);
        let segments = rest
            .split('/')
            .filter(|s| !s.is_empty())
            .map(percent_decode)
            .collect();
        Self {
            method: method.to_ascii_uppercase(),
            path: path.to_owned(),
            segments,
            query: url::form_urlencoded::parse(query.as_bytes())
                .map(|(k, v)| (k.into_owned(), v.into_owned()))
                .collect(),
            raw_query: query.to_owned(),
            headers: headers
                .into_iter()
                .map(|(k, v)| (k.to_ascii_lowercase(), v))
                .collect(),
            body,
        }
    }

    fn query(&self, name: &str) -> Option<&str> {
        self.query
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }

    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }

    fn json(&self) -> MockResult<Value> {
        if self.body.is_empty() {
            return Ok(Value::Null);
        }
        serde_json::from_slice(&self.body)
            .map_err(|e| MockError::bad("BadRequest", &format!("Invalid JSON: {e}")))
    }

    fn is_text(&self) -> bool {
        self.header("content-type")
            .is_some_and(|c| c.to_ascii_lowercase().starts_with("text/plain"))
    }

    fn page_size(&self) -> usize {
        self.header("prefer")
            .and_then(|p| {
                p.split(',')
                    .find_map(|part| part.trim().strip_prefix("odata.maxpagesize="))
                    .and_then(|n| n.trim().parse().ok())
            })
            .or_else(|| self.query("$top").and_then(|t| t.parse().ok()))
            .unwrap_or(10usize)
            .clamp(1, 1000)
    }
}

/// An answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Reply {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Reply {
    pub fn json(status: u16, value: &Value) -> Self {
        Self {
            status,
            headers: vec![(
                "content-type".into(),
                "application/json; odata.metadata=minimal".into(),
            )],
            body: value.to_string().into_bytes(),
        }
    }

    fn ok(value: &Value) -> Self {
        Self::json(200, value)
    }

    fn empty(status: u16) -> Self {
        Self {
            status,
            headers: Vec::new(),
            body: Vec::new(),
        }
    }

    fn bytes(content_type: &str, body: Vec<u8>) -> Self {
        Self {
            status: 200,
            headers: vec![("content-type".into(), content_type.into())],
            body,
        }
    }

    fn error(error: &MockError) -> Self {
        Self::json(error.status, &error.body)
    }
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 3 <= bytes.len()
            && let Some(b) = std::str::from_utf8(&bytes[i + 1..i + 3])
                .ok()
                .and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            out.push(b);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Recorded error bodies (`fixtures/responses/errors.json`).
pub(crate) fn recorded_error(name: &str) -> Value {
    let all: Value = serde_json::from_str(super::fixtures::RECORDED_ERRORS).unwrap_or(Value::Null);
    all.get(name)
        .cloned()
        .unwrap_or_else(|| json!({"error": {"code": "UnknownError", "message": name}}))
}

/// Handles one HTTP request (or one part of a batch).
pub(crate) fn dispatch(state: &mut State, req: &Req, in_batch: bool) -> Reply {
    let token = req
        .header("authorization")
        .and_then(|a| a.strip_prefix("Bearer "))
        .map(str::to_owned);
    state.requests.push(RecordedRequest {
        method: req.method.clone(),
        path: req.path.clone(),
        query: req.raw_query.clone(),
        in_batch,
        token: token.clone(),
        prefer: req.header("prefer").map(str::to_owned),
        if_match: req.header("if-match").map(str::to_owned),
        content_type: req.header("content-type").map(str::to_owned),
        body: String::from_utf8_lossy(&req.body).into_owned(),
    });
    if req
        .header("prefer")
        .is_some_and(|p| p.contains("IdType=\"ImmutableId\""))
    {
        state.tenant.ids.immutable = true;
    }
    if !in_batch {
        let accepted = match (&state.token, &token) {
            (_, None) => false,
            (None, Some(t)) => !t.is_empty(),
            (Some(want), Some(t)) => want == t,
        };
        if !accepted {
            return Reply::json(401, &recorded_error("401"));
        }
    }
    let target = if req.raw_query.is_empty() {
        req.path.clone()
    } else {
        format!("{}?{}", req.path, req.raw_query)
    };
    if let Some(i) = state
        .errors
        .iter()
        .position(|e| e.matches(&req.method, &target))
    {
        let error = state.errors[i].clone();
        state.errors[i].times -= 1;
        if state.errors[i].times == 0 {
            state.errors.remove(i);
        }
        let mut reply = Reply::json(error.status, &error.body);
        if let Some(retry) = error.retry_after {
            reply.headers.push(("retry-after".into(), retry));
        }
        return reply;
    }
    if !(in_batch && req.segments.first().is_some_and(|s| s == "$batch"))
        && let Some(budget) = &mut state.budget
    {
        budget.used += 1;
        if budget.used > budget.limit {
            let mut reply = Reply::json(429, &recorded_error("429"));
            reply
                .headers
                .push(("retry-after".into(), budget.retry_after.to_string()));
            return reply;
        }
    }
    if req.segments.first().is_some_and(|s| s == "$batch") {
        if in_batch {
            return Reply::error(&MockError::bad("BadRequest", "nested $batch"));
        }
        return batch(state, req);
    }
    match route(state, req) {
        Ok(reply) => reply,
        Err(error) => Reply::error(&error),
    }
}

fn batch(state: &mut State, req: &Req) -> Reply {
    let body = match req.json() {
        Ok(b) => b,
        Err(e) => return Reply::error(&e),
    };
    let Some(requests) = body.get("requests").and_then(Value::as_array).cloned() else {
        return Reply::error(&MockError::bad("BadRequest", "Missing requests"));
    };
    if requests.len() > 20 {
        return Reply::error(&MockError::bad(
            "BadRequest",
            "The number of batch requests exceeds the limit of 20.",
        ));
    }
    let mut responses = Vec::new();
    let mut seen = BTreeSet::new();
    for part in requests {
        let id = part["id"].as_str().unwrap_or_default().to_owned();
        if !seen.insert(id.clone()) {
            return Reply::error(&MockError::bad("BadRequest", "Duplicate request id"));
        }
        let url = part["url"].as_str().unwrap_or_default();
        let (path, query) = url.split_once('?').unwrap_or((url, ""));
        let mut headers: Vec<(String, String)> = part["headers"]
            .as_object()
            .map(|h| {
                h.iter()
                    .map(|(k, v)| (k.clone(), v.as_str().unwrap_or_default().to_owned()))
                    .collect()
            })
            .unwrap_or_default();
        if let Some(auth) = req.header("authorization") {
            headers.push(("authorization".into(), auth.to_owned()));
        }
        let body = match &part["body"] {
            Value::Null => Vec::new(),
            v => v.to_string().into_bytes(),
        };
        let inner = Req::new(
            part["method"].as_str().unwrap_or("GET"),
            &format!("{}{path}", nail_api::exchange::GRAPH_BASE_PATH),
            query,
            headers,
            body,
        );
        let reply = dispatch(state, &inner, true);
        let mut headers = serde_json::Map::new();
        for (k, v) in &reply.headers {
            headers.insert(k.clone(), json!(v));
        }
        let body = if reply.body.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&reply.body).unwrap_or_else(|_| {
                json!(base64::engine::general_purpose::STANDARD.encode(&reply.body))
            })
        };
        responses.push(json!({"id": id, "status": reply.status, "headers": headers, "body": body}));
    }
    Reply::ok(&json!({ "responses": responses }))
}

fn segs(req: &Req) -> Vec<&str> {
    req.segments.iter().map(String::as_str).collect()
}

fn route(state: &mut State, req: &Req) -> MockResult<Reply> {
    let segments = segs(req);
    let origin = state.origin.clone();
    let tenant = &mut state.tenant;
    match segments.as_slice() {
        ["places", rest @ ..] => places(tenant, req, rest, &origin),
        ["me", rest @ ..] => {
            let me = tenant.me.clone();
            mailbox_route(tenant, &me, req, rest, &origin)
        }
        ["users", upn, rest @ ..] => {
            let upn = (*upn).to_ascii_lowercase();
            mailbox_route(tenant, &upn, req, rest, &origin)
        }
        _ => Err(MockError::bad(
            "BadRequest",
            &format!("Resource not found for the segment '{}'.", req.path),
        )),
    }
}

fn user_json(mb: &Mailbox) -> Value {
    json!({"id": mb.user_id, "displayName": mb.display_name, "mail": mb.address,
        "userPrincipalName": mb.address})
}

fn mailbox_route(
    tenant: &mut Tenant,
    address: &str,
    req: &Req,
    rest: &[&str],
    origin: &str,
) -> MockResult<Reply> {
    let method = req.method.as_str();
    // Shared plumbing outside a single mailbox.
    match (method, rest) {
        ("POST", ["calendar", "getSchedule"]) => return get_schedule(tenant, req),
        ("POST", ["findMeetingTimes"]) => return find_meeting_times(tenant, req),
        ("POST", ["sendMail"]) => return send_mail(tenant, address, req),
        _ => {}
    }
    let ids = &mut tenant.ids;
    let me = tenant.me.clone();
    let mailbox = {
        let address = address.to_ascii_lowercase();
        if tenant.denied.contains(&address) {
            return Err(MockError::new(
                403,
                "ErrorAccessDenied",
                "Access is denied. Check credentials and try again.",
            ));
        }
        if tenant.on_premises.contains(&address) {
            return Err(MockError::new(
                404,
                "MailboxNotEnabledForRESTAPI",
                "The mailbox is either inactive, soft-deleted, or is hosted on-premise.",
            ));
        }
        tenant.mailboxes.get_mut(&address).ok_or_else(|| {
            MockError::new(
                404,
                "ErrorInvalidUser",
                &format!("The requested user '{address}' is invalid."),
            )
        })?
    };
    let actions = &mut tenant.meeting_actions;
    match (method, rest) {
        ("GET", []) => Ok(Reply::ok(&user_json(mailbox))),
        // Mailbox settings.
        ("GET", ["mailboxSettings"]) => Ok(Reply::ok(&json!({
            "automaticRepliesSetting": mailbox.automatic_replies,
            "timeZone": mailbox.time_zone,
        }))),
        ("GET", ["mailboxSettings", "automaticRepliesSetting"]) => {
            Ok(Reply::ok(&mailbox.automatic_replies))
        }
        ("PATCH", ["mailboxSettings"]) => {
            let body = req.json()?;
            if let Some(oof) = body.get("automaticRepliesSetting") {
                let status = oof["status"].as_str().unwrap_or("disabled");
                if !["disabled", "alwaysEnabled", "scheduled"].contains(&status) {
                    return Err(MockError::bad("ErrorInvalidArgument", "Invalid status"));
                }
                merge(&mut mailbox.automatic_replies, oof);
            }
            if let Some(zone) = body.get("timeZone").and_then(Value::as_str) {
                mailbox.time_zone = zone.to_owned();
            }
            Ok(Reply::ok(&body))
        }
        // Folders.
        ("GET", ["mailFolders", "delta"]) => {
            let mut initial: Vec<(usize, String)> = mailbox
                .folders
                .values()
                .filter(|f| f.parent.is_some())
                .map(|f| (depth(mailbox, &f.id), f.id.clone()))
                .collect();
            initial.sort();
            let initial = initial.into_iter().map(|(_, id)| id).collect();
            delta(mailbox, req, origin, "folders", initial, |mb, id| {
                mb.folders
                    .get(id)
                    .filter(|f| f.parent.is_some())
                    .map(|f| mb.folder_json(f))
            })
        }
        ("GET", ["mailFolders"]) => {
            let root = mailbox.root();
            let value: Vec<Value> = mailbox
                .folders
                .values()
                .filter(|f| f.parent.as_deref() == Some(root.as_str()))
                .map(|f| mailbox.folder_json(f))
                .collect();
            Ok(Reply::ok(&json!({ "value": value })))
        }
        ("POST", ["mailFolders"]) => {
            let body = req.json()?;
            let name = body["displayName"].as_str().unwrap_or_default();
            let id = mailbox.add_folder(ids, None, name)?;
            Ok(Reply::json(
                201,
                &mailbox.folder_json(&mailbox.folders[&id]),
            ))
        }
        ("POST", ["mailFolders", parent, "childFolders"]) => {
            let body = req.json()?;
            let name = body["displayName"].as_str().unwrap_or_default();
            let id = mailbox.add_folder(ids, Some(parent), name)?;
            Ok(Reply::json(
                201,
                &mailbox.folder_json(&mailbox.folders[&id]),
            ))
        }
        ("GET", ["mailFolders", folder]) => {
            let id = mailbox
                .folder_id(folder)
                .ok_or_else(|| MockError::not_found(folder))?;
            Ok(Reply::ok(&mailbox.folder_json(&mailbox.folders[&id])))
        }
        ("PATCH", ["mailFolders", folder]) => {
            let body = req.json()?;
            mailbox.rename_folder(folder, body["displayName"].as_str().unwrap_or_default())?;
            let id = mailbox.folder_id(folder).unwrap_or_default();
            Ok(Reply::ok(&mailbox.folder_json(&mailbox.folders[&id])))
        }
        ("DELETE", ["mailFolders", folder]) => {
            mailbox.remove_folder(folder)?;
            Ok(Reply::empty(204))
        }
        ("GET", ["mailFolders", folder, "messages", "delta"]) => {
            let folder = mailbox
                .folder_id(folder)
                .ok_or_else(|| MockError::not_found(folder))?;
            let since = req
                .query("$filter")
                .and_then(|f| f.strip_prefix("receivedDateTime ge "))
                .map(str::to_owned);
            if req.query("$filter").is_some() && since.is_none() {
                return Err(MockError::bad(
                    "ErrorInvalidUrlQueryFilter",
                    "Unsupported filter",
                ));
            }
            let initial = mailbox
                .messages
                .values()
                .filter(|m| m.folder == folder)
                .filter(|m| {
                    since
                        .as_ref()
                        .is_none_or(|s| m.received.as_str() >= s.as_str())
                })
                .map(|m| m.id.clone())
                .collect();
            let collection = format!("msgs:{folder}");
            delta(mailbox, req, origin, &collection, initial, |mb, id| {
                mb.messages
                    .get(id)
                    .filter(|m| m.folder == folder)
                    .map(super::tenant::MockMessage::to_json)
            })
        }
        ("GET", ["mailFolders", folder, "messages"]) => {
            let folder = mailbox
                .folder_id(folder)
                .ok_or_else(|| MockError::not_found(folder))?;
            let wanted = match req.query("$filter") {
                None => None,
                Some(f) => Some(
                    f.strip_prefix("internetMessageId eq '")
                        .and_then(|r| r.strip_suffix('\''))
                        .ok_or_else(|| {
                            MockError::bad("ErrorInvalidUrlQueryFilter", "Unsupported filter")
                        })?
                        .to_owned(),
                ),
            };
            let value: Vec<Value> = mailbox
                .messages
                .values()
                .filter(|m| m.folder == folder)
                .filter(|m| {
                    wanted
                        .as_ref()
                        .is_none_or(|w| mime::message_id(&m.raw).as_deref() == Some(w.as_str()))
                })
                .take(req.page_size())
                .map(super::tenant::MockMessage::to_json)
                .collect();
            Ok(Reply::ok(&json!({ "value": value })))
        }
        ("POST", ["mailFolders", folder, "messages"]) => create_mime(mailbox, ids, folder, req),
        ("POST", ["messages"]) => create_mime(mailbox, ids, "drafts", req),
        // Rules.
        ("GET", ["mailFolders", "inbox", "messageRules"]) => {
            Ok(Reply::ok(&json!({ "value": mailbox.rules })))
        }
        ("POST", ["mailFolders", "inbox", "messageRules"]) => {
            let mut rule = req.json()?;
            if rule["displayName"].as_str().is_none_or(str::is_empty) {
                return Err(MockError::bad(
                    "ErrorInvalidArgument",
                    "A rule needs a name",
                ));
            }
            check_rule_folders(mailbox, &rule)?;
            rule["id"] = json!(ids.make("rul"));
            rule["hasError"] = json!(false);
            rule["isReadOnly"] = json!(false);
            mailbox.rules.push(rule.clone());
            Ok(Reply::json(201, &rule))
        }
        ("GET" | "PATCH" | "DELETE", ["mailFolders", "inbox", "messageRules", id]) => {
            let index = mailbox
                .rules
                .iter()
                .position(|r| r["id"] == *id)
                .ok_or_else(|| MockError::not_found(id))?;
            match method {
                "GET" => Ok(Reply::ok(&mailbox.rules[index])),
                "DELETE" => {
                    mailbox.rules.remove(index);
                    Ok(Reply::empty(204))
                }
                _ => {
                    let patch = req.json()?;
                    check_rule_folders(mailbox, &patch)?;
                    merge(&mut mailbox.rules[index], &patch);
                    Ok(Reply::ok(&mailbox.rules[index]))
                }
            }
        }
        // Messages.
        ("GET", ["messages", id]) => {
            let m = mailbox
                .messages
                .get(*id)
                .ok_or_else(|| MockError::not_found(id))?;
            Ok(Reply::ok(&m.to_json()))
        }
        ("GET", ["messages", id, "$value"]) => {
            let m = mailbox
                .messages
                .get(*id)
                .ok_or_else(|| MockError::not_found(id))?;
            Ok(Reply::bytes("text/plain", m.raw.clone()))
        }
        ("PATCH", ["messages", id]) => {
            let body = req.json()?;
            if let Some(i) = body.get("inferenceClassification").and_then(Value::as_str)
                && i != "focused"
                && i != "other"
            {
                return Err(MockError::bad(
                    "ErrorInvalidArgument",
                    "inferenceClassification",
                ));
            }
            mailbox.change_message(id, |m| {
                if let Some(read) = body.get("isRead").and_then(Value::as_bool) {
                    m.is_read = read;
                }
                if let Some(flag) = body.pointer("/flag/flagStatus").and_then(Value::as_str) {
                    m.flag = flag.to_owned();
                }
                if let Some(categories) = body.get("categories").and_then(Value::as_array) {
                    m.categories = categories
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_owned)
                        .collect();
                }
                if let Some(i) = body.get("inferenceClassification").and_then(Value::as_str) {
                    m.inference = i.to_owned();
                }
                if let Some(props) = body
                    .get("singleValueExtendedProperties")
                    .and_then(Value::as_array)
                {
                    for p in props {
                        if p["id"] == "Integer 0x1081" {
                            m.last_verb = p["value"].as_str().map(str::to_owned);
                        }
                    }
                }
            })?;
            Ok(Reply::ok(&mailbox.messages[*id].to_json()))
        }
        ("DELETE", ["messages", id]) => {
            mailbox.move_message(ids, id, "deleteditems", false)?;
            Ok(Reply::empty(204))
        }
        ("POST", ["messages", id, action @ ("move" | "copy")]) => {
            let body = req.json()?;
            let to = body["destinationId"].as_str().unwrap_or_default();
            let new_id = mailbox.move_message(ids, id, to, *action == "copy")?;
            Ok(Reply::json(201, &mailbox.messages[&new_id].to_json()))
        }
        ("POST", ["messages", id, "permanentDelete"]) => {
            mailbox.remove_message(id)?;
            Ok(Reply::empty(204))
        }
        ("GET", ["messages", id, "attachments"]) => {
            let m = mailbox
                .messages
                .get(*id)
                .ok_or_else(|| MockError::not_found(id))?;
            Ok(Reply::ok(
                &json!({ "value": attachments(&m.raw).into_iter().map(|(v, _)| v).collect::<Vec<_>>() }),
            ))
        }
        ("GET", ["messages", id, "attachments", att, "$value"]) => {
            let m = mailbox
                .messages
                .get(*id)
                .ok_or_else(|| MockError::not_found(id))?;
            attachments(&m.raw)
                .into_iter()
                .find(|(v, _)| v["id"] == *att)
                .map(|(v, bytes)| {
                    Reply::bytes(
                        v["contentType"]
                            .as_str()
                            .unwrap_or("application/octet-stream"),
                        bytes,
                    )
                })
                .ok_or_else(|| MockError::not_found(att))
        }
        // Focused Inbox overrides.
        ("GET", ["inferenceClassification", "overrides"]) => {
            Ok(Reply::ok(&json!({ "value": mailbox.overrides })))
        }
        ("POST", ["inferenceClassification", "overrides"]) => {
            let body = req.json()?;
            let address = body
                .pointer("/senderEmailAddress/address")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_ascii_lowercase();
            if address.is_empty() {
                return Err(MockError::bad("ErrorInvalidArgument", "senderEmailAddress"));
            }
            if let Some(existing) = mailbox.overrides.iter_mut().find(|o| {
                o.pointer("/senderEmailAddress/address")
                    .and_then(Value::as_str)
                    .is_some_and(|a| a.eq_ignore_ascii_case(&address))
            }) {
                existing["classifyAs"] = body["classifyAs"].clone();
                return Ok(Reply::ok(existing));
            }
            let mut o = body.clone();
            o["id"] = json!(ids.make("ovr"));
            mailbox.overrides.push(o.clone());
            Ok(Reply::json(201, &o))
        }
        // Categories.
        ("GET", ["outlook", "masterCategories"]) => {
            Ok(Reply::ok(&json!({ "value": mailbox.categories })))
        }
        ("POST", ["outlook", "masterCategories"]) => {
            let mut body = req.json()?;
            let name = body["displayName"].as_str().unwrap_or_default().to_owned();
            if name.trim().is_empty() {
                return Err(MockError::bad("ErrorInvalidArgument", "displayName"));
            }
            check_color(&body)?;
            if mailbox.categories.iter().any(|c| {
                c["displayName"]
                    .as_str()
                    .is_some_and(|n| n.eq_ignore_ascii_case(&name))
            }) {
                return Err(MockError::new(
                    409,
                    "ErrorItemSave",
                    "A category with this name already exists.",
                ));
            }
            body["id"] = json!(ids.make("cat"));
            mailbox.categories.push(body.clone());
            Ok(Reply::json(201, &body))
        }
        ("GET" | "PATCH" | "DELETE", ["outlook", "masterCategories", id]) => {
            let index = mailbox
                .categories
                .iter()
                .position(|c| c["id"] == *id)
                .ok_or_else(|| MockError::not_found(id))?;
            match method {
                "GET" => Ok(Reply::ok(&mailbox.categories[index])),
                "DELETE" => {
                    mailbox.categories.remove(index);
                    Ok(Reply::empty(204))
                }
                _ => {
                    let patch = req.json()?;
                    if patch.get("displayName").is_some() {
                        return Err(MockError::bad(
                            "ErrorInvalidProperty",
                            "displayName is read-only",
                        ));
                    }
                    check_color(&patch)?;
                    merge(&mut mailbox.categories[index], &patch);
                    Ok(Reply::ok(&mailbox.categories[index]))
                }
            }
        }
        // Calendars and events.
        ("GET", ["calendars"]) => Ok(Reply::ok(&json!({ "value": mailbox.calendars }))),
        ("GET", ["calendar"]) => Ok(Reply::ok(&mailbox.calendars[0])),
        ("GET", ["calendars", calendar, "calendarView", "delta"]) => {
            calendar_delta(mailbox, req, origin, calendar)
        }
        ("GET", ["calendarView", "delta"]) => {
            let calendar = mailbox.calendars[0]["id"]
                .as_str()
                .unwrap_or_default()
                .to_owned();
            calendar_delta(mailbox, req, origin, &calendar)
        }
        ("POST", ["calendars", calendar, "events"]) => {
            let body = req.json()?;
            if body.get("start").is_none() || body.get("end").is_none() {
                return Err(MockError::bad(
                    "ErrorInvalidRequest",
                    "start and end are required",
                ));
            }
            let id = mailbox.add_event(ids, Some(calendar), body)?;
            Ok(Reply::json(201, &mailbox.events[&id]))
        }
        ("GET", ["events", id]) => event_or_occurrence(mailbox, id).map(|e| Reply::ok(&e)),
        ("PATCH", ["events", id]) => patch_event(mailbox, id, req),
        ("DELETE", ["events", id]) => {
            delete_event(mailbox, id)?;
            Ok(Reply::empty(204))
        }
        ("GET", ["events", id, "instances"]) => {
            let master = mailbox
                .events
                .get(*id)
                .cloned()
                .ok_or_else(|| MockError::not_found(id))?;
            let from = req.query("startDateTime").unwrap_or("1900-01-01T00:00:00Z");
            let to = req.query("endDateTime").unwrap_or("2200-01-01T00:00:00Z");
            let value = instances(mailbox, id, &master, from, to);
            page_list(req, origin, value)
        }
        (
            "POST",
            [
                "events",
                id,
                action @ ("accept" | "tentativelyAccept" | "decline" | "cancel"),
            ],
        ) => {
            let body = req.json()?;
            let event = mailbox
                .events
                .get_mut(*id)
                .ok_or_else(|| MockError::not_found(id))?;
            if *action == "cancel" {
                if event["isOrganizer"] != true {
                    return Err(MockError::bad(
                        "ErrorAccessDenied",
                        "Only the organizer can cancel the meeting.",
                    ));
                }
                mailbox.events.remove(*id);
                mailbox.touch_event(id);
                mailbox.event_calendar.remove(*id);
            } else {
                if event["isOrganizer"] == true {
                    return Err(MockError::bad(
                        "ErrorInvalidRequest",
                        "Your request can't be completed. You can't respond to a meeting you organized.",
                    ));
                }
                if body.get("proposedNewTime").is_some_and(|p| !p.is_null()) && *action == "accept"
                {
                    return Err(MockError::bad(
                        "ErrorInvalidRequest",
                        "proposedNewTime needs tentativelyAccept or decline",
                    ));
                }
                let response = match *action {
                    "accept" => "accepted",
                    "tentativelyAccept" => "tentativelyAccepted",
                    _ => "declined",
                };
                event["responseStatus"] =
                    json!({"response": response, "time": "2026-10-01T10:00:00Z"});
                mailbox.bump_event(id);
                mailbox.touch_event(id);
            }
            actions.push(MeetingAction {
                event_id: (*id).to_owned(),
                action: (*action).to_owned(),
                body,
            });
            Ok(Reply::empty(202))
        }
        // Contacts.
        ("GET", ["contactFolders"]) => Ok(Reply::ok(&json!({ "value": mailbox.contact_folders }))),
        ("GET", ["contacts"]) => {
            let value: Vec<Value> = mailbox
                .contacts
                .iter()
                .filter(|(id, _)| {
                    mailbox.contact_folder.get(*id) == Some(&mailbox.default_contacts)
                })
                .map(|(_, c)| c.clone())
                .collect();
            page_list(req, origin, value)
        }
        ("POST", ["contacts"]) => {
            let id = mailbox.add_contact(ids, None, req.json()?)?;
            Ok(Reply::json(201, &mailbox.contacts[&id]))
        }
        ("POST", ["contactFolders", folder, "contacts"]) => {
            let id = mailbox.add_contact(ids, Some(folder), req.json()?)?;
            Ok(Reply::json(201, &mailbox.contacts[&id]))
        }
        ("GET", ["contactFolders", folder, "contacts", "delta"]) => {
            let folder = (*folder).to_owned();
            if folder != mailbox.default_contacts
                && !mailbox.contact_folders.iter().any(|f| f["id"] == folder)
            {
                return Err(MockError::not_found(&folder));
            }
            let initial = mailbox
                .contact_folder
                .iter()
                .filter(|(_, f)| **f == folder)
                .map(|(id, _)| id.clone())
                .collect();
            let collection = format!("contacts:{folder}");
            delta(mailbox, req, origin, &collection, initial, |mb, id| {
                (mb.contact_folder.get(id) == Some(&folder))
                    .then(|| mb.contacts.get(id).cloned())
                    .flatten()
            })
        }
        ("GET", ["contacts", id]) => mailbox
            .contacts
            .get(*id)
            .map(Reply::ok)
            .ok_or_else(|| MockError::not_found(id)),
        ("GET", ["contacts", _, "photo", "$value"]) => Err(MockError::new(
            404,
            "ImageNotFound",
            "The photo wasn't found.",
        )),
        ("PATCH", ["contacts", id]) => {
            let patch = req.json()?;
            let contact = mailbox
                .contacts
                .get_mut(*id)
                .ok_or_else(|| MockError::not_found(id))?;
            check_if_match(
                req,
                &format!(
                    "W/\"{}\"",
                    contact["changeKey"].as_str().unwrap_or_default()
                ),
            )?;
            merge(contact, &patch);
            let next = contact["changeKey"]
                .as_str()
                .unwrap_or("EQAAAB0")
                .trim_start_matches("EQAAAB")
                .parse::<u64>()
                .unwrap_or(0)
                + 1;
            contact["changeKey"] = json!(format!("EQAAAB{next}"));
            let value = contact.clone();
            let folder = mailbox.contact_folder.get(*id).cloned().unwrap_or_default();
            mailbox.touch(&format!("contacts:{folder}"), id);
            Ok(Reply::ok(&value))
        }
        ("DELETE", ["contacts", id]) => {
            mailbox
                .contacts
                .remove(*id)
                .ok_or_else(|| MockError::not_found(id))?;
            let folder = mailbox.contact_folder.remove(*id).unwrap_or_default();
            mailbox.touch(&format!("contacts:{folder}"), id);
            Ok(Reply::empty(204))
        }
        // To Do (own mailbox only).
        (_, ["todo", ..]) if address != me => Err(MockError::not_found("todo")),
        ("GET", ["todo", "lists"]) => Ok(Reply::ok(&json!({ "value": mailbox.todo_lists }))),
        ("GET", ["todo", "lists", list, "tasks", "delta"]) => {
            let list = (*list).to_owned();
            if !mailbox.todo_lists.iter().any(|l| l["id"] == list) {
                return Err(MockError::not_found(&list));
            }
            let initial = mailbox
                .tasks
                .iter()
                .filter(|(_, t)| t.list == list)
                .map(|(id, _)| id.clone())
                .collect();
            let collection = format!("todo:{list}");
            delta(mailbox, req, origin, &collection, initial, |mb, id| {
                mb.tasks
                    .get(id)
                    .filter(|t| t.list == list)
                    .map(|t| t.value.clone())
            })
        }
        ("POST", ["todo", "lists", list, "tasks"]) => {
            let body = req.json()?;
            if body["title"].as_str().is_none_or(str::is_empty) {
                return Err(MockError::bad("invalidRequest", "title is required"));
            }
            let id = mailbox.add_task(ids, Some(list), body, Vec::new())?;
            Ok(Reply::json(201, &mailbox.tasks[&id].value))
        }
        ("GET" | "PATCH" | "DELETE", ["todo", "lists", list, "tasks", id]) => {
            let task = mailbox
                .tasks
                .get_mut(*id)
                .filter(|t| t.list == *list)
                .ok_or_else(|| MockError::not_found(id))?;
            match method {
                "GET" => Ok(Reply::ok(&task.value)),
                "DELETE" => {
                    mailbox.tasks.remove(*id);
                    mailbox.touch(&format!("todo:{list}"), id);
                    Ok(Reply::empty(204))
                }
                _ => {
                    check_if_match(req, task.value["@odata.etag"].as_str().unwrap_or_default())?;
                    let patch = req.json()?;
                    merge(&mut task.value, &patch);
                    mailbox.touch_task(id);
                    Ok(Reply::ok(&mailbox.tasks[*id].value))
                }
            }
        }
        (
            "GET",
            [
                "todo",
                "lists",
                list,
                "tasks",
                id,
                part @ ("checklistItems" | "linkedResources"),
            ],
        ) => {
            let task = mailbox
                .tasks
                .get(*id)
                .filter(|t| t.list == *list)
                .ok_or_else(|| MockError::not_found(id))?;
            let value = if *part == "checklistItems" {
                &task.checklist
            } else {
                &task.links
            };
            Ok(Reply::ok(&json!({ "value": value })))
        }
        ("POST", ["todo", "lists", list, "tasks", id, "checklistItems"]) => {
            let mut item = req.json()?;
            item["id"] = json!(ids.make("chk"));
            let task = mailbox
                .tasks
                .get_mut(*id)
                .filter(|t| t.list == *list)
                .ok_or_else(|| MockError::not_found(id))?;
            task.checklist.push(item.clone());
            mailbox.touch_task(id);
            Ok(Reply::json(201, &item))
        }
        ("PATCH" | "DELETE", ["todo", "lists", list, "tasks", id, "checklistItems", item]) => {
            let patch = req.json()?;
            let task = mailbox
                .tasks
                .get_mut(*id)
                .filter(|t| t.list == *list)
                .ok_or_else(|| MockError::not_found(id))?;
            let index = task
                .checklist
                .iter()
                .position(|c| c["id"] == *item)
                .ok_or_else(|| MockError::not_found(item))?;
            let reply = if method == "DELETE" {
                task.checklist.remove(index);
                Reply::empty(204)
            } else {
                merge(&mut task.checklist[index], &patch);
                Reply::ok(&task.checklist[index])
            };
            mailbox.touch_task(id);
            Ok(reply)
        }
        _ => Err(MockError::bad(
            "BadRequest",
            &format!("Resource not found for the segment '{}'.", req.path),
        )),
    }
}

fn depth(mb: &Mailbox, id: &str) -> usize {
    let mut depth = 0;
    let mut current = mb.folders.get(id).and_then(|f| f.parent.clone());
    while let Some(parent) = current {
        depth += 1;
        current = mb.folders.get(&parent).and_then(|f| f.parent.clone());
        if depth > 64 {
            break;
        }
    }
    depth
}

fn check_color(body: &Value) -> MockResult<()> {
    match body.get("color").and_then(Value::as_str) {
        None => Ok(()),
        Some(c) if nail_api::exchange::CategoryColor::parse(c).is_some() => Ok(()),
        Some(_) => Err(MockError::bad("ErrorInvalidArgument", "color")),
    }
}

fn check_rule_folders(mb: &Mailbox, rule: &Value) -> MockResult<()> {
    for key in ["moveToFolder", "copyToFolder"] {
        if let Some(folder) = rule
            .pointer(&format!("/actions/{key}"))
            .and_then(Value::as_str)
            && mb.folder_id(folder).is_none()
        {
            return Err(MockError::bad(
                "ErrorInvalidIdMalformed",
                "The folder of the rule does not exist.",
            ));
        }
    }
    Ok(())
}

fn check_if_match(req: &Req, current: &str) -> MockResult<()> {
    match req.header("if-match") {
        Some(want) if want != "*" && want != current => Err(MockError::new(
            412,
            "ErrorIrresolvableConflict",
            "The send or update operation could not be performed because the change key passed in the request does not match the current change key for the item.",
        )),
        _ => Ok(()),
    }
}

fn create_mime(
    mailbox: &mut Mailbox,
    ids: &mut super::tenant::IdGen,
    folder: &str,
    req: &Req,
) -> MockResult<Reply> {
    if !req.is_text() {
        return Err(MockError::bad(
            "ErrorMimeContentInvalid",
            "This mock takes MIME uploads only",
        ));
    }
    let raw = base64::engine::general_purpose::STANDARD
        .decode(String::from_utf8_lossy(&req.body).trim())
        .map_err(|_| {
            MockError::bad(
                "ErrorMimeContentInvalidBase64String",
                "Invalid base64 string for MIME content.",
            )
        })?;
    let id = mailbox.add_message(ids, folder, raw, false, true)?;
    Ok(Reply::json(201, &mailbox.messages[&id].to_json()))
}

fn send_mail(tenant: &mut Tenant, address: &str, req: &Req) -> MockResult<Reply> {
    let address = address.to_ascii_lowercase();
    let raw = if req.is_text() {
        base64::engine::general_purpose::STANDARD
            .decode(String::from_utf8_lossy(&req.body).trim())
            .map_err(|_| {
                MockError::bad(
                    "ErrorMimeContentInvalidBase64String",
                    "Invalid base64 string for MIME content.",
                )
            })?
    } else {
        return Err(MockError::bad(
            "ErrorInvalidRequest",
            "This mock takes MIME uploads only",
        ));
    };
    if mime::header_recipients(&raw).is_empty() {
        return Err(MockError::bad(
            "ErrorInvalidRecipients",
            "At least one recipient isn't valid.",
        ));
    }
    let me = tenant.me.clone();
    let ids = &mut tenant.ids;
    let mailbox = if address == me {
        tenant.mailboxes.get_mut(&me)
    } else if tenant.denied.contains(&address) {
        return Err(MockError::new(403, "ErrorSendAsDenied", "The user account which was used to submit this request does not have the right to send mail on behalf of the specified sending account."));
    } else {
        tenant.mailboxes.get_mut(&address)
    }
    .ok_or_else(|| MockError::new(404, "ErrorInvalidUser", "The requested user is invalid."))?;
    // The Sent Items copy has no Bcc header.
    let copy: Vec<u8> = {
        let text = String::from_utf8_lossy(&raw);
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
    };
    mailbox.add_message(ids, "sentitems", copy, true, false)?;
    tenant.sent.push(SentMail {
        mailbox: address,
        raw,
    });
    Ok(Reply::empty(202))
}

/// A list answer paged with `$top` / `odata.maxpagesize` and a skip token.
fn page_list(req: &Req, origin: &str, value: Vec<Value>) -> MockResult<Reply> {
    let size = req.page_size();
    let offset: usize = req
        .query("$skiptoken")
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let page: Vec<Value> = value.iter().skip(offset).take(size).cloned().collect();
    let mut out = json!({ "value": page });
    if offset + size < value.len() {
        out["@odata.nextLink"] = json!(link(
            req,
            origin,
            "$skiptoken",
            &(offset + size).to_string()
        ));
    }
    Ok(Reply::ok(&out))
}

fn link(req: &Req, origin: &str, key: &str, token: &str) -> String {
    let mut query = url::form_urlencoded::Serializer::new(String::new());
    for (k, v) in &req.query {
        if k != "$skiptoken" && k != "$deltatoken" {
            query.append_pair(k, v);
        }
    }
    query.append_pair(key, token);
    format!("{origin}{}?{}", req.path, query.finish())
}

/// A delta answer over `collection`: the initial listing (`initial`) or
/// the changes since a delta token, paged with skip tokens
/// `<from>.<to>.<offset>` (`from` = `i` for the initial listing).
fn delta(
    mb: &Mailbox,
    req: &Req,
    origin: &str,
    collection: &str,
    initial: Vec<String>,
    render: impl Fn(&Mailbox, &str) -> Option<Value>,
) -> MockResult<Reply> {
    let expired = || {
        MockError::new(
            410,
            "SyncStateNotFound",
            "The sync state generation is not found. Please restart the sync.",
        )
    };
    let (from, to, offset) = if let Some(token) = req.query("$deltatoken") {
        let t: u64 = token.parse().map_err(|_| expired())?;
        if !mb.token_valid(t) {
            return Err(expired());
        }
        (Some(t), mb.seq(), 0usize)
    } else if let Some(skip) = req.query("$skiptoken") {
        let mut parts = skip.split('.');
        let from = parts.next().unwrap_or("i");
        let to: u64 = parts
            .next()
            .and_then(|t| t.parse().ok())
            .ok_or_else(expired)?;
        let offset: usize = parts
            .next()
            .and_then(|t| t.parse().ok())
            .ok_or_else(expired)?;
        let from = if from == "i" {
            None
        } else {
            let f: u64 = from.parse().map_err(|_| expired())?;
            if !mb.token_valid(f) {
                return Err(expired());
            }
            Some(f)
        };
        (from, to, offset)
    } else {
        (None, mb.seq(), 0)
    };
    let ids = match from {
        None => initial,
        Some(f) => mb.changed_since(collection, f, to),
    };
    let size = req.page_size();
    let page: Vec<Value> = ids
        .iter()
        .skip(offset)
        .take(size)
        .map(|id| {
            render(mb, id).unwrap_or_else(|| json!({"id": id, "@removed": {"reason": "deleted"}}))
        })
        .collect();
    let mut out = json!({ "value": page });
    if offset + size < ids.len() {
        let from = from.map_or_else(|| "i".to_owned(), |f| f.to_string());
        out["@odata.nextLink"] = json!(link(
            req,
            origin,
            "$skiptoken",
            &format!("{from}.{to}.{}", offset + size)
        ));
    } else {
        out["@odata.deltaLink"] = json!(link(req, origin, "$deltatoken", &to.to_string()));
    }
    Ok(Reply::ok(&out))
}

fn calendar_delta(mb: &Mailbox, req: &Req, origin: &str, calendar: &str) -> MockResult<Reply> {
    if !mb.calendars.iter().any(|c| c["id"] == calendar) {
        return Err(MockError::not_found(calendar));
    }
    if req.query("$deltatoken").is_none()
        && req.query("$skiptoken").is_none()
        && (req.query("startDateTime").is_none() || req.query("endDateTime").is_none())
    {
        return Err(MockError::bad(
            "ErrorInvalidParameter",
            "This request requires a time window specified by the query string parameters StartDateTime and EndDateTime.",
        ));
    }
    let initial = mb
        .event_calendar
        .iter()
        .filter(|(_, c)| *c == calendar)
        .map(|(id, _)| id.clone())
        .collect();
    let collection = format!("cal:{calendar}");
    delta(mb, req, origin, &collection, initial, |mb, id| {
        let event = mb.events.get(id)?;
        let master_calendar = id
            .split(".occ.")
            .next()
            .and_then(|m| mb.event_calendar.get(m));
        if mb
            .event_calendar
            .get(id)
            .or(master_calendar)
            .map(String::as_str)
            != Some(calendar)
        {
            return None;
        }
        if event["type"] == "seriesMaster" {
            // calendarView lists occurrences, never the master.
            return Some(json!({
                "id": format!("{id}.occ.delta"),
                "type": "occurrence",
                "seriesMasterId": id,
                "originalStartTimeZone": event["originalStartTimeZone"],
                "start": event["start"], "end": event["end"],
            }));
        }
        Some(event.clone())
    })
}

/// The dates of a series' occurrences between `from` and `to` (daily,
/// weekly and absolute monthly patterns; enough for tests).
fn occurrence_dates(master: &Value, from: &str, to: &str) -> Vec<chrono::NaiveDate> {
    use chrono::{Datelike as _, NaiveDate};
    let Some(pattern) = master.pointer("/recurrence/pattern") else {
        return Vec::new();
    };
    let range = &master["recurrence"]["range"];
    let parse = |s: &str| NaiveDate::parse_from_str(s.get(..10).unwrap_or(s), "%Y-%m-%d").ok();
    let (Some(start), Some(from), Some(to)) = (
        range["startDate"].as_str().and_then(parse),
        parse(from),
        parse(to),
    ) else {
        return Vec::new();
    };
    let end = range["endDate"]
        .as_str()
        .and_then(parse)
        .filter(|_| range["type"] == "endDate");
    let count =
        (range["type"] == "numbered").then(|| range["numberOfOccurrences"].as_u64().unwrap_or(1));
    let interval = i64::try_from(pattern["interval"].as_u64().unwrap_or(1).max(1)).unwrap_or(1);
    let days: Vec<String> = pattern["daysOfWeek"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect();
    let mut out = Vec::new();
    let mut n = 0u64;
    let mut day = start;
    for _ in 0..5000 {
        if end.is_some_and(|e| day > e) || count.is_some_and(|c| n >= c) || day >= to {
            break;
        }
        let hit = match pattern["type"].as_str().unwrap_or("daily") {
            "daily" => (day - start).num_days() % interval == 0,
            "weekly" => {
                let weeks = (day - start).num_days().div_euclid(7);
                weeks % interval == 0
                    && days.iter().any(|d| {
                        d.get(..3)
                            .is_some_and(|p| p.eq_ignore_ascii_case(&day.weekday().to_string()))
                    })
            }
            "absoluteMonthly" => {
                u64::from(day.day()) == pattern["dayOfMonth"].as_u64().unwrap_or(1)
            }
            _ => day == start,
        };
        if hit {
            n += 1;
            if day >= from {
                out.push(day);
            }
        }
        day = day.succ_opt().unwrap_or(day);
    }
    out
}

fn occurrence(master_id: &str, master: &Value, date: &str) -> Value {
    let time = |key: &str| {
        let dt = master[key]["dateTime"].as_str().unwrap_or("T00:00:00");
        let time = dt.get(10..19).unwrap_or("T00:00:00");
        json!({"dateTime": format!("{date}{time}.0000000"), "timeZone": master[key]["timeZone"]})
    };
    let mut value = master.clone();
    if let Some(o) = value.as_object_mut() {
        o.remove("recurrence");
        o.remove("cancelledOccurrences");
    }
    value["id"] = json!(format!("{master_id}.occ.{date}"));
    value["type"] = json!("occurrence");
    value["seriesMasterId"] = json!(master_id);
    value["start"] = time("start");
    value["end"] = time("end");
    let start_time = master["start"]["dateTime"]
        .as_str()
        .and_then(|d| d.get(10..19))
        .unwrap_or("T00:00:00");
    value["originalStart"] = json!(format!("{date}{start_time}Z"));
    value
}

fn cancelled_dates(master: &Value) -> Vec<String> {
    master["cancelledOccurrences"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .filter_map(|o| o.get(o.len().saturating_sub(10)..))
        .map(str::to_owned)
        .collect()
}

fn instances(mb: &Mailbox, master_id: &str, master: &Value, from: &str, to: &str) -> Vec<Value> {
    let cancelled = cancelled_dates(master);
    occurrence_dates(master, from, to)
        .into_iter()
        .map(|d| d.format("%Y-%m-%d").to_string())
        .filter(|d| !cancelled.contains(d))
        .map(|d| {
            let id = format!("{master_id}.occ.{d}");
            mb.events
                .get(&id)
                .cloned()
                .unwrap_or_else(|| occurrence(master_id, master, &d))
        })
        .collect()
}

fn event_or_occurrence(mb: &Mailbox, id: &str) -> MockResult<Value> {
    if let Some(event) = mb.events.get(id) {
        return Ok(event.clone());
    }
    if let Some((master_id, date)) = id.split_once(".occ.")
        && let Some(master) = mb.events.get(master_id)
        && !cancelled_dates(master).iter().any(|d| d == date)
    {
        return Ok(occurrence(master_id, master, date));
    }
    Err(MockError::not_found(id))
}

fn patch_event(mb: &mut Mailbox, id: &str, req: &Req) -> MockResult<Reply> {
    let patch = req.json()?;
    if !mb.events.contains_key(id) {
        // Patching an occurrence makes it an exception.
        let mut occurrence = event_or_occurrence(mb, id)?;
        occurrence["type"] = json!("exception");
        mb.events.insert(id.to_owned(), occurrence);
    }
    let event = mb
        .events
        .get_mut(id)
        .ok_or_else(|| MockError::not_found(id))?;
    check_if_match(
        req,
        &format!("W/\"{}\"", event["changeKey"].as_str().unwrap_or_default()),
    )?;
    merge(event, &patch);
    if patch.get("recurrence").is_some_and(|r| !r.is_null()) && event["type"] == "singleInstance" {
        event["type"] = json!("seriesMaster");
    }
    mb.bump_event(id);
    mb.touch_event(id);
    Ok(Reply::ok(&mb.events[id]))
}

fn delete_event(mb: &mut Mailbox, id: &str) -> MockResult<()> {
    if let Some((master_id, date)) = id.split_once(".occ.") {
        let master = mb
            .events
            .get_mut(master_id)
            .ok_or_else(|| MockError::not_found(id))?;
        let mut cancelled = master["cancelledOccurrences"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        cancelled.push(json!(format!("OID.{master_id}.{date}")));
        master["cancelledOccurrences"] = Value::Array(cancelled);
        mb.events.remove(id);
        mb.bump_event(master_id);
        mb.touch_event(master_id);
        return Ok(());
    }
    if mb.events.remove(id).is_none() {
        return Err(MockError::not_found(id));
    }
    mb.touch_event(id);
    mb.event_calendar.remove(id);
    let exceptions: Vec<String> = mb
        .events
        .keys()
        .filter(|k| k.starts_with(&format!("{id}.occ.")))
        .cloned()
        .collect();
    for e in exceptions {
        mb.events.remove(&e);
    }
    Ok(())
}

/// The attachments of a raw message (top-level parts with a file name),
/// as metadata and bytes.
fn attachments(raw: &[u8]) -> Vec<(Value, Vec<u8>)> {
    let text = String::from_utf8_lossy(raw);
    let Some(boundary) = mime::header(raw, "content-type").and_then(|ct| {
        ct.split(';')
            .find_map(|p| p.trim().strip_prefix("boundary="))
            .map(|b| b.trim_matches('"').to_owned())
    }) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (i, part) in text.split(&format!("--{boundary}")).enumerate().skip(1) {
        let part = part.trim_start_matches("\r\n");
        let headers = part.as_bytes();
        let Some(name) = mime::header(headers, "content-disposition").and_then(|d| {
            d.split(';').find_map(|p| {
                p.trim()
                    .strip_prefix("filename=")
                    .map(|f| f.trim_matches('"').to_owned())
            })
        }) else {
            continue;
        };
        let content_type = mime::header(headers, "content-type")
            .and_then(|c| c.split(';').next().map(str::to_owned))
            .unwrap_or_else(|| "application/octet-stream".into());
        let body = part.split_once("\r\n\r\n").map_or("", |(_, b)| b);
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(body.split_whitespace().collect::<String>())
            .unwrap_or_else(|_| body.as_bytes().to_vec());
        out.push((
            json!({"@odata.type": "#microsoft.graph.fileAttachment", "id": format!("att{i}"),
                "name": name, "contentType": content_type, "size": bytes.len(), "isInline": false}),
            bytes,
        ));
    }
    out
}

fn places(tenant: &Tenant, req: &Req, rest: &[&str], origin: &str) -> MockResult<Reply> {
    if tenant.places_forbidden {
        return Err(MockError::new(
            403,
            "ErrorAccessDenied",
            "Access is denied. Check credentials and try again.",
        ));
    }
    let value = match rest {
        ["microsoft.graph.roomlist"] => tenant.room_lists.iter().map(|(p, _)| p.clone()).collect(),
        ["microsoft.graph.room"] => tenant.rooms.clone(),
        [list, "microsoft.graph.roomlist", "rooms"] => {
            let (_, rooms) = tenant
                .room_lists
                .iter()
                .find(|(p, _)| {
                    p["emailAddress"]
                        .as_str()
                        .is_some_and(|a| a.eq_ignore_ascii_case(list))
                })
                .ok_or_else(|| MockError::not_found(list))?;
            tenant
                .rooms
                .iter()
                .filter(|r| {
                    r["emailAddress"]
                        .as_str()
                        .is_some_and(|a| rooms.contains(&a.to_ascii_lowercase()))
                })
                .cloned()
                .collect()
        }
        _ => return Err(MockError::bad("BadRequest", "Unknown place type")),
    };
    page_list(req, origin, value)
}

fn busy_items(tenant: &Tenant, address: &str, from: &str, to: &str) -> Option<Vec<Value>> {
    let items = tenant.schedules.get(&address.to_ascii_lowercase())?;
    Some(
        items
            .iter()
            .filter(|i| {
                let start = i["start"]["dateTime"].as_str().unwrap_or_default();
                let end = i["end"]["dateTime"].as_str().unwrap_or_default();
                start.get(..19).unwrap_or(start) < to && end.get(..19).unwrap_or(end) > from
            })
            .cloned()
            .collect(),
    )
}

fn get_schedule(tenant: &Tenant, req: &Req) -> MockResult<Reply> {
    let body = req.json()?;
    let schedules = body["schedules"].as_array().cloned().unwrap_or_default();
    if schedules.is_empty() || schedules.len() > 100 {
        return Err(MockError::bad("ErrorInvalidParameter", "schedules"));
    }
    let from = body
        .pointer("/startTime/dateTime")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let to = body
        .pointer("/endTime/dateTime")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let value: Vec<Value> = schedules
        .iter()
        .filter_map(Value::as_str)
        .map(|address| match busy_items(tenant, address, from, to) {
            Some(items) => json!({"scheduleId": address, "availabilityView": "", "scheduleItems": items}),
            None => json!({"scheduleId": address, "availabilityView": "", "scheduleItems": [],
                "error": {"message": "Failed to resolve the recipient.", "responseCode": "ErrorMailRecipientNotFound"}}),
        })
        .collect();
    Ok(Reply::ok(&json!({ "value": value })))
}

fn find_meeting_times(tenant: &Tenant, req: &Req) -> MockResult<Reply> {
    let body = req.json()?;
    let slot = body
        .pointer("/timeConstraint/timeSlots/0")
        .cloned()
        .unwrap_or(Value::Null);
    let parse = |v: &Value| {
        v["dateTime"].as_str().and_then(|s| {
            chrono::NaiveDateTime::parse_from_str(s.get(..19).unwrap_or(s), "%Y-%m-%dT%H:%M:%S")
                .ok()
        })
    };
    let (Some(mut start), Some(end)) = (parse(&slot["start"]), parse(&slot["end"])) else {
        return Err(MockError::bad("ErrorInvalidParameter", "timeConstraint"));
    };
    let minutes: i64 = body["meetingDuration"]
        .as_str()
        .and_then(|d| d.strip_prefix("PT")?.strip_suffix('M')?.parse().ok())
        .unwrap_or(30);
    let max = usize::try_from(body["maxCandidates"].as_u64().unwrap_or(5)).unwrap_or(5);
    let attendees: Vec<(String, String)> = body["attendees"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|a| {
            Some((
                a.pointer("/emailAddress/address")?
                    .as_str()?
                    .to_ascii_lowercase(),
                a["type"].as_str().unwrap_or("required").to_owned(),
            ))
        })
        .collect();
    let mut suggestions = Vec::new();
    while start + chrono::Duration::minutes(minutes) <= end && suggestions.len() < max {
        let slot_end = start + chrono::Duration::minutes(minutes);
        let (s, e) = (
            start.format("%Y-%m-%dT%H:%M:%S").to_string(),
            slot_end.format("%Y-%m-%dT%H:%M:%S").to_string(),
        );
        let free = attendees.iter().all(|(a, _)| {
            busy_items(tenant, a, &s, &e)
                .is_none_or(|items| items.iter().all(|i| i["status"] == "free"))
        });
        if free {
            let rooms: Vec<Value> = attendees
                .iter()
                .filter(|(_, t)| t == "resource")
                .map(|(a, _)| json!({"displayName": a, "locationEmailAddress": a}))
                .collect();
            suggestions.push(json!({
                "confidence": 100.0,
                "organizerAvailability": "free",
                "suggestionReason": "Suggested because it is one of the nearest times when all attendees are available.",
                "meetingTimeSlot": {"start": {"dateTime": format!("{s}.0000000"), "timeZone": "UTC"},
                                    "end": {"dateTime": format!("{e}.0000000"), "timeZone": "UTC"}},
                "attendeeAvailability": attendees.iter().map(|(a, t)| json!({"attendee": {"type": t, "emailAddress": {"address": a}}, "availability": "free"})).collect::<Vec<_>>(),
                "locations": rooms,
            }));
        }
        start = slot_end;
    }
    let reason = if suggestions.is_empty() {
        "AttendeesUnavailable"
    } else {
        ""
    };
    Ok(Reply::ok(
        &json!({"emptySuggestionsReason": reason, "meetingTimeSuggestions": suggestions}),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_decode() {
        let req = Req::new(
            "get",
            "/v1.0/users/support%40contoso.example/messages/AAMk%2Da%3D",
            "%24top=2",
            vec![],
            vec![],
        );
        assert_eq!(
            req.segments,
            ["users", "support@contoso.example", "messages", "AAMk-a="]
        );
        assert_eq!(req.query("$top"), Some("2"));
        assert_eq!(req.method, "GET");
        assert_eq!(percent_decode("a%2"), "a%2");
        assert_eq!(percent_decode("%41b"), "Ab");
    }

    #[test]
    fn occurrences_of_patterns() {
        let weekly = json!({"start": {"dateTime": "2026-10-05T09:00:00", "timeZone": "UTC"},
            "end": {"dateTime": "2026-10-05T09:30:00", "timeZone": "UTC"},
            "recurrence": {"pattern": {"type": "weekly", "interval": 1, "daysOfWeek": ["monday"]},
                           "range": {"type": "numbered", "startDate": "2026-10-05", "numberOfOccurrences": 3}}});
        let dates: Vec<String> =
            occurrence_dates(&weekly, "2026-10-01T00:00:00Z", "2027-01-01T00:00:00Z")
                .iter()
                .map(|d| d.to_string())
                .collect();
        assert_eq!(dates, ["2026-10-05", "2026-10-12", "2026-10-19"]);
        let o = occurrence("M", &weekly, "2026-10-12");
        assert_eq!(o["originalStart"], "2026-10-12T09:00:00Z");
        assert_eq!(o["start"]["dateTime"], "2026-10-12T09:00:00.0000000");
        assert!(o.get("recurrence").is_none());
        let daily = json!({"recurrence": {"pattern": {"type": "daily", "interval": 2},
            "range": {"type": "endDate", "startDate": "2026-10-01", "endDate": "2026-10-06"}}});
        assert_eq!(
            occurrence_dates(&daily, "2026-10-01", "2026-12-01").len(),
            3
        );
        assert!(occurrence_dates(&json!({}), "2026-10-01", "2026-12-01").is_empty());
    }

    #[test]
    fn attachments_of_multipart_messages() {
        let raw = b"Content-Type: multipart/mixed; boundary=\"b1\"\r\n\r\n--b1\r\nContent-Type: text/plain\r\n\r\nHi\r\n--b1\r\nContent-Type: application/pdf; name=x.pdf\r\nContent-Disposition: attachment; filename=\"x.pdf\"\r\nContent-Transfer-Encoding: base64\r\n\r\nJVBERi0x\r\n--b1--\r\n";
        let found = attachments(raw);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].0["name"], "x.pdf");
        assert_eq!(found[0].1, b"%PDF-1");
        assert!(attachments(b"Subject: x\r\n\r\nplain").is_empty());
    }
}
