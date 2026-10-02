//! A fake core for the host tests: in-memory mail, storage with a small
//! quota, recorded HTTP requests, events and log lines.

#![allow(dead_code)]

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use chrono::{DateTime, NaiveDateTime, Utc};
use nail_api::mcp::tools::McpWriteResult;
use nail_api::plugins::{
    PluginGrant, PluginLogSource, PluginManifest, PluginMessage, PluginMessageBody,
    PluginPermission, PluginStorageUsage,
};
use nail_api::{
    AccountId, ApiError, DraftId, ErrorCode, Event, Flags, MessageId, PluginId, ThreadId,
};
use nail_plugins::{
    AccountScope, AttachmentData, BoxFuture, HookCallRecord, HostLimits, HostLogLine, HttpRequest,
    HttpResponse, JobRunRecord, PackageFiles, PluginCollection, PluginContact, PluginHost,
    PluginServices, PluginSpec, PluginWriteRequest, ServiceResult, StoredJobState,
};
use serde_json::{Value, json};

pub const QUOTA: u64 = 200;

#[derive(Default)]
pub struct Fake {
    pub messages: Mutex<Vec<(PluginMessage, Option<PluginMessageBody>)>>,
    pub storage: Mutex<BTreeMap<(String, String), Value>>,
    pub fetches: Mutex<Vec<HttpRequest>>,
    pub events: Mutex<Vec<Event>>,
    pub logs: Mutex<Vec<HostLogLine>>,
    pub searches: Mutex<Vec<(String, AccountScope, AccountScope, u32)>>,
    // --- M4a ---
    pub writes: Mutex<Vec<PluginWriteRequest>>,
    /// What `write` answers (default: done with one op).
    pub write_result: Mutex<Option<Result<McpWriteResult, ApiError>>>,
    pub attachments: Mutex<HashMap<(String, u32), AttachmentData>>,
    pub drafts: Mutex<HashMap<String, String>>,
    pub locale: Mutex<Option<String>>,
    pub now: Mutex<Option<DateTime<Utc>>>,
    pub job_states: Mutex<HashMap<(String, String), Option<i64>>>,
    pub job_runs: Mutex<Vec<JobRunRecord>>,
    pub hook_calls: Mutex<Vec<HookCallRecord>>,
    pub cpu: Mutex<Vec<(String, i64, u64)>>,
    /// Response body of `http_fetch` (default: a small JSON document).
    pub fetch_body: Mutex<Option<Vec<u8>>>,
}

pub fn message(id: &str, account: &str, thread: &str, encrypted: bool) -> PluginMessage {
    PluginMessage {
        id: id.into(),
        uri: format!("nail://messages/{id}"),
        account_id: account.into(),
        folder_id: "inbox".into(),
        thread_id: Some(thread.into()),
        subject: format!("Subject {id}"),
        from: None,
        to: vec![],
        cc: vec![],
        date: "2026-09-30T08:00:00Z".into(),
        flags: Flags::default(),
        tags: vec![],
        has_attachments: false,
        encrypted,
    }
}

fn body(id: &str) -> PluginMessageBody {
    PluginMessageBody {
        id: id.into(),
        text: format!("Body of {id}"),
        html: None,
        attachments: vec![],
    }
}

impl Fake {
    pub fn new() -> Arc<Self> {
        let fake = Self::default();
        *fake.messages.lock().unwrap() = vec![
            (message("m1", "work", "t1", false), Some(body("m1"))),
            (message("m2", "home", "t1", false), Some(body("m2"))),
            (message("m3", "work", "t3", true), Some(body("m3"))),
        ];
        Arc::new(fake)
    }

    fn find(&self, id: &MessageId) -> Option<(PluginMessage, Option<PluginMessageBody>)> {
        self.messages
            .lock()
            .unwrap()
            .iter()
            .find(|(m, _)| &m.id == id)
            .cloned()
    }

    fn usage(&self, plugin: &str) -> PluginStorageUsage {
        let storage = self.storage.lock().unwrap();
        let mut usage = PluginStorageUsage {
            quota_bytes: QUOTA,
            ..PluginStorageUsage::default()
        };
        for ((p, k), v) in storage.iter() {
            if p == plugin {
                usage.keys += 1;
                usage.used_bytes += (k.len() + v.to_string().len()) as u64;
            }
        }
        usage
    }

    pub fn events(&self) -> Vec<Event> {
        self.events.lock().unwrap().clone()
    }

