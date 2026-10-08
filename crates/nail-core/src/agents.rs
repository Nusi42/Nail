//! Requests of wave M4a for confirmations, action policies and plug-in
//! extensions (contract ADR 0079; integration ADR 0084).
//!
//! * Confirmations and policies are served from the store. The core owns
//!   one [`StoreBroker`] ([`Service::confirmation_broker`]): the MCP server
//!   and plug-in writes submit and wait through it, and deciding a
//!   confirmation here wakes the waiting call at once.
//! * Plug-in writes ([`Service::plugin_write`]) run through
//!   [`nail_mcp::ActorWrites`] — the MCP write tools' plans, policies,
//!   confirmations and Client API requests (so through the operation
//!   queue), attributed to the plug-in.
//! * Before-send hooks ([`Service::send_draft_checked`]): a veto answers
//!   `SendVetoed` and is kept in memory for ten minutes ("Send anyway" =
//!   `OverrideSendVeto`); headers the hooks add are applied when the message
//!   is built, added content is written into the draft.
//! * Open hooks (annotations), previews, actions and manual job runs go to
//!   the plug-in host.
//! * Revoking an MCP client or removing a plug-in cancels its pending
//!   confirmations and removes its policies ([`Service::forget_actor`]);
//!   changing a grant or disabling a plug-in cancels its pending
//!   confirmations (they were checked under the old permissions), and so
//!   does starting Nail (nothing waits for those of an earlier run;
//!   review M4a, ADR 0085).

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use nail_api::accounts::ComposerMode;
use nail_api::agents::{
    ActionActor, ActionPolicyMode, ConfirmationDecision, ConfirmationSource, check_policy,
};
use nail_api::compose::Draft;
use nail_api::plugins::hooks::{
    PluginContentAddition, PluginContentPosition, PluginHeader, PluginOutgoingMessage,
};
use nail_api::plugins::jobs::CPU_QUOTA_MS_PER_HOUR;
use nail_api::plugins::jobs::PluginJobStatus;
use nail_api::plugins::{PluginActivity, PluginAttachment};
use nail_api::{ApiError, DraftId, Event, Request, Response, SendCrypto};
use nail_mcp::broker::{ConfirmationBroker as _, StoreBroker};
use nail_mcp::{CoreData, WriteSettings};
use nail_triage::Clock as _;

use crate::error::Result;
use crate::mcp::McpCore;
use crate::service::{Service, lock};

/// How long "Send anyway" stays possible after a veto (ADR 0079).
const VETO_TTL: Duration = Duration::from_mins(10);

