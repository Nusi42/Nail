//! The write tools (M4a WP101, ADR 0079; design.md, "MCP server" →
//! Tools, "Permissions and safety").
//!
//! Every write call goes through the same steps:
//!
//! 1. **Idempotency.** With an `idempotency_key` (per client, 24 hours) a
//!    repeated call returns the first call's result instead of acting
//!    again; calls with the same key run one after the other.
//! 2. **Rate limits.** The call counts against `writes_per_minute`, a
//!    message it sends against `sends_per_hour` ([`crate::limits`]).
//! 3. **Plan.** The grant is checked (the tool's scopes, the account and
//!    folder of every item; [`plan`]), the arguments validated and every
//!    item resolved. Nothing has changed yet.
//! 4. **Policy.** Destructive kinds (Trash, permanent delete) always ask;
//!    the others follow the client's action policy
//!    ([`crate::broker::ConfirmationBroker::policy`]).
//! 5. **Confirmation.** A write that asks becomes a pending confirmation
//!    the user sees in Nail. A background task waits for the decision and
//!    runs the action when it is approved — so an approval never gets lost
//!    when the client goes away — while the call waits for that task.
//!    Clients that support elicitation are also asked in their own form:
//!    in-band for sessions (protocol ≤ 2025-11-25), as an MRTR
//!    `input_required` result for stateless 2026-07-28 clients. An
//!    elicitation answer may deny anything but approve only kinds that can
//!    be always-allowed.
//! 6. **Run** through [`crate::data::McpData::write`] (the core's
//!    operation queue), then the result ([`McpWriteResult`]) is remembered
//!    under the idempotency key and one audit entry with the content-free
//!    before / after lines is written.

pub(crate) mod actor;
pub(crate) mod mail;
pub(crate) mod pim;
pub(crate) mod plan;
pub(crate) mod rules;

use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use nail_api::agents::{
    ActionActor, ActionAttribution, ActionKind, Confirmation, ConfirmationDecision,
    ConfirmationSource, ConfirmationState, NewConfirmation,
};
use nail_api::mcp::tools::{IDEMPOTENCY_TTL_SECS, McpToolSpec, McpWriteResult, is_idempotency_key};
use nail_api::mcp::{McpCallKind, McpClient, McpOutcome};
use nail_api::{ApiError, ConfirmationId, ErrorCode, LocalizedMessage, McpClientId};
use rmcp::ErrorData;
use rmcp::model::{
    BooleanSchema, CallToolRequestParams, CallToolResponse, CallToolResult, ElicitRequest,
    ElicitRequestParams, ElicitResult, ElicitationAction, ElicitationSchema, InputRequest,
    InputRequiredResult, PrimitiveSchemaDefinition,
};
use rmcp::service::{RequestContext, RoleServer};
use serde_json::{Value, json};
use tokio::sync::watch;

use crate::broker::Submitted;
use crate::data::McpData;
use crate::server::{Failure, Shared, now};

use self::plan::{AuditChange, Plan};

/// How writes wait for the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WriteSettings {
    /// How long a confirmation waits for a decision (seconds; capped at
    /// [`nail_api::agents::MAX_CONFIRMATION_TIMEOUT_SECS`]).
    pub confirmation_timeout_secs: u64,
    /// How often the store-backed broker looks for a decision made
    /// elsewhere (decisions forwarded as events wake it at once).
    pub poll_interval: Duration,
}

impl Default for WriteSettings {
    fn default() -> Self {
        Self {
            confirmation_timeout_secs: nail_api::agents::CONFIRMATION_TIMEOUT_SECS,
            poll_interval: Duration::from_millis(500),
        }
    }
}

/// How a write ended.
#[derive(Debug, Clone)]
pub(crate) enum WriteOutcome {
    /// A result the agent gets (and the idempotency key remembers).
    Result(McpWriteResult),
    /// Running it failed.
    Failed(ApiError),
}

/// A stateless client's pending elicitation round (MRTR).
struct MrtrEntry {
    client: McpClientId,
    tool: String,
    confirmation: ConfirmationId,
    kind: ActionKind,
    outcome: watch::Receiver<Option<WriteOutcome>>,
    created: Instant,
}

/// One lock per (client, idempotency key).
type KeyLocks = HashMap<(McpClientId, String), Arc<tokio::sync::Mutex<()>>>;