    pub fn log_messages(&self, plugin: &str) -> Vec<String> {
        self.logs
            .lock()
            .unwrap()
            .iter()
            .filter(|l| l.plugin_id.as_str() == plugin)
            .map(|l| l.message.clone())
            .collect()
    }

    pub fn log_lines(&self, plugin: &str) -> Vec<HostLogLine> {
        self.logs
            .lock()
            .unwrap()
            .iter()
            .filter(|l| l.plugin_id.as_str() == plugin)
            .cloned()
            .collect()
    }

    pub fn writes(&self) -> Vec<PluginWriteRequest> {
        self.writes.lock().unwrap().clone()
    }

    pub fn hook_calls(&self, plugin: &str) -> Vec<HookCallRecord> {
        self.hook_calls
            .lock()
            .unwrap()
            .iter()
            .filter(|c| c.plugin_id.as_str() == plugin)
            .cloned()
            .collect()
    }

    pub fn job_runs(&self, plugin: &str) -> Vec<JobRunRecord> {
        self.job_runs
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r.plugin_id.as_str() == plugin)
            .cloned()
            .collect()
    }

    pub fn set_now(&self, at: &str) {
        *self.now.lock().unwrap() = Some(DateTime::parse_from_rfc3339(at).unwrap().to_utc());
    }

    pub fn add_attachment(&self, message: &str, index: u32, name: &str, mime: &str, bytes: &[u8]) {
        self.attachments.lock().unwrap().insert(
            (message.to_owned(), index),
            AttachmentData {
                name: name.into(),
                mime_type: mime.into(),
                size: bytes.len() as u64,
                bytes: bytes.to_vec(),
            },
        );
    }

    /// Host log lines with this Fluent id.
    pub fn localized(&self, plugin: &str, id: &str) -> Vec<HostLogLine> {
        self.log_lines(plugin)
            .into_iter()
            .filter(|l| l.localized.as_ref().is_some_and(|m| m.id == id))
            .collect()
    }

    /// `nail.log` lines of a plug-in starting with `prefix` (the prefix
    /// removed).
    pub fn results(&self, plugin: &str, prefix: &str) -> Vec<String> {
        self.log_lines(plugin)
            .into_iter()
            .filter(|l| l.source == PluginLogSource::Plugin)
            .filter_map(|l| l.message.strip_prefix(prefix).map(str::to_owned))
            .collect()
    }
}

fn ready<T: Send + 'static>(value: T) -> BoxFuture<'static, T> {
    Box::pin(async move { value })
}

impl PluginServices for Fake {
    fn expand_accounts(&self, accounts: &[AccountId]) -> Vec<AccountId> {
        let mut out = Vec::new();
        for a in accounts {
            match a.as_str() {
                "everything" => {
                    out.push("work".into());
                    out.push("home".into());
                }
                "work" | "home" => out.push(a.clone()),
                _ => {}
            }
        }
        out
    }