/// Most vetoes kept at once (the oldest go first).
const MAX_VETOES: usize = 200;

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
                    ApiError::invalid_localized(
                        format!("{} cannot be allowed without asking", current.kind),
                        issue.message_id(),
                    )
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
            // The MCP tool call or plug-in write waiting for it looks now.
            service.confirmation_broker().wake();
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
                    ApiError::invalid_localized(
                        format!("{kind} cannot be allowed without asking"),
                        issue.message_id(),
                    )
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
            let host = service.inner.plugins.existing_host();
            // Job titles in the core's language (`%id%` resolved).
            let catalogs = service.plugin_catalogs(&record).await;
            let locale = service.inner.locale.as_deref();
            let jobs = record
                .manifest
                .contributes
                .jobs
                .iter()
                .map(|def| {
                    let state = states.iter().find(|s| s.job == def.id);
                    let running = host.is_some_and(|h| h.job_running(&plugin_id, &def.id));
                    // The host's schedule when it runs the plug-in, else the
                    // stored one.
                    let next = host
                        .and_then(|h| h.job_next_run(&plugin_id, &def.id))
                        .map(|t| t.timestamp())
                        .or(state.and_then(|s| s.next_run_at));
                    PluginJobStatus {
                        job: def.id.clone(),
                        title: def.title.as_deref().map(|t| catalogs.text(locale, t)),
                        schedule: def.schedule.clone(),
                        events: def.events.clone(),
                        running,
                        last_run_at: rfc3339(state.and_then(|s| s.last_run_at)),
                        last_outcome: state.and_then(|s| s.last_outcome),
                        last_error: state.and_then(|s| s.last_error.clone()),
                        next_run_at: rfc3339(next),
                        run_count: state.map_or(0, |s| s.run_count),
                        failure_count: state.map_or(0, |s| s.failure_count),
                    }
                })
                .collect();
            // The host reports usage about once a minute; what it measured
            // since counts too.
            let host_cpu = host.map_or(0, |h| {
                u64::try_from(h.cpu_last_hour(&plugin_id).as_millis()).unwrap_or(u64::MAX)
            });
            Response::PluginActivity {
                activity: Box::new(PluginActivity {
                    plugin_id,
                    jobs,
                    hooks,
                    cpu_ms_last_hour: cpu.max(host_cpu),
                    cpu_quota_ms_per_hour: CPU_QUOTA_MS_PER_HOUR,
                }),
            }
        }
        Request::GetMessagePluginAnnotations {
            message_id,
            client_id,
        } => {
            let (banners, badges) = match service.inner.plugins.existing_host() {
                Some(host) => {
                    let report = host.open_hooks(&message_id, client_id).await;
                    service.hooks_ran(report.runs.iter().map(|r| &r.plugin_id));
                    (report.banners, report.badges)
                }
                None => (Vec::new(), Vec::new()),
            };
            Response::MessagePluginAnnotations {
                message_id,
                banners,
                badges,
            }
        }
        Request::RunPluginJob { plugin_id, job } => {
            service.plugin_host().run_job(&plugin_id, &job)?;
            Response::Done
        }
        Request::RenderAttachmentPreview {
            message_id,
            index,
            renderer,
            client_id,
        } => {
            let preview = service
                .plugin_host()
                .render_attachment_preview(&message_id, index, &renderer, client_id)
                .await?;
            Response::AttachmentPreview {
                message_id,
                index,
                renderer,
                preview: Box::new(preview),
            }
        }
        Request::RunPluginAction {
            action,
            target,
            client_id,
            ..
        } => {
            service
                .plugin_host()
                .run_action(&action, &target, client_id)
                .await?;
            Response::Done
        }
        Request::OverrideSendVeto { veto_id } => service.override_send_veto(&veto_id).await?,
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

/// A before-send veto waiting for "Send anyway".
struct PendingVeto {
    draft_id: DraftId,
    delay_secs: Option<u32>,
    send_at: Option<String>,
    crypto: Option<SendCrypto>,
    /// The draft revision a confirmed send pinned (ADR 0086): "Send
    /// anyway" sends only that draft too.
    expected: Option<String>,
    /// What the other plug-ins' hooks added (applied on "Send anyway").
    headers: Vec<PluginHeader>,
    additions: Vec<PluginContentAddition>,
    at: Instant,
}

/// The core's M4a state: the confirmation broker, pending vetoes, headers
/// before-send hooks added per draft.
#[derive(Default)]
pub(crate) struct AgentsCtl {
    broker: OnceLock<Arc<StoreBroker>>,
    /// MCP clients' rate limits, shared by the MCP server and plug-in
    /// writes made for clients.
    limiter: Arc<nail_mcp::limits::RateLimiter>,
    vetoes: Mutex<HashMap<String, PendingVeto>>,
    headers: Mutex<HashMap<DraftId, Vec<PluginHeader>>>,
    /// When `PluginActivityChanged` was last sent for a plug-in's hooks.
    hooks_announced: Mutex<HashMap<nail_api::PluginId, Instant>>,
}

/// Plug-in writes and MCP writes share this data provider.
pub(crate) type CoreWrites = nail_mcp::ActorWrites<CoreData<McpCore>>;

impl Service {
    /// The one confirmation broker of this core (see the module docs).
    pub(crate) fn confirmation_broker(&self) -> Arc<StoreBroker> {
        Arc::clone(self.inner.agents.broker.get_or_init(|| {
            let weak = Arc::downgrade(&self.inner);
            Arc::new(StoreBroker::new(
                self.store().clone(),
                Arc::new(move |event| {
                    if let Some(inner) = weak.upgrade() {
                        Service { inner }.broadcast(event);
                    }
                }),
                WriteSettings::default().poll_interval,
            ))
        }))
    }

    /// The MCP clients' rate limiter (shared with plug-in writes for them).
    pub(crate) fn mcp_rate_limiter(&self) -> Arc<nail_mcp::limits::RateLimiter> {
        Arc::clone(&self.inner.agents.limiter)
    }