/// Writes in flight.
#[derive(Default)]
pub(crate) struct WriteState {
    /// The outcome of each confirmation being waited for.
    waiting: Mutex<HashMap<ConfirmationId, watch::Receiver<Option<WriteOutcome>>>>,
    /// MRTR request states.
    mrtr: Mutex<HashMap<String, MrtrEntry>>,
    /// One lock per (client, idempotency key).
    keys: Mutex<KeyLocks>,
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl WriteState {
    /// Whether a task of this server waits for the confirmation.
    fn is_waiting(&self, id: &ConfirmationId) -> bool {
        lock(&self.waiting)
            .get(id)
            .is_some_and(|rx| rx.has_changed().is_ok())
    }

    async fn lock_key(&self, client: &McpClientId, key: &str) -> tokio::sync::OwnedMutexGuard<()> {
        let mutex = {
            let mut keys = lock(&self.keys);
            // Forget locks nobody holds or waits for.
            keys.retain(|_, m| Arc::strong_count(m) > 1);
            keys.entry((client.clone(), key.to_owned()))
                .or_default()
                .clone()
        };
        mutex.lock_owned().await
    }
}

/// The key of the elicitation in an MRTR round.
const MRTR_KEY: &str = "nail_confirmation";

/// Who acts: the client.
pub(crate) fn attribution(client: &McpClient) -> ActionAttribution {
    ActionAttribution::new(
        ActionActor::mcp_client(client.id.clone()),
        client.name.clone(),
    )
}

/// The agent-facing result of an outcome.
fn tool_result(outcome: &WriteOutcome) -> CallToolResult {
    match outcome {
        WriteOutcome::Result(result) => {
            let mut value = serde_json::to_value(result).unwrap_or(Value::Null);
            match result {
                McpWriteResult::Done { .. } => CallToolResult::structured(value),
                McpWriteResult::Vetoed { .. } => {
                    value["hint"] = json!(
                        "A plug-in's before-send check stopped the message; the draft is unchanged. \
                         The user can review it and send it from Nail."
                    );
                    CallToolResult::structured_error(value)
                }
                McpWriteResult::Denied { .. } => {
                    value["hint"] = json!("The user denied this action in Nail; nothing changed.");
                    CallToolResult::structured_error(value)
                }
                McpWriteResult::Expired { .. } => {
                    value["hint"] = json!(
                        "Nobody confirmed this action in time; nothing changed. Ask the user and \
                         try again with a new idempotency_key."
                    );
                    CallToolResult::structured_error(value)
                }
            }
        }
        WriteOutcome::Failed(err) => failure_result(&Failure::Api(err.clone())),
    }
}

/// The agent-facing result of a refused or failed call.
pub(crate) fn failure_result(failure: &Failure) -> CallToolResult {
    match failure {
        Failure::Denied(denial) => CallToolResult::structured_error(json!({
            "error": "denied",
            "detail": denial.to_string(),
            "hint": "The user can grant access in Nail: Settings → MCP clients.",
        })),
        Failure::Api(err) => CallToolResult::structured_error(json!({
            "error": err.code.as_str(),
            "detail": err.message,
        })),
        Failure::Limited(limited) => CallToolResult::structured_error(json!({
            "error": "rate_limited",
            "limit": limited.window.name(),
            "max": limited.max,
            "retry_after_secs": limited.retry_after_secs,
            "detail": limited.to_string(),
        })),
    }
}

/// The audit detail of a write: `write ` + JSON (targets, before / after
/// without mailbox content, what it did).
pub(crate) fn audit_detail(plan: &Plan, outcome: &WriteOutcome) -> String {
    const MAX_IDS: usize = 50;
    let ids: Vec<&str> = plan
        .targets
        .iter()
        .filter(|t| t.kind != nail_api::agents::ActionTargetKind::Recipient)
        .map(|t| t.id.as_str())
        .collect();
    let mut value = json!({
        "kind": plan.kind,
        "targets": ids.iter().take(MAX_IDS).collect::<Vec<_>>(),
        "changes": plan.audit,
    });
    if ids.len() > MAX_IDS {
        value["more_targets"] = json!(ids.len() - MAX_IDS);
    }
    match outcome {
        WriteOutcome::Result(result) => {
            if let Ok(Value::Object(fields)) = serde_json::to_value(result) {
                for (k, v) in fields {
                    value[k] = v;
                }
            }
        }
        WriteOutcome::Failed(err) => {
            value["status"] = json!("failed");
            value["error"] = json!(err.code.as_str());
        }
    }
    format!("{}{value}", crate::messages::WRITE_DETAIL_PREFIX)
}

pub(crate) fn audit_outcome(outcome: &WriteOutcome) -> McpOutcome {
    match outcome {
        WriteOutcome::Result(McpWriteResult::Done { .. }) => McpOutcome::Allowed,
        WriteOutcome::Result(_) => McpOutcome::Declined,
        WriteOutcome::Failed(_) => McpOutcome::Error,
    }
}

/// The user-facing reason a confirmed action failed.
fn failure_text(err: &ApiError) -> LocalizedMessage {
    err.localized
        .as_deref()
        .cloned()
        .unwrap_or_else(|| LocalizedMessage::for_code(err.code))
}

fn idempotency_key(request: &CallToolRequestParams) -> Option<String> {
    request
        .arguments
        .as_ref()
        .and_then(|a| a.get("idempotency_key"))
        .and_then(Value::as_str)
        .map(str::to_owned)
}

/// The `_meta` of a write result: the confirmation, if any.
fn with_confirmation(result: McpWriteResult, id: &ConfirmationId) -> McpWriteResult {
    match result {
        McpWriteResult::Done {
            op_ids, created, ..
        } => McpWriteResult::Done {
            op_ids,
            created,
            confirmation_id: Some(id.as_str().to_owned()),
        },
        other => other,
    }
}

impl<D: McpData> Shared<D> {
    /// The confirmation `by` asks for to run `plan` (`operation` = the
    /// tool or write).
    pub(crate) fn new_confirmation(
        &self,
        plan: &Plan,
        by: ActionAttribution,
        operation: String,
        idempotency_key: Option<String>,
    ) -> NewConfirmation {
        NewConfirmation {
            requested_by: by,
            kind: plan.kind,
            operation,
            summary: plan.summary.clone(),
            targets: plan.targets.clone(),
            changes: plan.changes.clone(),
            account_ids: plan.accounts.clone(),
            payload: serde_json::to_value(&plan.action).unwrap_or(Value::Null),
            idempotency_key,
            timeout_secs: self.settings.confirmation_timeout_secs,
        }
    }