    fn mail_get(&self, id: MessageId) -> BoxFuture<'static, ServiceResult<Option<PluginMessage>>> {
        ready(Ok(self.find(&id).map(|(m, _)| m)))
    }

    fn mail_body(
        &self,
        id: MessageId,
    ) -> BoxFuture<'static, ServiceResult<Option<PluginMessageBody>>> {
        ready(Ok(self.find(&id).and_then(|(_, b)| b)))
    }

    fn mail_search(
        &self,
        query: String,
        scope: AccountScope,
        body_scope: AccountScope,
        limit: u32,
    ) -> BoxFuture<'static, ServiceResult<Vec<PluginMessage>>> {
        self.searches
            .lock()
            .unwrap()
            .push((query.clone(), scope, body_scope, limit));
        // Deliberately ignores the scope: the gate filters again.
        let found = self
            .messages
            .lock()
            .unwrap()
            .iter()
            .filter(|(m, _)| m.subject.contains(&query))
            .map(|(m, _)| m.clone())
            .collect();
        ready(Ok(found))
    }

    fn mail_thread(
        &self,
        thread: ThreadId,
    ) -> BoxFuture<'static, ServiceResult<Vec<PluginMessage>>> {
        let found = self
            .messages
            .lock()
            .unwrap()
            .iter()
            .filter(|(m, _)| m.thread_id.as_ref() == Some(&thread))
            .map(|(m, _)| m.clone())
            .collect();
        ready(Ok(found))
    }

    fn storage_get(
        &self,
        plugin: PluginId,
        key: String,
    ) -> BoxFuture<'static, ServiceResult<Option<Value>>> {
        let value = self
            .storage
            .lock()
            .unwrap()
            .get(&(plugin.to_string(), key))
            .cloned();
        ready(Ok(value))
    }

    fn storage_set(
        &self,
        plugin: PluginId,
        key: String,
        value: Value,
    ) -> BoxFuture<'static, ServiceResult<()>> {
        let old = self.usage(plugin.as_str()).used_bytes;
        let previous = self
            .storage
            .lock()
            .unwrap()
            .get(&(plugin.to_string(), key.clone()))
            .map(|v| (key.len() + v.to_string().len()) as u64)
            .unwrap_or(0);
        let new = old - previous + (key.len() + value.to_string().len()) as u64;
        if new > QUOTA {
            return ready(Err(ApiError::new(ErrorCode::TooLarge, "quota exceeded")));
        }
        self.storage
            .lock()
            .unwrap()
            .insert((plugin.to_string(), key), value);
        ready(Ok(()))
    }

    fn storage_delete(
        &self,
        plugin: PluginId,
        key: String,
    ) -> BoxFuture<'static, ServiceResult<bool>> {
        let removed = self
            .storage
            .lock()
            .unwrap()
            .remove(&(plugin.to_string(), key))
            .is_some();
        ready(Ok(removed))
    }

    fn storage_keys(
        &self,
        plugin: PluginId,
        prefix: Option<String>,
    ) -> BoxFuture<'static, ServiceResult<Vec<String>>> {
        let keys = self
            .storage
            .lock()
            .unwrap()
            .keys()
            .filter(|(p, k)| {
                p == plugin.as_str() && prefix.as_deref().is_none_or(|x| k.starts_with(x))
            })
            .map(|(_, k)| k.clone())
            .collect();
        ready(Ok(keys))
    }

    fn storage_clear(&self, plugin: PluginId) -> BoxFuture<'static, ServiceResult<()>> {
        self.storage
            .lock()
            .unwrap()
            .retain(|(p, _), _| p != plugin.as_str());
        ready(Ok(()))
    }

    fn storage_usage(
        &self,
        plugin: PluginId,
    ) -> BoxFuture<'static, ServiceResult<PluginStorageUsage>> {
        ready(Ok(self.usage(plugin.as_str())))
    }

    fn http_fetch(
        &self,
        _plugin: PluginId,
        request: HttpRequest,
    ) -> BoxFuture<'static, ServiceResult<HttpResponse>> {
        self.fetches.lock().unwrap().push(request);
        let body = self
            .fetch_body
            .lock()
            .unwrap()
            .clone()
            .unwrap_or_else(|| br#"{"hello":"world"}"#.to_vec());
        ready(Ok(HttpResponse {
            status: 200,
            headers: vec![
                ("Content-Type".into(), "application/json".into()),
                ("x-multi".into(), "a".into()),
                ("x-multi".into(), "b".into()),
            ],
            body,
        }))
    }

    fn emit(&self, event: Event) {
        self.events.lock().unwrap().push(event);
    }

    fn log(&self, line: HostLogLine) {
        self.logs.lock().unwrap().push(line);
    }

    fn write(
        &self,
        request: PluginWriteRequest,
    ) -> BoxFuture<'static, ServiceResult<McpWriteResult>> {
        self.writes.lock().unwrap().push(request);
        let result =
            self.write_result
                .lock()
                .unwrap()
                .clone()
                .unwrap_or(Ok(McpWriteResult::Done {
                    op_ids: vec!["op-1".into()],
                    created: vec!["new-1".into()],
                    confirmation_id: None,
                }));
        ready(result)
    }

    fn calendars(&self) -> BoxFuture<'static, ServiceResult<Vec<PluginCollection>>> {
        ready(Ok(vec![collection("cal-1", "Work")]))
    }

    fn address_books(&self) -> BoxFuture<'static, ServiceResult<Vec<PluginCollection>>> {
        ready(Ok(vec![collection("book-1", "Contacts")]))
    }

    fn task_lists(&self) -> BoxFuture<'static, ServiceResult<Vec<PluginCollection>>> {
        ready(Ok(vec![collection("list-1", "Inbox")]))
    }

    fn find_contacts(
        &self,
        email: String,
        _limit: u32,
    ) -> BoxFuture<'static, ServiceResult<Vec<PluginContact>>> {
        let found = (email == "ada@example.com")
            .then(|| PluginContact {
                id: "c-1".into(),
                address_book_id: "book-1".into(),
                full_name: Some("Ada".into()),
                emails: vec![email.clone()],
                organization: None,
            })
            .into_iter()
            .collect();
        ready(Ok(found))
    }

    fn attachment(
        &self,
        message: MessageId,
        index: u32,
        max_bytes: u64,
    ) -> BoxFuture<'static, ServiceResult<Option<AttachmentData>>> {
        let found = self
            .attachments
            .lock()
            .unwrap()
            .get(&(message.to_string(), index))
            .cloned()
            .map(|mut a| {
                if a.size > max_bytes {
                    a.bytes.clear();
                }
                a
            });
        ready(Ok(found))
    }

    fn draft_mode(&self, draft: DraftId) -> BoxFuture<'static, ServiceResult<Option<String>>> {
        ready(Ok(self.drafts.lock().unwrap().get(draft.as_str()).cloned()))
    }

    /// Filters for the tests: `subject:<word>` (subject contains it),
    /// `none` (never), `fail` (an error).
    fn message_matches(
        &self,
        message: MessageId,
        query: String,
    ) -> BoxFuture<'static, ServiceResult<bool>> {
        if query == "fail" {
            return ready(Err(ApiError::invalid_request("bad query")));
        }
        let subject = self
            .find(&message)
            .map(|(m, _)| m.subject)
            .unwrap_or_default();
        let matched = match query.strip_prefix("subject:") {
            Some(word) => subject.contains(word),
            None => false,
        };
        ready(Ok(matched))
    }

    fn locale(&self) -> Option<String> {
        self.locale.lock().unwrap().clone()
    }

    fn now(&self) -> DateTime<Utc> {
        self.now.lock().unwrap().unwrap_or_else(Utc::now)
    }

    fn local_time(&self, at: DateTime<Utc>) -> NaiveDateTime {
        at.naive_utc()
    }

    fn job_states(
        &self,
        plugin: PluginId,
    ) -> BoxFuture<'static, ServiceResult<Vec<StoredJobState>>> {
        let states = self
            .job_states
            .lock()
            .unwrap()
            .iter()
            .filter(|((p, _), _)| p == plugin.as_str())
            .map(|((_, job), next)| StoredJobState {
                job: job.clone(),
                next_run_at: *next,
            })
            .collect();
        ready(Ok(states))
    }

    fn set_job_next_run(
        &self,
        plugin: PluginId,
        job: String,
        next_run_at: Option<i64>,
    ) -> BoxFuture<'static, ServiceResult<()>> {
        self.job_states
            .lock()
            .unwrap()
            .insert((plugin.to_string(), job), next_run_at);
        ready(Ok(()))
    }

    fn record_job_run(&self, record: JobRunRecord) -> BoxFuture<'static, ServiceResult<()>> {
        self.job_states.lock().unwrap().insert(
            (record.plugin_id.to_string(), record.job.clone()),
            record.next_run_at,
        );
        self.job_runs.lock().unwrap().push(record);
        ready(Ok(()))
    }

    fn record_hook_call(&self, record: HookCallRecord) -> BoxFuture<'static, ServiceResult<()>> {
        self.hook_calls.lock().unwrap().push(record);
        ready(Ok(()))
    }

    fn record_cpu(
        &self,
        plugin: PluginId,
        at: i64,
        cpu_ms: u64,
    ) -> BoxFuture<'static, ServiceResult<()>> {
        self.cpu
            .lock()
            .unwrap()
            .push((plugin.to_string(), at, cpu_ms));
        ready(Ok(()))
    }
}