    /// Cancels the actor's pending confirmations (waiting calls answer
    /// `expired`).
    pub(crate) async fn cancel_confirmations_of(&self, actor: &ActionActor) {
        let now = self.inner.clock.unix();
        let for_store = actor.clone();
        match self
            .store()
            .call(move |s| s.cancel_confirmations_of(&for_store, now))
            .await
        {
            Ok(cancelled) => self.confirmations_cancelled(cancelled),
            Err(err) => tracing::warn!(%err, "cannot cancel an actor's confirmations"),
        }
    }

    /// At start: confirmations of an earlier run have nobody waiting to
    /// run them, so approving one would do nothing — they are cancelled.
    pub(crate) fn cancel_orphaned_confirmations(&self) {
        let now = self.inner.clock.unix();
        match self.store().cancel_pending_confirmations(now) {
            Ok(cancelled) => self.confirmations_cancelled(cancelled),
            Err(err) => tracing::warn!(%err, "cannot cancel confirmations of an earlier run"),
        }
    }

    fn confirmations_cancelled(&self, cancelled: Vec<nail_api::agents::Confirmation>) {
        if cancelled.is_empty() {
            return;
        }
        for confirmation in cancelled {
            self.broadcast(Event::ConfirmationResolved {
                confirmation: Box::new(confirmation),
            });
        }
        self.confirmation_broker().wake();
    }

    /// The write path of plug-ins (the MCP write tools' plans).
    pub(crate) fn actor_writes(&self) -> CoreWrites {
        nail_mcp::ActorWrites::new(
            self.store().clone(),
            Arc::new(CoreData(McpCore(self.clone()))),
            WriteSettings::default(),
        )
    }

    /// An MCP client was revoked or deleted, or a plug-in removed: its
    /// pending confirmations are cancelled (waiting calls answer
    /// `expired`) and its policies removed.
    pub(crate) async fn forget_actor(&self, actor: &ActionActor) {
        let now = self.inner.clock.unix();
        let for_store = actor.clone();
        match self
            .store()
            .call(move |s| {
                let cancelled = s.cancel_confirmations_of(&for_store, now)?;
                let customized = s.action_policies(&for_store)?.iter().any(|p| p.customized);
                s.remove_action_policies(&for_store)?;
                Ok((cancelled, customized))
            })
            .await
        {
            Ok((cancelled, customized)) => {
                self.confirmations_cancelled(cancelled);
                if customized {
                    self.broadcast(Event::ActionPoliciesChanged {
                        actor: actor.clone(),
                    });
                }
            }
            Err(err) => tracing::warn!(%err, "cannot cancel an actor's confirmations"),
        }
    }

