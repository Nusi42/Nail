//! Requests of wave M4a for confirmations, action policies and plug-in
//! extensions (contract ADR 0079).
//!
//! Store-backed now: listing and deciding confirmations, the action
//! policies (with the destructive-kinds check and its Fluent id), plug-in
//! activity (job state, hook statistics, CPU time). Deciding a
//! confirmation records the decision and emits
//! [`Event::ConfirmationResolved`]; waking the MCP tool call or plug-in
//! write that waits for it is WP105's (the requesters do not exist yet).
//!
//! The plug-in host requests answer as the host does before WP102 / WP105:
//! annotations are empty (no plug-in has an open hook yet), previews,
//! actions and manual job runs `unsupported`, "Send anyway" `not_found`
//! (nothing is ever vetoed yet).

use nail_api::agents::{
    ActionActor, ActionPolicyMode, ConfirmationDecision, ConfirmationSource, check_policy,
};
use nail_api::plugins::PluginActivity;
use nail_api::plugins::jobs::CPU_QUOTA_MS_PER_HOUR;
use nail_api::plugins::jobs::PluginJobStatus;
use nail_api::{ApiError, Event, LocalizedMessage, Request, Response};
use nail_triage::Clock as _;

use crate::error::Result;
use crate::service::Service;

/// Largest page of [`Request::ListConfirmations`].
const MAX_PAGE: u32 = 500;

/// Handles one of this module's requests.
pub(crate) async fn handle(service: &Service, request: Request) -> Result<Response> {
    let now = service.inner.clock.unix();
    Ok(match request {
        Request::ListConfirmations {
            filter,
            actor,
            before,
            limit,
        } => {
            let limit = limit.clamp(1, MAX_PAGE);
            let confirmations = service
                .store()
                .call(move |s| s.list_confirmations(filter, actor.as_ref(), before.as_ref(), limit))
                .await?;
            Response::Confirmations { confirmations }
        }
        Request::DecideConfirmation {
            confirmation_id,
            decision,
            always_allow,
        } => {
            let approve = decision == ConfirmationDecision::Approve;
            let id = confirmation_id.clone();
            let current = service
                .store()
                .call(move |s| s.get_confirmation(&id))
                .await?;
            if approve && always_allow {
                check_policy(current.kind, ActionPolicyMode::Allow).map_err(|issue| {
                    ApiError::invalid_request(format!(
                        "{} cannot be allowed without asking",
                        current.kind
                    ))
                    .with_localized(LocalizedMessage::new(issue.message_id()))
                })?;
            }
            let id = confirmation_id.clone();
            let decided = service
                .store()
                .call(move |s| s.decide_confirmation(&id, approve, ConfirmationSource::Nail, now))
                .await?;
            if approve && always_allow {
                let actor = decided.requested_by.actor.clone();
                let kind = decided.kind;
                let for_store = actor.clone();
                service
                    .store()
                    .call(move |s| {
                        s.set_action_policy(&for_store, kind, Some(ActionPolicyMode::Allow), now)
                    })
                    .await?;
                service.broadcast(Event::ActionPoliciesChanged { actor });
            }
            service.broadcast(Event::ConfirmationResolved {
                confirmation: Box::new(decided.clone()),
            });
            Response::ConfirmationDecided {
                confirmation: Box::new(decided),
            }
        }
        Request::ListActionPolicies { actor } => {
            ensure_actor(service, &actor).await?;
            let for_store = actor.clone();
            let policies = service
                .store()
                .call(move |s| s.action_policies(&for_store))
                .await?;
            Response::ActionPolicies { actor, policies }
        }
        Request::SetActionPolicy { actor, kind, mode } => {
            if let Some(mode) = mode {
                check_policy(kind, mode).map_err(|issue| {
                    ApiError::invalid_request(format!("{kind} cannot be allowed without asking"))
                        .with_localized(LocalizedMessage::new(issue.message_id()))
                })?;
            }
            ensure_actor(service, &actor).await?;
            let for_store = actor.clone();
            let policies = service
                .store()
                .call(move |s| s.set_action_policy(&for_store, kind, mode, now))
                .await?;
            service.broadcast(Event::ActionPoliciesChanged {
                actor: actor.clone(),
            });
            Response::ActionPolicies { actor, policies }
        }
        Request::GetPluginActivity { plugin_id } => {
            let id = plugin_id.clone();
            let (record, states, hooks, cpu) = service
                .store()
                .call(move |s| {
                    Ok((
                        s.plugin(&id)?,
                        s.plugin_job_states(&id)?,
                        s.plugin_hook_stats(&id)?,
                        s.plugin_cpu_last_hour(&id, now)?,
                    ))
                })
                .await?;
            let rfc3339 = |t: Option<i64>| {
                t.and_then(|t| chrono::DateTime::from_timestamp(t, 0))
                    .map(|t| t.format("%Y-%m-%dT%H:%M:%SZ").to_string())
            };
            let jobs = record
                .manifest
                .contributes
                .jobs
                .iter()
                .map(|def| {
                    let state = states.iter().find(|s| s.job == def.id);
                    PluginJobStatus {
                        job: def.id.clone(),
                        title: def.title.clone(),
                        schedule: def.schedule.clone(),
                        events: def.events.clone(),
                        running: false,
                        last_run_at: rfc3339(state.and_then(|s| s.last_run_at)),
                        last_outcome: state.and_then(|s| s.last_outcome),
                        last_error: state.and_then(|s| s.last_error.clone()),
                        next_run_at: rfc3339(state.and_then(|s| s.next_run_at)),
                        run_count: state.map_or(0, |s| s.run_count),
                        failure_count: state.map_or(0, |s| s.failure_count),
                    }
                })
                .collect();
            Response::PluginActivity {
                activity: Box::new(PluginActivity {
                    plugin_id,
                    jobs,
                    hooks,
                    cpu_ms_last_hour: cpu,
                    cpu_quota_ms_per_hour: CPU_QUOTA_MS_PER_HOUR,
                }),
            }
        }
        Request::GetMessagePluginAnnotations { message_id, .. } => {
            Response::MessagePluginAnnotations {
                message_id,
                banners: Vec::new(),
                badges: Vec::new(),
            }
        }
        Request::RunPluginJob { .. }
        | Request::RenderAttachmentPreview { .. }
        | Request::RunPluginAction { .. } => {
            return Err(ApiError::unsupported(
                "plug-in jobs, previews and actions are not wired yet (M4a WP105)",
            )
            .into());
        }
        Request::OverrideSendVeto { veto_id } => {
            return Err(ApiError::not_found(format!("no send veto {veto_id} is pending")).into());
        }
        other => {
            return Err(ApiError::invalid_request(format!("not an M4a request: {other:?}")).into());
        }
    })
}

/// `not_found` unless the client or plug-in exists.
async fn ensure_actor(service: &Service, actor: &ActionActor) -> Result<()> {
    let actor = actor.clone();
    service
        .store()
        .call(move |s| match &actor {
            ActionActor::McpClient { client_id } => s.get_mcp_client(client_id).map(|_| ()),
            ActionActor::Plugin { plugin_id } => s.plugin(plugin_id).map(|_| ()),
        })
        .await?;
    Ok(())
}