fn collection(id: &str, name: &str) -> PluginCollection {
    PluginCollection {
        id: id.into(),
        name: name.into(),
        default: true,
        read_only: false,
    }
}

/// A manifest like [`manifest`] with more contributions merged in.
pub fn manifest_with(id: &str, permissions: &[&str], extra: Value) -> PluginManifest {
    let mut value = serde_json::to_value(manifest(id, permissions)).unwrap();
    if let Value::Object(map) = extra {
        for (key, v) in map {
            value["contributes"][key] = v;
        }
    }
    PluginManifest::parse(value.to_string().as_bytes())
        .unwrap_or_else(|issues| panic!("{issues:?}"))
}

/// A plug-in with extra contributions, every permission granted.
pub fn spec_with(id: &str, permissions: &[&str], extra: Value, js: &str) -> PluginSpec {
    let manifest = manifest_with(id, permissions, extra);
    let mut map = BTreeMap::new();
    map.insert("index.js".to_owned(), js.as_bytes().to_vec());
    map.insert("assets/logo.png".into(), vec![0x89, b'P', b'N', b'G']);
    let grants = all_grants(&manifest);
    PluginSpec {
        manifest,
        files: PackageFiles::from_map(map),
        grants,
        settings: BTreeMap::new(),
        dev_folder: None,
    }
}