    /// Hooks of these plug-ins ran: tell the plug-in managers (at most
    /// every few seconds per plug-in; hooks run often).
    pub(crate) fn hooks_ran<'a>(&self, plugins: impl Iterator<Item = &'a nail_api::PluginId>) {
        const EVERY: Duration = Duration::from_secs(5);
        let mut announce = Vec::new();
        {
            let mut last = lock(&self.inner.agents.hooks_announced);
            for plugin in plugins {
                if last.get(plugin).is_none_or(|t| t.elapsed() >= EVERY) {
                    last.insert(plugin.clone(), Instant::now());
                    announce.push(plugin.clone());
                }
            }
        }
        announce.dedup();
        for plugin_id in announce {
            self.broadcast(Event::PluginActivityChanged { plugin_id });
        }
    }

    /// `SendDraft` with the plug-ins' before-send hooks: a veto answers
    /// `SendVetoed` (nothing queued); otherwise the hooks' headers and
    /// content are applied and the draft is queued. With `expected` (the
    /// revision a confirmation pinned; ADR 0086) every write of the draft
    /// on the way (the hooks' content, the crypto choice, the outbox
    /// entry) checks the revision in its store transaction and carries it
    /// forward, so only the approved draft can be queued.
    pub(crate) async fn send_draft_checked(
        &self,
        draft_id: DraftId,
        delay_secs: Option<u32>,
        send_at: Option<String>,
        crypto: Option<SendCrypto>,
        expected: Option<String>,
    ) -> Result<Response> {
        let mut expected = expected;
        if let Some(host) = self.inner.plugins.existing_host().cloned() {
            let id = draft_id.clone();
            let draft = self.inner.store.call(move |s| s.get_draft(&id)).await?;
            // Hooks only ever see the approved draft.
            nail_store::check_revision(&draft, expected.as_deref())?;
            let message = self.outgoing_message(&draft_id, &draft, crypto.as_ref())?;
            let report = host.before_send_hooks(&message).await;
            self.hooks_ran(report.runs.iter().map(|r| &r.plugin_id));
            let any_veto = report.vetoed();
            let headers: Vec<PluginHeader> = report.headers.into_iter().map(|(_, h)| h).collect();
            let additions: Vec<PluginContentAddition> =
                report.additions.into_iter().map(|(_, a)| a).collect();
            if any_veto {
                let veto_id = format!("{:016x}", rand::random::<u64>());
                let mut vetoes = lock(&self.inner.agents.vetoes);
                vetoes.retain(|_, v| v.at.elapsed() < VETO_TTL);
                if vetoes.len() >= MAX_VETOES
                    && let Some(oldest) = vetoes
                        .iter()
                        .min_by_key(|(_, v)| v.at)
                        .map(|(k, _)| k.clone())
                {
                    vetoes.remove(&oldest);
                }
                vetoes.insert(
                    veto_id.clone(),
                    PendingVeto {
                        draft_id: draft_id.clone(),
                        delay_secs,
                        send_at,
                        crypto,
                        expected,
                        headers,
                        additions,
                        at: Instant::now(),
                    },
                );
                return Ok(Response::SendVetoed {
                    draft_id,
                    veto_id: veto_id.into(),
                    vetoes: report.vetoes,
                });
            }
            expected = self
                .apply_before_send(&draft_id, draft, headers, &additions, expected)
                .await?;
        }
        let entry = self
            .send_draft_at(draft_id, delay_secs, send_at, crypto, expected)
            .await?;
        Ok(Response::SendQueued {
            entry: Box::new(entry),
        })
    }

    /// `OverrideSendVeto` ("Send anyway"): sends with the original options.
    pub(crate) async fn override_send_veto(
        &self,
        veto_id: &nail_api::SendVetoId,
    ) -> Result<Response> {
        let pending = {
            let mut vetoes = lock(&self.inner.agents.vetoes);
            vetoes.retain(|_, v| v.at.elapsed() < VETO_TTL);
            vetoes.remove(veto_id.as_str())
        };
        let Some(pending) = pending else {
            return Err(ApiError::not_found(format!("no send veto {veto_id} is pending")).into());
        };
        let id = pending.draft_id.clone();
        let draft = self.inner.store.call(move |s| s.get_draft(&id)).await?;
        let expected = self
            .apply_before_send(
                &pending.draft_id,
                draft,
                pending.headers,
                &pending.additions,
                pending.expected,
            )
            .await?;
        let entry = self
            .send_draft_at(
                pending.draft_id,
                pending.delay_secs,
                pending.send_at,
                pending.crypto,
                expected,
            )
            .await?;
        Ok(Response::SendQueued {
            entry: Box::new(entry),
        })
    }

    /// Writes the hooks' added content into the draft and remembers their
    /// headers for when the message is built. With `expected` the write
    /// happens only while the draft is at that revision; returns the
    /// revision to send (the new one when content was added).
    async fn apply_before_send(
        &self,
        draft_id: &DraftId,
        mut draft: Draft,
        headers: Vec<PluginHeader>,
        additions: &[PluginContentAddition],
        expected: Option<String>,
    ) -> Result<Option<String>> {
        let mut expected = expected;
        if !additions.is_empty() {
            add_content(&mut draft, additions);
            match expected {
                Some(revision) => {
                    expected = Some(self.store_draft_at(draft, revision).await?);
                }
                None => {
                    self.store_draft(draft, false).await?;
                }
            }
        }
        let mut stored = lock(&self.inner.agents.headers);
        if headers.is_empty() {
            stored.remove(draft_id);
        } else {
            stored.insert(draft_id.clone(), headers);
        }
        Ok(expected)
    }

    /// The headers before-send hooks added to a draft (applied by the
    /// sender; kept in memory, forgotten once sent).
    pub(crate) fn plugin_headers(&self, draft_id: &DraftId) -> Vec<PluginHeader> {
        lock(&self.inner.agents.headers)
            .get(draft_id)
            .cloned()
            .unwrap_or_default()
    }

    /// Forgets a sent draft's headers.
    pub(crate) fn forget_plugin_headers(&self, draft_id: &DraftId) {
        lock(&self.inner.agents.headers).remove(draft_id);
    }

    /// A draft as before-send hooks see it.
    fn outgoing_message(
        &self,
        draft_id: &DraftId,
        draft: &Draft,
        crypto: Option<&SendCrypto>,
    ) -> Result<PluginOutgoingMessage> {
        let (account, identity) = self.identity(&draft.identity_id)?;
        let encrypted =
            crate::crypto::effective_crypto(crypto.cloned(), draft.crypto.as_ref(), &identity)
                .is_active();
        let text = match draft.mode {
            ComposerMode::Html if draft.text.trim().is_empty() => draft
                .html
                .as_deref()
                .map(nail_compose::html_to_text)
                .unwrap_or_default(),
            ComposerMode::Plain | ComposerMode::Html => draft.text.clone(),
            ComposerMode::Markdown => draft.markdown.clone().unwrap_or_else(|| draft.text.clone()),
        };
        Ok(PluginOutgoingMessage {
            draft_id: draft_id.clone(),
            account_id: account.id.clone(),
            identity_id: identity.id.clone(),
            from: nail_api::Address {
                name: identity.name.clone(),
                email: identity.email.clone(),
            },
            to: draft.to.clone(),
            cc: draft.cc.clone(),
            bcc: draft.bcc.clone(),
            subject: draft.subject.clone(),
            text,
            html: match draft.mode {
                ComposerMode::Html => draft.html.clone(),
                _ => None,
            },
            attachments: draft
                .attachments
                .iter()
                .map(|a| PluginAttachment {
                    name: a.filename.clone(),
                    mime_type: a.mime.clone(),
                    size: a.size,
                })
                .collect(),
            in_reply_to: draft.replied_message().cloned(),
            encrypted,
        })
    }

    /// Replaces the plug-in Markdown blocks of a Markdown body (fences whose
    /// info string is `<plugin>.<block>`) with what their plug-ins render
    /// for sending. Blocks no running plug-in renders stay code blocks.
    pub(crate) async fn render_plugin_blocks(&self, markdown: &str) -> String {
        let Some(host) = self.inner.plugins.existing_host().cloned() else {
            return markdown.to_owned();
        };
        let fences = plugin_fences(markdown);
        if fences.is_empty() {
            return markdown.to_owned();
        }
        let mut out = String::with_capacity(markdown.len());
        let mut at = 0;
        for fence in fences {
            out.push_str(&markdown[at..fence.start]);
            match host
                .render_markdown_block(
                    &fence.info,
                    &fence.body,
                    nail_api::plugins::extensions::PluginRenderPurpose::Send,
                )
                .await
            {
                Ok(rendered) => out.push_str(&html_block(&rendered.html)),
                Err(err) => {
                    tracing::debug!(%err, info = %fence.info, "plug-in block kept as code");
                    out.push_str(&markdown[fence.start..fence.end]);
                }
            }
            at = fence.end;
        }
        out.push_str(&markdown[at..]);
        out
    }
}