    /// Runs an approved plan and records on its confirmation how it ended.
    pub(crate) async fn run_approved(
        &self,
        by: &ActionAttribution,
        plan: &Plan,
        id: &ConfirmationId,
    ) -> WriteOutcome {
        let outcome = self.run_plan(by, plan).await;
        let finished = match &outcome {
            WriteOutcome::Result(McpWriteResult::Vetoed { reasons }) => {
                Err(LocalizedMessage::new("mcp-tool-error-vetoed")
                    .arg("reasons", reasons.join("; ")))
            }
            WriteOutcome::Result(_) => Ok(()),
            WriteOutcome::Failed(err) => Err(failure_text(err)),
        };
        if let Err(err) = self.broker.finish(id, finished).await {
            tracing::warn!(%err, "cannot record how a confirmed write ended");
        }
        outcome
    }

    /// Runs an allowed or approved plan for `by`.
    pub(crate) async fn run_plan(&self, by: &ActionAttribution, plan: &Plan) -> WriteOutcome {
        match self.execute(by, &plan.action).await {
            Ok(done) => WriteOutcome::Result(match done.vetoed {
                Some(reasons) => McpWriteResult::Vetoed { reasons },
                None => McpWriteResult::Done {
                    op_ids: done.op_ids,
                    created: done.created,
                    confirmation_id: None,
                },
            }),
            Err(Failure::Api(err)) => WriteOutcome::Failed(err),
            Err(other) => WriteOutcome::Failed(ApiError::internal(format!("{other:?}"))),
        }
    }

    /// Remembers the result and writes the audit entry.
    async fn record(
        &self,
        client: &McpClient,
        tool: &str,
        key: Option<&str>,
        plan: &Plan,
        outcome: &WriteOutcome,
    ) {
        if let (Some(key), WriteOutcome::Result(result)) = (key, outcome) {
            let (id, key, tool) = (client.id.clone(), key.to_owned(), tool.to_owned());
            let value = serde_json::to_value(result).unwrap_or(Value::Null);
            let at = now();
            let ttl = i64::try_from(IDEMPOTENCY_TTL_SECS).unwrap_or(i64::MAX);
            if let Err(err) = self
                .store
                .call(move |s| {
                    s.store_mcp_idempotent_result(&id, &key, &tool, &value, at, at - ttl)
                })
                .await
            {
                tracing::warn!(%err, "cannot remember an MCP idempotency key");
            }
        }
        self.audit(
            Some(client),
            McpCallKind::Tool,
            tool,
            audit_outcome(outcome),
            Some(audit_detail(plan, outcome)),
        )
        .await;
    }