pub fn limits() -> HostLimits {
    HostLimits {
        memory_bytes: 16 * 1024 * 1024,
        activation_budget: Duration::from_secs(2),
        call_budget: Duration::from_millis(500),
        activation_timeout: Duration::from_secs(5),
        reply_timeout: Duration::from_secs(5),
        ..HostLimits::default()
    }
}

pub fn host(fake: &Arc<Fake>) -> PluginHost {
    host_with(fake, limits())
}

pub fn host_with(fake: &Arc<Fake>, limits: HostLimits) -> PluginHost {
    let services: Arc<dyn PluginServices> = fake.clone();
    PluginHost::new(services, tokio::runtime::Handle::current(), limits)
}

pub fn manifest(id: &str, permissions: &[&str]) -> PluginManifest {
    let json = json!({
        "id": id,
        "name": format!("Test {id}"),
        "version": "1.0.0",
        "apiVersion": "1",
        "main": "index.js",
        "permissions": permissions,
        "contributes": {
            "commands": [
                {"id": "run", "title": "Run", "contexts": ["list", "reader"]},
                {"id": "other", "title": "Other"}
            ],
            "panels": [{"id": "side", "title": "Side"}],
            "settings": [
                {"type": "number", "key": "days", "title": "Days", "default": 1, "min": 0},
                {"type": "string", "key": "name", "title": "Name"}
            ]
        }
    });
    PluginManifest::parse(json.to_string().as_bytes()).unwrap()
}

pub fn all_grants(manifest: &PluginManifest) -> Vec<PluginGrant> {
    manifest
        .permissions
        .iter()
        .cloned()
        .map(PluginGrant::all)
        .collect()
}

pub fn grant(permission: &str, accounts: Option<&[&str]>) -> PluginGrant {
    PluginGrant {
        permission: PluginPermission::parse(permission).unwrap(),
        accounts: accounts.map(|a| a.iter().map(|x| AccountId::from(*x)).collect()),
    }
}

/// A plug-in `id` with `permissions` (all granted) and `index.js` = `js`.
pub fn spec(id: &str, permissions: &[&str], js: &str) -> PluginSpec {
    spec_with_files(id, permissions, &[("index.js", js)])
}

pub fn spec_with_files(id: &str, permissions: &[&str], files: &[(&str, &str)]) -> PluginSpec {
    let manifest = manifest(id, permissions);
    let mut map: BTreeMap<String, Vec<u8>> = files
        .iter()
        .map(|(p, c)| ((*p).to_owned(), c.as_bytes().to_vec()))
        .collect();
    map.insert("assets/logo.png".into(), vec![0x89, b'P', b'N', b'G']);
    let grants = all_grants(&manifest);
    PluginSpec {
        manifest,
        files: PackageFiles::from_map(map),
        grants,
        settings: BTreeMap::new(),
        dev_folder: None,
    }
}

/// Polls `check` until it returns `Some` (10 s at most).
pub async fn wait_for<T>(what: &str, mut check: impl FnMut() -> Option<T>) -> T {
    let started = Instant::now();
    loop {
        if let Some(value) = check() {
            return value;
        }
        if started.elapsed() > Duration::from_secs(10) {
            panic!("timed out waiting for {what}");
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// Waits for the first `nail.log` line starting with `prefix`.
pub async fn result(fake: &Fake, plugin: &str, prefix: &str) -> String {
    wait_for(&format!("{plugin}: {prefix}"), || {
        fake.results(plugin, prefix).into_iter().next()
    })
    .await
}

/// Waits for the first result and parses it as JSON.
pub async fn json_result(fake: &Fake, plugin: &str, prefix: &str) -> Value {
    let text = result(fake, plugin, prefix).await;
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{text}: {e}"))
}

pub fn plugin_id(id: &str) -> PluginId {
    PluginId::new(id)
}

pub fn stop_reasons(fake: &Fake, plugin: &str) -> Vec<nail_api::plugins::PluginStopReason> {
    fake.events()
        .into_iter()
        .filter_map(|e| match e {
            Event::PluginStopped {
                plugin_id, reason, ..
            } if plugin_id.as_str() == plugin => Some(reason),
            _ => None,
        })
        .collect()
}

pub fn map(value: Value) -> HashMap<String, Value> {
    serde_json::from_value(value).unwrap()
}