/// Adds before-send content to a draft (plain text always; HTML or
/// Markdown in those modes).
pub(crate) fn add_content(draft: &mut Draft, additions: &[PluginContentAddition]) {
    fn join(top: bool, body: &str, add: &str, sep: &str) -> String {
        if body.is_empty() {
            add.to_owned()
        } else if top {
            format!("{add}{sep}{body}")
        } else {
            format!("{body}{sep}{add}")
        }
    }
    for addition in additions {
        let top = addition.position == PluginContentPosition::Top;
        draft.text = join(top, &draft.text, &addition.text, "\n\n");
        match draft.mode {
            ComposerMode::Plain => {}
            ComposerMode::Html => {
                let html = addition.html.clone().unwrap_or_else(|| {
                    format!(
                        "<p>{}</p>",
                        html_escape(&addition.text).replace('\n', "<br>")
                    )
                });
                let body = draft.html.clone().unwrap_or_default();
                draft.html = Some(join(top, &body, &html, "\n"));
            }
            ComposerMode::Markdown => {
                let body = draft.markdown.clone().unwrap_or_default();
                draft.markdown = Some(join(top, &body, &addition.text, "\n\n"));
            }
        }
    }
}

fn html_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Adds header lines at the top of a built RFC 5322 message.
pub(crate) fn add_headers(raw: &[u8], headers: &[PluginHeader]) -> Vec<u8> {
    let mut out = Vec::with_capacity(raw.len() + headers.len() * 40);
    for header in headers {
        // The host validated them (X- names, one line); stay safe anyway.
        if header.name.contains([':', '\r', '\n']) || header.value.contains(['\r', '\n']) {
            continue;
        }
        out.extend_from_slice(header.name.as_bytes());
        out.extend_from_slice(b": ");
        out.extend_from_slice(header.value.as_bytes());
        out.extend_from_slice(b"\r\n");
    }
    out.extend_from_slice(raw);
    out
}