    /// Waits for the decision, runs the plan when approved, records the
    /// result. Runs in its own task.
    async fn confirmed(
        self: Arc<Self>,
        client: McpClient,
        tool: String,
        key: Option<String>,
        plan: Plan,
        id: ConfirmationId,
    ) -> WriteOutcome {
        let outcome = match self.broker.wait(&id).await {
            Ok(decided) => match decided.state {
                ConfirmationState::Approved => {
                    let outcome = self.run_approved(&attribution(&client), &plan, &id).await;
                    match outcome {
                        WriteOutcome::Result(result) => {
                            WriteOutcome::Result(with_confirmation(result, &id))
                        }
                        failed @ WriteOutcome::Failed(_) => failed,
                    }
                }
                ConfirmationState::Denied => WriteOutcome::Result(McpWriteResult::Denied {
                    confirmation_id: id.as_str().to_owned(),
                }),
                ConfirmationState::Expired | ConfirmationState::Cancelled => {
                    WriteOutcome::Result(McpWriteResult::Expired {
                        confirmation_id: id.as_str().to_owned(),
                    })
                }
                ConfirmationState::Pending | ConfirmationState::Failed => WriteOutcome::Failed(
                    ApiError::new(ErrorCode::Conflict, "the confirmation ended unexpectedly"),
                ),
            },
            Err(err) => WriteOutcome::Failed(err),
        };
        self.record(&client, &tool, key.as_deref(), &plan, &outcome)
            .await;
        outcome
    }

    /// Starts (or joins) the task waiting for `confirmation`.
    fn wait_for(
        self: &Arc<Self>,
        client: &McpClient,
        tool: &str,
        key: Option<&str>,
        plan: Plan,
        confirmation: &ConfirmationId,
    ) -> watch::Receiver<Option<WriteOutcome>> {
        let mut waiting = lock(&self.writes.waiting);
        if let Some(rx) = waiting.get(confirmation)
            && rx.has_changed().is_ok()
        {
            return rx.clone();
        }
        let (tx, rx) = watch::channel(None);
        waiting.insert(confirmation.clone(), rx.clone());
        drop(waiting);
        let shared = self.clone();
        let (client, tool, key, id) = (
            client.clone(),
            tool.to_owned(),
            key.map(str::to_owned),
            confirmation.clone(),
        );
        tokio::spawn(async move {
            let outcome = shared
                .clone()
                .confirmed(client, tool, key, plan, id.clone())
                .await;
            drop(tx.send(Some(outcome)));
            lock(&shared.writes.waiting).remove(&id);
        });
        rx
    }

    /// Applies an elicitation answer to a pending confirmation.
    async fn elicited(&self, id: &ConfirmationId, kind: ActionKind, answer: Option<ElicitResult>) {
        let decision = match answer {
            Some(ElicitResult {
                action: ElicitationAction::Accept,
                content,
                ..
            }) => {
                let approve = content
                    .as_ref()
                    .and_then(|c| c.get("approve"))
                    .and_then(Value::as_bool)
                    .unwrap_or(true);
                match (approve, ConfirmationSource::Elicitation.may_approve(kind)) {
                    (true, true) => Some(ConfirmationDecision::Approve),
                    // Destructive actions are approved in Nail only.
                    (true, false) => None,
                    (false, _) => Some(ConfirmationDecision::Deny),
                }
            }
            Some(ElicitResult {
                action: ElicitationAction::Decline,
                ..
            }) => Some(ConfirmationDecision::Deny),
            // Dismissed or failed: Nail's own confirmation still decides.
            _ => None,
        };
        if let Some(decision) = decision
            && let Err(err) = self
                .broker
                .answer(id, decision, ConfirmationSource::Elicitation)
                .await
        {
            // Decided in Nail first, or expired.
            tracing::debug!(%err, "elicitation answer not applied");
        }
    }

    /// The elicitation form for a confirmation.
    fn elicitation(&self, plan: &Plan) -> ElicitRequestParams {
        let summary = self.data.render(&plan.summary);
        let (message, properties) = if plan.kind.can_always_allow() {
            let approve = BooleanSchema::new()
                .title(
                    self.data
                        .render(&LocalizedMessage::new("mcp-tool-elicit-approve")),
                )
                .with_default(true);
            (
                LocalizedMessage::new("mcp-tool-elicit-ask").arg("summary", summary),
                [(
                    "approve".to_owned(),
                    PrimitiveSchemaDefinition::Boolean(approve),
                )]
                .into_iter()
                .collect(),
            )
        } else {
            (
                LocalizedMessage::new("mcp-tool-elicit-destructive").arg("summary", summary),
                BTreeMap::default(),
            )
        };
        ElicitRequestParams::FormElicitationParams {
            meta: None,
            message: self.data.render(&message),
            requested_schema: ElicitationSchema::new(properties),
        }
    }

