//! Writes of actors that are not MCP clients — plug-ins (M4a WP105,
//! ADR 0084): the same plans, policies, confirmations and Client API
//! requests as the MCP write tools, so agent and plug-in writes share one
//! implementation.
//!
//! The caller (the core) has checked the actor's permissions already and
//! passes the accounts the write may reach. There is no idempotency key,
//! rate limit, elicitation or MCP audit entry (those are MCP's); a write
//! that asks becomes a pending confirmation attributed to the actor, and a
//! background task runs it once approved, so an approval given after the
//! plug-in stopped waiting still runs exactly once.

use std::sync::Arc;

use nail_api::agents::{ActionAttribution, ConfirmationState, NewConfirmation};
use nail_api::mcp::tools::{
    CreateDraftArgs, CreateEventArgs, DeleteMessagesArgs, McpWriteResult, RespondToInviteArgs,
    SendMessageArgs, UpdateDraftArgs, UpdateEventArgs, UpdateMessagesArgs, UpsertContactArgs,
    UpsertTaskArgs,
};
use nail_api::mcp::{McpGrant, McpScope};
use nail_api::{AccountId, ApiError, ErrorCode, LocalizedMessage};
use nail_store::Store;
use serde_json::Value;

pub use super::pim::PimDelete;
use super::plan::Plan;
use super::{Ctx, WriteOutcome, WriteSettings, failure_text, mail, pim, with_confirmation};
use crate::access::Access;
use crate::data::McpData;
use crate::server::{Failure, Shared};

/// What an actor wants written: the MCP write tools' arguments, plus the
/// PIM deletes plug-ins have.
#[derive(Debug, Clone, PartialEq)]
pub enum ActorWrite {
    /// As `update_messages`.
    UpdateMessages(UpdateMessagesArgs),
    /// As `delete_messages`.
    DeleteMessages(DeleteMessagesArgs),
    /// As `create_draft`.
    CreateDraft(CreateDraftArgs),
    /// As `update_draft`.
    UpdateDraft(UpdateDraftArgs),
    /// As `send_message`.
    SendMessage(SendMessageArgs),
    /// As `create_event`.
    CreateEvent(CreateEventArgs),
    /// As `update_event`.
    UpdateEvent(UpdateEventArgs),
    /// As `respond_to_invite`.
    RespondToInvite(RespondToInviteArgs),
    /// As `upsert_contact`.
    UpsertContact(UpsertContactArgs),
    /// As `upsert_task`.
    UpsertTask(UpsertTaskArgs),
    /// Delete an event, contact or task.
    Delete(PimDelete),
}

/// One write of an actor.
#[derive(Debug, Clone, PartialEq)]
pub struct ActorWriteRequest {
    /// Who writes (actor, display name, the MCP client it serves).
    pub by: ActionAttribution,
    /// The API that asked (`nail.mail.update`, `hooks.arrival`), shown in
    /// the confirmation.
    pub operation: String,
    /// What to write.
    pub write: ActorWrite,
    /// The accounts it may reach (`None` = every account). Messages,
    /// folders, identities, drafts and PIM collections of other accounts
    /// are `not_found`.
    pub accounts: Option<Vec<AccountId>>,
}

/// Plans and runs actor writes (see the module docs).
pub struct ActorWrites<D> {
    shared: Arc<Shared<D>>,
}

impl<D> Clone for ActorWrites<D> {
    fn clone(&self) -> Self {
        Self {
            shared: Arc::clone(&self.shared),
        }
    }
}

/// The scopes an actor write may use once the caller checked its
/// permissions: everything but raw source, decrypted content, the UI
/// context and rules (plug-ins write no rules).
const ACTOR_SCOPES: [McpScope; 8] = [
    McpScope::Read,
    McpScope::Attachments,
    McpScope::Organize,
    McpScope::PermanentDelete,
    McpScope::Draft,
    McpScope::Send,
    McpScope::Calendar,
    McpScope::Pim,
];

fn failure(failure: Failure) -> ApiError {
    match failure {
        Failure::Api(err) => err,
        Failure::Denied(denial) => ApiError::new(ErrorCode::Forbidden, denial.to_string()),
        Failure::Limited(limited) => ApiError::new(ErrorCode::RateLimited, limited.to_string()),
    }
}

impl<D: McpData> ActorWrites<D> {
    /// Writes over `store` and `data` (its broker: `data.confirmations()`,
    /// else a store-backed one).
    pub fn new(store: Store, data: Arc<D>, settings: WriteSettings) -> Self {
        Self {
            shared: Arc::new(Shared::new(store, data, settings)),
        }
    }

