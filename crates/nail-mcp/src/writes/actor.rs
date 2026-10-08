//! Writes of actors that are not MCP clients — plug-ins (M4a WP105,
//! ADR 0084): the same plans, policies, confirmations and Client API
//! requests as the MCP write tools, so agent and plug-in writes share one
//! implementation.
//!
//! The caller (the core) has checked the actor's permissions already and
//! passes the accounts the write may reach. There is no idempotency key
//! or elicitation (those are MCP's); a write that asks becomes a pending
//! confirmation attributed to the actor, and a background task runs it
//! once approved, so an approval given after the plug-in stopped waiting
//! still runs exactly once.
//!
//! Every write leaves an entry in the MCP audit log (`plugin:<id>
//! <operation>`, content-free like the tools' entries), which the plug-in
//! cannot change or push out (review M4a, ADR 0085).
//!
//! A plug-in serving an MCP client's tool call (`on_behalf_of`) writes
//! within the overlap of both: the client's grant (scopes, accounts,
//! excluded folders) narrows the plug-in's, either one's policy asking
//! makes it ask, and the write counts against the client's
//! `writes_per_minute` / `sends_per_hour` (review M4a). A revoked client
//! reaches nothing.

use std::sync::Arc;

use nail_api::agents::{ActionActor, ActionAttribution, ConfirmationState};
use nail_api::mcp::tools::{
    CreateDraftArgs, CreateEventArgs, DeleteMessagesArgs, McpWriteResult, RespondToInviteArgs,
    SendMessageArgs, UpdateDraftArgs, UpdateEventArgs, UpdateMessagesArgs, UpsertContactArgs,
    UpsertTaskArgs,
};
use nail_api::mcp::{McpCallKind, McpClient, McpGrant, McpOutcome, McpScope};
use nail_api::{AccountId, ApiError, ErrorCode};
use nail_store::{McpAuditRecord, Store};

pub use super::pim::PimDelete;
use super::plan::Plan;
use super::{
    Ctx, WriteOutcome, WriteSettings, audit_detail, audit_outcome, mail, pim, with_confirmation,
};
use crate::access::Access;
use crate::data::McpData;
use crate::limits::Window;
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
    /// Whether it may delete (Trash, Junk, permanent). `false` for the
    /// actions of arrival hooks, which are never a delete: such a write is
    /// refused (`forbidden`).
    pub allow_destructive: bool,
}

/// Plans and runs actor writes (see the module docs).
pub struct ActorWrites<D> {
    shared: Arc<Shared<D>>,
}

impl<D> std::fmt::Debug for ActorWrites<D> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ActorWrites").finish_non_exhaustive()
    }
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

/// The audit name of an actor's write: `plugin:<id> <operation>`.
pub fn actor_audit_name(by: &ActionAttribution, operation: &str) -> String {
    format!("{}:{} {operation}", by.actor.kind_str(), by.actor.id_str())
}

impl<D: McpData> ActorWrites<D> {
    /// Writes over `store` and `data` (its broker: `data.confirmations()`,
    /// else a store-backed one; its rate limiter: `data.rate_limiter()`).
    pub fn new(store: Store, data: Arc<D>, settings: WriteSettings) -> Self {
        Self {
            shared: Arc::new(Shared::new(store, data, settings)),
        }
    }

    /// Runs one write: plan, policy (destructive kinds always ask),
    /// confirmation, the Client API requests, the audit entry. Denied,
    /// expired and vetoed writes are results, not errors.
    pub async fn run(&self, request: ActorWriteRequest) -> Result<McpWriteResult, ApiError> {
        let by = request.by.clone();
        let operation = request.operation.clone();
        let client = match &by.on_behalf_of {
            Some(on_behalf) => {
                let id = on_behalf.client_id.clone();
                match self.shared.store.call(move |s| s.get_mcp_client(&id)).await {
                    Ok(client) => Some(client),
                    Err(err) => {
                        let err = ApiError::from(err);
                        self.audit(&by, None, &operation, None, Err(&err)).await;
                        return Err(err);
                    }
                }
            }
            None => None,
        };
        let (plan, result) = self.run_planned(request, client.as_ref()).await;
        self.audit(
            &by,
            client.as_ref(),
            &operation,
            plan.as_ref(),
            result.as_ref(),
        )
        .await;
        result
    }