    fn mrtr_token() -> String {
        let mut bytes = [0u8; 16];
        if getrandom::fill(&mut bytes).is_err() {
            // Unique enough without the OS: time and a counter.
            static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            bytes[..8].copy_from_slice(&n.to_le_bytes());
            bytes[8..].copy_from_slice(&now().to_le_bytes());
        }
        bytes.iter().fold(String::with_capacity(32), |mut out, b| {
            // Writing to a String cannot fail.
            let _ = write!(out, "{b:02x}");
            out
        })
    }
}

async fn outcome_of(mut rx: watch::Receiver<Option<WriteOutcome>>) -> WriteOutcome {
    match rx.wait_for(Option::is_some).await {
        Ok(outcome) => outcome.clone().unwrap_or_else(|| {
            WriteOutcome::Failed(ApiError::internal("the write ended without a result"))
        }),
        Err(_) => WriteOutcome::Failed(ApiError::internal("the write ended without a result")),
    }
}

/// Whether the calling client can answer an elicitation form.
fn elicits(context: &RequestContext<RoleServer>) -> bool {
    context
        .client_capabilities()
        .and_then(|c| c.elicitation)
        .is_some_and(|e| e.form.is_some() || e.url.is_none())
}

/// Whether the call is stateless (2026-07-28 and later: elicitation goes
/// through MRTR rounds instead of an in-band request).
fn stateless(context: &RequestContext<RoleServer>) -> bool {
    context
        .protocol_version()
        .is_some_and(|v| !v.has_initialize())
}

/// Runs a write tool call for `client`.
pub(crate) async fn call<D: McpData>(
    shared: &Arc<Shared<D>>,
    client: &McpClient,
    spec: &'static McpToolSpec,
    request: &CallToolRequestParams,
    context: &RequestContext<RoleServer>,
) -> Result<CallToolResponse, ErrorData> {
    let tool = spec.name;
    if let Some(state) = &request.request_state {
        return continue_round(shared, client, tool, state, request).await;
    }
    let key = idempotency_key(request);
    if let Some(k) = &key
        && !is_idempotency_key(k)
    {
        let err = ErrorData::invalid_params(
            "invalid idempotency_key: 1 to 128 printable ASCII characters without spaces",
            None,
        );
        shared
            .audit(
                Some(client),
                McpCallKind::Tool,
                tool,
                McpOutcome::Error,
                Some(err.message.to_string()),
            )
            .await;
        return Err(err);
    }
    let serial = match &key {
        Some(k) => Some(shared.writes.lock_key(&client.id, k).await),
        None => None,
    };
    if let Some(k) = &key {
        let (id, k2) = (client.id.clone(), k.clone());
        let since = now() - i64::try_from(IDEMPOTENCY_TTL_SECS).unwrap_or(i64::MAX);
        let earlier = shared
            .store
            .call(move |s| s.mcp_idempotent_result(&id, &k2, since))
            .await
            .unwrap_or_else(|err| {
                tracing::warn!(%err, "cannot read an MCP idempotency key");
                None
            });
        if let Some((earlier_tool, value)) = earlier {
            if earlier_tool != tool {
                let failure = Failure::Api(ApiError::new(
                    ErrorCode::Conflict,
                    format!("the idempotency_key was used for {earlier_tool}"),
                ));
                let result = shared
                    .audited(client, McpCallKind::Tool, tool, Err::<(), _>(failure))
                    .await;
                let failure = result.err().unwrap_or(Failure::Api(ApiError::internal("")));
                return Ok(failure_result(&failure).into());
            }
            if let Ok(result) = serde_json::from_value::<McpWriteResult>(value) {
                shared
                    .audit(
                        Some(client),
                        McpCallKind::Tool,
                        tool,
                        McpOutcome::Allowed,
                        Some(crate::messages::DETAIL_REPLAYED.to_owned()),
                    )
                    .await;
                return Ok(tool_result(&WriteOutcome::Result(result)).into());
            }
        }
    }
    let refused = |failure: Failure| async move {
        let result = shared
            .audited(client, McpCallKind::Tool, tool, Err::<(), _>(failure))
            .await;
        let failure = result.err().unwrap_or(Failure::Api(ApiError::internal("")));
        Ok::<_, ErrorData>(CallToolResponse::from(failure_result(&failure)))
    };
    if let Err(limited) = shared
        .limiter
        .check(client, crate::limits::Window::Writes, 1)
    {
        return refused(Failure::Limited(limited)).await;
    }
    let access = shared.access(client).await;
    let plan = match plan_for(shared, &access, client, tool, request).await {
        Ok(Ok(plan)) => plan,
        Ok(Err(failure)) => return refused(failure).await,
        Err(err) => {
            shared
                .audit(
                    Some(client),
                    McpCallKind::Tool,
                    tool,
                    McpOutcome::Error,
                    Some(err.message.to_string()),
                )
                .await;
            return Err(err);
        }
    };
    if plan.sends > 0
        && let Err(limited) = shared
            .limiter
            .check(client, crate::limits::Window::Sends, plan.sends)
    {
        return refused(Failure::Limited(limited)).await;
    }
    let by = attribution(client);
    // A key with a confirmation behind it goes through the confirmation
    // again, which checks that it names the same request (review M4a).
    let key_confirmed = match &key {
        Some(k) => {
            let (actor, k) = (by.actor.clone(), k.clone());
            shared
                .store
                .call(move |s| s.has_confirmation_key(&actor, &k))
                .await
                .unwrap_or(true)
        }
        None => false,
    };
    let asks = plan.kind.is_destructive()
        || key_confirmed
        || match shared.broker.policy(&by.actor, plan.kind).await {
            Ok(policy) => policy.asks(),
            Err(err) => {
                tracing::warn!(%err, "cannot read an MCP client's action policy; asking");
                true
            }
        };
    if !asks {
        let outcome = shared.run_plan(&by, &plan).await;
        shared
            .record(client, tool, key.as_deref(), &plan, &outcome)
            .await;
        return Ok(tool_result(&outcome).into());
    }
    let new = shared.new_confirmation(&plan, by, tool.to_owned(), key.clone());
    let (confirmation, existing): (Confirmation, bool) = match shared.broker.submit(new).await {
        Ok(Submitted::New(c)) => (c, false),
        Ok(Submitted::Existing(c)) => (c, true),
        Err(err) => return refused(Failure::Api(err)).await,
    };
    // The key's earlier request (the store checked it is this one) is
    // pending, but nothing in this server waits for it: an approval would
    // run nothing. Cancel it; the agent retries with a new key.
    let orphaned = existing
        && confirmation.state == ConfirmationState::Pending
        && !shared.writes.is_waiting(&confirmation.id);
    if orphaned && let Err(err) = shared.broker.cancel(&confirmation.id).await {
        tracing::warn!(%err, "cannot cancel an orphaned confirmation");
    }
    let early = match confirmation.state {
        ConfirmationState::Pending if orphaned => Some(McpWriteResult::Expired {
            confirmation_id: confirmation.id.as_str().to_owned(),
        }),
        ConfirmationState::Pending => None,
        ConfirmationState::Denied => Some(McpWriteResult::Denied {
            confirmation_id: confirmation.id.as_str().to_owned(),
        }),
        ConfirmationState::Expired | ConfirmationState::Cancelled => {
            Some(McpWriteResult::Expired {
                confirmation_id: confirmation.id.as_str().to_owned(),
            })
        }
        // Approved under this key before, but the result was not kept
        // (the server stopped meanwhile): never run it twice.
        ConfirmationState::Approved | ConfirmationState::Failed => {
            return refused(Failure::Api(ApiError::new(
                ErrorCode::Conflict,
                "this idempotency_key was confirmed before; use a new key",
            )))
            .await;
        }
    };
    if let Some(result) = early {
        let outcome = WriteOutcome::Result(result);
        shared
            .record(client, tool, key.as_deref(), &plan, &outcome)
            .await;
        return Ok(tool_result(&outcome).into());
    }
    let id = confirmation.id.clone();
    let kind = plan.kind;
    let form = shared.elicitation(&plan);
    let rx = shared.wait_for(client, tool, key.as_deref(), plan, &id);
    drop(serial);
    if elicits(context) {
        if stateless(context) {
            let token = Shared::<D>::mrtr_token();
            let mut rounds = lock(&shared.writes.mrtr);
            let max_age = Duration::from_secs(
                nail_api::agents::MAX_CONFIRMATION_TIMEOUT_SECS.saturating_add(60),
            );
            rounds.retain(|_, entry| entry.created.elapsed() < max_age);
            rounds.insert(
                token.clone(),
                MrtrEntry {
                    client: client.id.clone(),
                    tool: tool.to_owned(),
                    confirmation: id,
                    kind,
                    outcome: rx,
                    created: Instant::now(),
                },
            );
            let requests = [(
                MRTR_KEY.to_owned(),
                InputRequest::Elicitation(ElicitRequest::new(form)),
            )]
            .into_iter()
            .collect();
            return Ok(InputRequiredResult::new(Some(requests), Some(token)).into());
        }
        let ask = context.peer.create_elicitation(form);
        tokio::pin!(ask);
        let outcome = outcome_of(rx.clone());
        tokio::pin!(outcome);
        let mut asked = false;
        loop {
            tokio::select! {
                outcome = &mut outcome => return Ok(tool_result(&outcome).into()),
                answer = &mut ask, if !asked => {
                    asked = true;
                    shared.elicited(&id, kind, answer.ok()).await;
                }
            }
        }
    }
    Ok(tool_result(&outcome_of(rx).await).into())
}

/// An MRTR retry: the client answered the elicitation form.
async fn continue_round<D: McpData>(
    shared: &Arc<Shared<D>>,
    client: &McpClient,
    tool: &str,
    state: &str,
    request: &CallToolRequestParams,
) -> Result<CallToolResponse, ErrorData> {
    let entry = {
        let rounds = lock(&shared.writes.mrtr);
        rounds
            .get(state)
            .filter(|e| e.client == client.id && e.tool == tool)
            .map(|e| (e.confirmation.clone(), e.kind, e.outcome.clone()))
    };
    let Some((id, kind, rx)) = entry else {
        return Err(ErrorData::invalid_params(
            "unknown or expired request state",
            None,
        ));
    };
    let answer = request
        .input_responses
        .as_ref()
        .and_then(|r| r.get(MRTR_KEY))
        .and_then(|v| serde_json::from_value::<ElicitResult>(v.clone()).ok());
    shared.elicited(&id, kind, answer).await;
    let outcome = outcome_of(rx).await;
    lock(&shared.writes.mrtr).remove(state);
    Ok(tool_result(&outcome).into())
}

/// Builds the plan of `tool`; `Err` for malformed arguments (a protocol
/// error), `Ok(Err)` for a refusal or an invalid request.
async fn plan_for<D: McpData>(
    shared: &Shared<D>,
    access: &crate::access::Access,
    client: &McpClient,
    tool: &str,
    request: &CallToolRequestParams,
) -> Result<Result<Plan, Failure>, ErrorData> {
    let ctx = Ctx {
        shared,
        access,
        name: &client.name,
    };
    Ok(match tool {
        "update_messages" => mail::update_messages(&ctx, crate::tools::args(request)?).await,
        "delete_messages" => mail::delete_messages(&ctx, crate::tools::args(request)?).await,
        "create_draft" => mail::create_draft(&ctx, crate::tools::args(request)?).await,
        "update_draft" => mail::update_draft(&ctx, crate::tools::args(request)?).await,
        "send_message" => mail::send_message(&ctx, crate::tools::args(request)?).await,
        "edit_open_draft" => mail::edit_open_draft(&ctx, crate::tools::args(request)?).await,
        "create_event" => pim::create_event(&ctx, crate::tools::args(request)?).await,
        "update_event" => pim::update_event(&ctx, crate::tools::args(request)?).await,
        "respond_to_invite" => pim::respond_to_invite(&ctx, crate::tools::args(request)?).await,
        "upsert_contact" => pim::upsert_contact(&ctx, crate::tools::args(request)?).await,
        "upsert_task" => pim::upsert_task(&ctx, crate::tools::args(request)?).await,
        "create_rule" => rules::create_rule(&ctx, crate::tools::args(request)?).await,
        _ => {
            return Err(ErrorData::invalid_params(
                format!("unknown tool {}", crate::http::audited_path(tool)),
                None,
            ));
        }
    })
}

/// What plan builders need.
pub(crate) struct Ctx<'a, D> {
    pub shared: &'a Shared<D>,
    pub access: &'a crate::access::Access,
    /// The actor's name (`$actor` of the summaries).
    pub name: &'a str,
}