    /// Runs one write: plan, policy (destructive kinds always ask),
    /// confirmation, the Client API requests. Denied, expired and vetoed
    /// writes are results, not errors.
    pub async fn run(&self, request: ActorWriteRequest) -> Result<McpWriteResult, ApiError> {
        let grant = McpGrant {
            scopes: ACTOR_SCOPES.to_vec(),
            all_accounts: request.accounts.is_none(),
            accounts: request.accounts.clone().unwrap_or_default(),
            excluded_folders: Vec::new(),
            skip_draft_confirmation: false,
        };
        let access = Access::resolve(&grant, &[]);
        let plan = self
            .plan(&access, &request.by.name, request.write)
            .await
            .map_err(failure)?;
        let shared = &self.shared;
        let asks = plan.kind.is_destructive()
            || match shared.broker.policy(&request.by.actor, plan.kind).await {
                Ok(policy) => policy.asks(),
                Err(err) => {
                    tracing::warn!(%err, "cannot read an action policy; asking");
                    true
                }
            };
        if !asks {
            return outcome(shared.run_plan(&request.by, &plan).await);
        }
        let new = NewConfirmation {
            requested_by: request.by.clone(),
            kind: plan.kind,
            operation: request.operation.clone(),
            summary: plan.summary.clone(),
            targets: plan.targets.clone(),
            changes: plan.changes.clone(),
            account_ids: plan.accounts.clone(),
            payload: serde_json::to_value(&plan.action).unwrap_or(Value::Null),
            idempotency_key: None,
            timeout_secs: shared.settings.confirmation_timeout_secs,
        };
        let confirmation = shared.broker.submit(new).await?.confirmation().clone();
        let id = confirmation.id.clone();
        let task = tokio::spawn({
            let shared = Arc::clone(shared);
            let by = request.by;
            async move { confirmed(&shared, &by, &plan, &id).await }
        });
        match task.await {
            Ok(result) => result,
            Err(err) => Err(ApiError::internal(format!("the write task failed: {err}"))),
        }
    }

    async fn plan(&self, access: &Access, name: &str, write: ActorWrite) -> Result<Plan, Failure> {
        let ctx = Ctx {
            shared: &self.shared,
            access,
            name,
        };
        match write {
            ActorWrite::UpdateMessages(a) => mail::update_messages(&ctx, a).await,
            ActorWrite::DeleteMessages(a) => mail::delete_messages(&ctx, a).await,
            ActorWrite::CreateDraft(a) => mail::create_draft(&ctx, a).await,
            ActorWrite::UpdateDraft(a) => mail::update_draft(&ctx, a).await,
            ActorWrite::SendMessage(a) => mail::send_message(&ctx, a).await,
            ActorWrite::CreateEvent(a) => pim::create_event(&ctx, a).await,
            ActorWrite::UpdateEvent(a) => pim::update_event(&ctx, a).await,
            ActorWrite::RespondToInvite(a) => pim::respond_to_invite(&ctx, a).await,
            ActorWrite::UpsertContact(a) => pim::upsert_contact(&ctx, a).await,
            ActorWrite::UpsertTask(a) => pim::upsert_task(&ctx, a).await,
            ActorWrite::Delete(d) => pim::delete_item(&ctx, d).await,
        }
    }
}

fn outcome(outcome: WriteOutcome) -> Result<McpWriteResult, ApiError> {
    match outcome {
        WriteOutcome::Result(result) => Ok(result),
        WriteOutcome::Failed(err) => Err(err),
    }
}

/// Waits for the decision and runs the plan when approved.
async fn confirmed<D: McpData>(
    shared: &Shared<D>,
    by: &ActionAttribution,
    plan: &Plan,
    id: &nail_api::ConfirmationId,
) -> Result<McpWriteResult, ApiError> {
    let decided = shared.broker.wait(id).await?;
    match decided.state {
        ConfirmationState::Approved => {
            let ran = shared.run_plan(by, plan).await;
            let finished = match &ran {
                WriteOutcome::Result(McpWriteResult::Vetoed { reasons }) => {
                    Err(LocalizedMessage::new("mcp-tool-error-vetoed")
                        .arg("reasons", reasons.join("; ")))
                }
                WriteOutcome::Result(_) => Ok(()),
                WriteOutcome::Failed(err) => Err(failure_text(err)),
            };
            if let Err(err) = shared.broker.finish(id, finished).await {
                tracing::warn!(%err, "cannot record how a confirmed write ended");
            }
            outcome(ran).map(|r| with_confirmation(r, id))
        }
        ConfirmationState::Denied => Ok(McpWriteResult::Denied {
            confirmation_id: id.as_str().to_owned(),
        }),
        ConfirmationState::Expired | ConfirmationState::Cancelled => Ok(McpWriteResult::Expired {
            confirmation_id: id.as_str().to_owned(),
        }),
        ConfirmationState::Pending | ConfirmationState::Failed => Err(ApiError::new(
            ErrorCode::Conflict,
            "the confirmation ended unexpectedly",
        )),
    }
}