    async fn run_planned(
        &self,
        request: ActorWriteRequest,
        client: Option<&McpClient>,
    ) -> (Option<Plan>, Result<McpWriteResult, ApiError>) {
        let grant = McpGrant {
            scopes: ACTOR_SCOPES.to_vec(),
            all_accounts: request.accounts.is_none(),
            accounts: request.accounts.clone().unwrap_or_default(),
            excluded_folders: Vec::new(),
            skip_draft_confirmation: false,
        };
        let shared = &self.shared;
        let mut access = Access::resolve(&grant, &[]);
        if let Some(client) = client {
            // A revoked client has an empty grant: nothing is left.
            access = access.intersect(&shared.access(client).await);
        }
        let plan = match self.plan(&access, &request.by.name, request.write).await {
            Ok(plan) => plan,
            Err(err) => return (None, Err(failure(err))),
        };
        if plan.kind.is_destructive() && !request.allow_destructive {
            let err = ApiError::new(
                ErrorCode::Forbidden,
                format!("{} may not delete messages", request.operation),
            );
            return (Some(plan), Err(err));
        }
        if let Some(client) = client {
            let limited = shared
                .limiter
                .check(client, Window::Writes, 1)
                .and_then(|()| {
                    if plan.sends > 0 {
                        shared.limiter.check(client, Window::Sends, plan.sends)
                    } else {
                        Ok(())
                    }
                });
            if let Err(limited) = limited {
                let err = ApiError::new(ErrorCode::RateLimited, limited.to_string());
                return (Some(plan), Err(err));
            }
        }
        let client_actor = client.map(|c| ActionActor::mcp_client(c.id.clone()));
        let mut asks = plan.kind.is_destructive();
        for actor in std::iter::once(&request.by.actor).chain(client_actor.as_ref()) {
            if asks {
                break;
            }
            asks = match shared.broker.policy(actor, plan.kind).await {
                Ok(policy) => policy.asks(),
                Err(err) => {
                    tracing::warn!(%err, "cannot read an action policy; asking");
                    true
                }
            };
        }
        if !asks {
            let result = outcome(shared.run_plan(&request.by, &plan).await);
            return (Some(plan), result);
        }
        let new =
            shared.new_confirmation(&plan, request.by.clone(), request.operation.clone(), None);
        let confirmation = match shared.broker.submit(new).await {
            Ok(submitted) => submitted.confirmation().clone(),
            Err(err) => return (Some(plan), Err(err)),
        };
        let id = confirmation.id.clone();
        let task = tokio::spawn({
            let shared = Arc::clone(shared);
            let by = request.by;
            let plan = plan.clone();
            async move { confirmed(&shared, &by, &plan, &id).await }
        });
        let result = match task.await {
            Ok(result) => result,
            Err(err) => Err(ApiError::internal(format!("the write task failed: {err}"))),
        };
        (Some(plan), result)
    }

    /// The audit entry of one write, attributed to the MCP client it
    /// served (if any); content-free like the tools' entries.
    async fn audit(
        &self,
        by: &ActionAttribution,
        client: Option<&McpClient>,
        operation: &str,
        plan: Option<&Plan>,
        result: Result<&McpWriteResult, &ApiError>,
    ) {
        let (outcome, detail) = match (plan, result) {
            (Some(plan), Ok(done)) => {
                let written = WriteOutcome::Result(done.clone());
                (audit_outcome(&written), Some(audit_detail(plan, &written)))
            }
            (Some(plan), Err(err))
                if !matches!(err.code, ErrorCode::RateLimited | ErrorCode::Forbidden) =>
            {
                let failed = WriteOutcome::Failed(err.clone());
                (McpOutcome::Error, Some(audit_detail(plan, &failed)))
            }
            (_, Err(err)) if err.code == ErrorCode::Forbidden => {
                (McpOutcome::Denied, Some(err.to_string()))
            }
            (_, Err(err)) => Failure::Api(err.clone()).audit(),
            (None, Ok(_)) => (McpOutcome::Allowed, None),
        };
        let record = McpAuditRecord {
            client_id: client.map(|c| c.id.clone()),
            client_name: by.name.clone(),
            at: crate::server::now(),
            kind: McpCallKind::Tool,
            name: actor_audit_name(by, operation),
            outcome,
            detail,
        };
        if let Err(err) = self
            .shared
            .store
            .call(move |s| s.append_mcp_audit(&record))
            .await
        {
            tracing::warn!(%err, "cannot write the audit entry of an actor's write");
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
            let ran = shared.run_approved(by, plan, id).await;
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