impl<D: McpData> Ctx<'_, D> {
    /// The client's name (`$actor` of the summaries).
    pub(crate) fn actor(&self) -> &str {
        self.name
    }

    /// A rendered user-facing value.
    pub(crate) fn text(&self, id: &str) -> String {
        self.shared.data.render(&LocalizedMessage::new(id))
    }

    /// "Yes" / "No" in the core's locale.
    pub(crate) fn yes_no(&self, value: bool) -> String {
        self.text(if value {
            "mcp-tool-value-yes"
        } else {
            "mcp-tool-value-no"
        })
    }

    /// A boolean of several items: yes, no or mixed.
    pub(crate) fn yes_no_mixed(&self, values: impl IntoIterator<Item = bool>) -> Option<String> {
        let values: Vec<bool> = values.into_iter().collect();
        match (values.iter().all(|v| *v), values.iter().any(|v| *v)) {
            _ if values.is_empty() => None,
            (true, _) => Some(self.yes_no(true)),
            (false, false) => Some(self.yes_no(false)),
            (false, true) => Some(self.text("mcp-tool-value-mixed")),
        }
    }
}

/// Audit value of a boolean of several items.
pub(crate) fn audit_bools(values: impl IntoIterator<Item = bool>) -> Option<String> {
    let values: Vec<bool> = values.into_iter().collect();
    if values.is_empty() {
        None
    } else if values.iter().all(|v| *v) {
        Some("true".into())
    } else if values.iter().any(|v| *v) {
        Some("mixed".into())
    } else {
        Some("false".into())
    }
}