/// One plug-in fence in a Markdown body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PluginFence {
    /// Byte range of the whole fence (opening line to closing line).
    pub start: usize,
    pub end: usize,
    /// `<plugin>.<block>`.
    pub info: String,
    /// The lines between the fences.
    pub body: String,
}

/// The fenced code blocks whose info string names a plug-in block.
pub(crate) fn plugin_fences(markdown: &str) -> Vec<PluginFence> {
    let mut fences = Vec::new();
    let mut lines = Vec::new();
    let mut offset = 0;
    for line in markdown.split_inclusive('\n') {
        lines.push((offset, line));
        offset += line.len();
    }
    let opening = |line: &str| -> Option<(char, usize, String)> {
        let trimmed = line.trim_end_matches(['\r', '\n']);
        let indent = trimmed.len() - trimmed.trim_start_matches(' ').len();
        if indent > 3 {
            return None;
        }
        let rest = &trimmed[indent..];
        let c = rest.chars().next().filter(|c| *c == '`' || *c == '~')?;
        let count = rest.chars().take_while(|x| *x == c).count();
        if count < 3 {
            return None;
        }
        let info = rest[count..].trim().to_owned();
        if c == '`' && info.contains('`') {
            return None;
        }
        Some((c, count, info))
    };
    let mut i = 0;
    while i < lines.len() {
        let (start, line) = lines[i];
        let Some((c, count, info)) = opening(line) else {
            i += 1;
            continue;
        };
        let name = info
            .split_whitespace()
            .next()
            .unwrap_or_default()
            .to_owned();
        // Find the closing fence.
        let mut j = i + 1;
        let mut close = None;
        while j < lines.len() {
            let text = lines[j].1.trim_end_matches(['\r', '\n']);
            let t = text.trim_start_matches(' ');
            if text.len() - t.len() <= 3
                && t.chars().take_while(|x| *x == c).count() >= count
                && t.trim_start_matches(c).trim().is_empty()
            {
                close = Some(j);
                break;
            }
            j += 1;
        }
        let end_line = close.unwrap_or(lines.len() - 1);
        let end = lines[end_line].0 + lines[end_line].1.len();
        let plugin_block = name.split_once('.').is_some_and(|(p, b)| {
            !p.is_empty() && !b.is_empty() && !b.contains('.') && close.is_some()
        });
        if plugin_block {
            let body: String = lines[i + 1..end_line].iter().map(|(_, l)| *l).collect();
            fences.push(PluginFence {
                start,
                end,
                info: name,
                body,
            });
        }
        i = end_line + 1;
    }
    fences
}

/// Rendered block HTML as a Markdown HTML block (no blank lines inside, so
/// the block is not cut; blank lines around it).
fn html_block(html: &str) -> String {
    let lines: Vec<&str> = html.lines().filter(|l| !l.trim().is_empty()).collect();
    format!("\n<div>\n{}\n</div>\n\n", lines.join("\n"))
}