/// Audit line helper.
pub(crate) fn audit(
    field: &'static str,
    before: Option<String>,
    after: Option<String>,
) -> AuditChange {
    AuditChange::new(field, before, after)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nail_api::agents::{ActionTargetKind, cut_text};

    fn plan() -> Plan {
        Plan {
            kind: ActionKind::Organize,
            summary: LocalizedMessage::new("confirmation-summary-organize"),
            targets: vec![
                plan::target(ActionTargetKind::Message, "m1", "Secret subject"),
                plan::target(ActionTargetKind::Recipient, "a@example.org", "Ada"),
            ],
            changes: vec![],
            accounts: vec!["work".into()],
            audit: vec![audit("flagged", Some("false".into()), Some("true".into()))],
            sends: 0,
            action: plan::Action::Requests { requests: vec![] },
        }
    }

    #[test]
    fn audit_details_carry_no_mailbox_content() {
        let detail = audit_detail(
            &plan(),
            &WriteOutcome::Result(McpWriteResult::Done {
                op_ids: vec!["op-1".into()],
                created: vec![],
                confirmation_id: Some("7".into()),
            }),
        );
        assert!(detail.starts_with(crate::messages::WRITE_DETAIL_PREFIX));
        assert!(
            !detail.contains("Secret") && !detail.contains("Ada"),
            "{detail}"
        );
        let value: Value =
            serde_json::from_str(&detail[crate::messages::WRITE_DETAIL_PREFIX.len()..]).unwrap();
        assert_eq!(value["targets"], json!(["m1"]));
        assert_eq!(value["changes"][0]["field"], "flagged");
        assert_eq!(value["status"], "done");
        assert_eq!(value["confirmation_id"], "7");
        let failed = audit_detail(
            &plan(),
            &WriteOutcome::Failed(ApiError::not_found("message m1")),
        );
        assert!(failed.contains("\"error\":\"not_found\""), "{failed}");
        assert_eq!(audit_bools([true, false]).as_deref(), Some("mixed"));
        assert_eq!(audit_bools([]), None);
        assert_eq!(cut_text("x"), "x");
    }

    #[test]
    fn results_tell_agents_what_happened() {
        let denied = tool_result(&WriteOutcome::Result(McpWriteResult::Denied {
            confirmation_id: "3".into(),
        }));
        assert_eq!(denied.is_error, Some(true));
        let value = denied.structured_content.unwrap();
        assert_eq!(value["status"], "denied");
        assert!(value["hint"].as_str().unwrap().contains("nothing changed"));
        let done = tool_result(&WriteOutcome::Result(McpWriteResult::Done {
            op_ids: vec![],
            created: vec!["d1".into()],
            confirmation_id: None,
        }));
        assert_ne!(done.is_error, Some(true));
        assert_eq!(
            with_confirmation(
                McpWriteResult::Done {
                    op_ids: vec![],
                    created: vec![],
                    confirmation_id: None
                },
                &"9".into()
            ),
            McpWriteResult::Done {
                op_ids: vec![],
                created: vec![],
                confirmation_id: Some("9".into())
            }
        );
        assert_eq!(
            audit_outcome(&WriteOutcome::Failed(ApiError::internal("x"))),
            McpOutcome::Error
        );
    }
}