/// A plug-in write as the shared write path takes it.
pub(crate) fn actor_write(write: nail_plugins::writes::PluginWrite) -> nail_mcp::ActorWrite {
    use nail_mcp::{ActorWrite, PimDelete};
    use nail_plugins::writes::PluginWrite;
    match write {
        PluginWrite::UpdateMessages(a) => ActorWrite::UpdateMessages(a),
        PluginWrite::DeleteMessages(a) => ActorWrite::DeleteMessages(a),
        PluginWrite::CreateDraft(a) => ActorWrite::CreateDraft(a),
        PluginWrite::UpdateDraft(a) => ActorWrite::UpdateDraft(a),
        PluginWrite::SendMessage(a) => ActorWrite::SendMessage(a),
        PluginWrite::CreateEvent(a) => ActorWrite::CreateEvent(a),
        PluginWrite::UpdateEvent(a) => ActorWrite::UpdateEvent(a),
        PluginWrite::RespondToInvite(a) => ActorWrite::RespondToInvite(a),
        PluginWrite::UpsertContact(a) => ActorWrite::UpsertContact(a),
        PluginWrite::UpsertTask(a) => ActorWrite::UpsertTask(a),
        PluginWrite::DeleteEvent {
            event_id,
            occurrence,
            scope,
        } => ActorWrite::Delete(PimDelete::Event {
            event_id,
            occurrence,
            scope,
        }),
        PluginWrite::DeleteContact { contact_id } => {
            ActorWrite::Delete(PimDelete::Contact { contact_id })
        }
        PluginWrite::DeleteTask { task_id } => ActorWrite::Delete(PimDelete::Task { task_id }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draft(mode: ComposerMode) -> Draft {
        let mut draft: Draft = serde_json::from_value(serde_json::json!({
            "identity_id": "me@work.example",
            "subject": "Hi",
            "text": "Body",
        }))
        .unwrap();
        draft.mode = mode;
        draft.html = Some("<p>Body</p>".into());
        draft.markdown = Some("Body".into());
        draft
    }

    fn addition(position: PluginContentPosition, html: Option<&str>) -> PluginContentAddition {
        PluginContentAddition {
            position,
            text: "Note <1>".into(),
            html: html.map(str::to_owned),
        }
    }

    #[test]
    fn content_is_added_in_the_drafts_mode() {
        let mut plain = draft(ComposerMode::Plain);
        add_content(&mut plain, &[addition(PluginContentPosition::Bottom, None)]);
        assert_eq!(plain.text, "Body\n\nNote <1>");
        assert_eq!(plain.html.as_deref(), Some("<p>Body</p>"), "html untouched");

        let mut html = draft(ComposerMode::Html);
        add_content(
            &mut html,
            &[
                addition(PluginContentPosition::Top, None),
                addition(PluginContentPosition::Bottom, Some("<p><b>Sig</b></p>")),
            ],
        );
        assert_eq!(
            html.html.as_deref(),
            Some("<p>Note &lt;1&gt;</p>\n<p>Body</p>\n<p><b>Sig</b></p>")
        );
        assert_eq!(html.text, "Note <1>\n\nBody\n\nNote <1>");

        let mut md = draft(ComposerMode::Markdown);
        add_content(&mut md, &[addition(PluginContentPosition::Top, None)]);
        assert_eq!(md.markdown.as_deref(), Some("Note <1>\n\nBody"));
    }

    #[test]
    fn headers_go_on_top_and_unsafe_ones_are_dropped() {
        let raw = b"From: a@b\r\nSubject: x\r\n\r\nbody";
        let out = add_headers(
            raw,
            &[
                PluginHeader {
                    name: "X-Checked".into(),
                    value: "yes".into(),
                },
                PluginHeader {
                    name: "X-Evil".into(),
                    value: "a\r\nBcc: x@y".into(),
                },
            ],
        );
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "X-Checked: yes\r\nFrom: a@b\r\nSubject: x\r\n\r\nbody"
        );
    }

    #[test]
    fn plugin_fences_are_found_and_others_left_alone() {
        let md = "Intro\n\n```chart.bar title\na,1\nb,2\n```\n\n```rust\nfn x() {}\n```\n\n~~~~kanban.board\n~~~\nstill inside\n~~~~\nEnd\n```open.block\nnever closed\n";
        let fences = plugin_fences(md);
        assert_eq!(fences.len(), 2, "{fences:?}");
        assert_eq!(fences[0].info, "chart.bar");
        assert_eq!(fences[0].body, "a,1\nb,2\n");
        assert_eq!(
            &md[fences[0].start..fences[0].end],
            "```chart.bar title\na,1\nb,2\n```\n"
        );
        assert_eq!(fences[1].info, "kanban.board");
        assert_eq!(fences[1].body, "~~~\nstill inside\n");
        assert_eq!(
            html_block("<p>a</p>\n\n<p>b</p>"),
            "\n<div>\n<p>a</p>\n<p>b</p>\n</div>\n\n"
        );
    }
}
