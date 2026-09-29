//! Requests from remote-UI sessions (ADR 0041): the remote UI speaks the
//! same protocol as the desktop, but every request is checked first.
//!
//! 1. [`nail_remote_auth::RemoteAuth::authorize`]: the scope (or `admin`
//!    plus a fresh re-authentication) the request needs, desktop-only
//!    requests refused, every denial audited.
//! 2. The grant's account list and folder exclusions (resolved like an
//!    MCP client's, [`nail_mcp::access::Access`]): messages, folders and
//!    accounts a request names must be granted; lists in the answer are
//!    narrowed to what is granted.
//! 3. The `encrypted` gate: decrypted content of end-to-end encrypted mail
//!    only with that scope (like MCP, [`Service::message_is_encrypted`]).
//! 4. Credential and pairing requests run with the session as the actor.
//!
//! `GetMessage` also gets its `render_url` on the sandbox origin.

use nail_api::mcp::McpScope;
use nail_api::{
    AccountId, ApiError, ErrorCode, FolderId, MessageId, MessageSummary, MoveTarget, Request,
    Response,
};
use nail_mcp::access::Access;
use nail_remote_auth::RemoteSessionCtx;

use crate::error::Result;
use crate::service::Service;

fn forbidden(message: String) -> ApiError {
    ApiError::new(ErrorCode::Forbidden, message)
}

impl Service {
    /// Answers `request` from the remote session `ctx` (see the module
    /// docs). Never fails: errors become [`Response::Error`].
    pub async fn handle_remote(&self, ctx: &RemoteSessionCtx, request: Request) -> Response {
        match self.dispatch_remote(ctx, request).await {
            Ok(response) => response,
            Err(err) => ApiError::from(err).into(),
        }
    }

    async fn dispatch_remote(&self, ctx: &RemoteSessionCtx, request: Request) -> Result<Response> {
        self.remote_auth().authorize(ctx, &request)?;
        let access = Access::resolve(
            &ctx.grant().mail,
            &self.inner.accounts.config().virtual_accounts,
        );
        let narrowed = !access.all_accounts() || access.excludes_folders();
        if narrowed {
            self.check_targets(&access, &request).await?;
        }
        let response = match request {
            request @ (Request::SetRemotePassword { .. }
            | Request::StartTotpEnrollment { .. }
            | Request::ConfirmTotpEnrollment { .. }
            | Request::DisableTotp { .. }
            | Request::CreatePairingCode { .. }) => {
                self.remote_auth().handle_admin(request, Some(ctx)).await?
            }
            Request::GetMessage { id } => {
                if !access.has(McpScope::Encrypted) {
                    let check = id.clone();
                    if self
                        .blocking(move |s| s.message_is_encrypted(&check))
                        .await?
                    {
                        return Err(forbidden(format!(
                            "message {id} is encrypted; reading it remotely needs the encrypted scope"
                        ))
                        .into());
                    }
                }
                let mut message = self.message_body(id.clone()).await?;
                message.render_url = self.remote_render_url(&id);
                Response::Message {
                    message: Box::new(message),
                }
            }
            other => self.handle(other).await,
        };
        Ok(if narrowed {
            narrow_response(&access, response)
        } else {
            response
        })
    }

    /// Fails unless every account, folder and message `request` names is
    /// within the grant.
    async fn check_targets(&self, access: &Access, request: &Request) -> Result<()> {
        let account = |id: &AccountId| -> Result<()> {
            if access.allows_account(id) {
                Ok(())
            } else {
                Err(forbidden(format!("account {id} is not granted")).into())
            }
        };
        let folder = |id: &FolderId| -> Result<()> {
            if access.allows_folder(id) {
                Ok(())
            } else {
                Err(forbidden(format!("folder {id} is not granted")).into())
            }
        };
        match request {
            Request::ListFolders { account_id }
            | Request::ListIdentities {
                account_id: Some(account_id),
            }
            | Request::SyncNow {
                account_id: Some(account_id),
            } => account(account_id),
            Request::SyncNow { account_id: None } if !access.all_accounts() => {
                Err(forbidden("syncing every account needs a grant for all accounts".into()).into())
            }
            Request::ListMessages { folder_id, .. } | Request::ListThreads { folder_id, .. } => {
                folder(folder_id)
            }
            Request::Search { query } => {
                if let Some(id) = &query.account_id {
                    account(id)?;
                }
                if let Some(id) = &query.folder_id {
                    folder(id)?;
                }
                Ok(())
            }
            Request::GetMessage { id } => {
                self.check_messages(access, std::slice::from_ref(id)).await
            }
            Request::UpdateFlags { message_ids, .. }
            | Request::DeleteMessages { message_ids, .. } => {
                self.check_messages(access, message_ids).await
            }
            Request::MoveMessages {
                message_ids,
                target,
            } => {
                if let MoveTarget::FolderId(id) = target {
                    folder(id)?;
                }
                self.check_messages(access, message_ids).await
            }
            Request::AllowRemoteContent { message_id, .. } => {
                self.check_messages(access, std::slice::from_ref(message_id))
                    .await
            }
            _ => Ok(()),
        }
    }

    async fn check_messages(&self, access: &Access, ids: &[MessageId]) -> Result<()> {
        let ids = ids.to_vec();
        let summaries = self
            .inner
            .store
            .call(move |s| {
                ids.iter()
                    .map(|id| s.get_message(id).map(|m| m.summary))
                    .collect::<nail_store::Result<Vec<_>>>()
            })
            .await?;
        for summary in &summaries {
            if !message_allowed(access, summary) {
                return Err(forbidden(format!("message {} is not granted", summary.id)).into());
            }
        }
        Ok(())
    }
}

fn message_allowed(access: &Access, message: &MessageSummary) -> bool {
    access.allows_account(&message.account_id) && access.allows_folder(&message.folder_id)
}

/// Drops what the grant does not cover from list answers.
fn narrow_response(access: &Access, response: Response) -> Response {
    let keep = |m: &MessageSummary| message_allowed(access, m);
    match response {
        Response::Accounts { mut accounts } => {
            accounts.retain(|a| access.allows_account(&a.id));
            Response::Accounts { accounts }
        }
        Response::Folders { mut folders } => {
            folders.retain(|f| access.allows_account(&f.account_id) && access.allows_folder(&f.id));
            Response::Folders { folders }
        }
        Response::Messages {
            folder_id,
            offset,
            total,
            mut messages,
        } => {
            messages.retain(keep);
            Response::Messages {
                folder_id,
                offset,
                total,
                messages,
            }
        }
        Response::Thread {
            thread,
            mut messages,
        } => {
            messages.retain(keep);
            Response::Thread { thread, messages }
        }
        Response::SearchResults {
            offset,
            total,
            mut hits,
        } => {
            hits.retain(|h| keep(&h.message));
            Response::SearchResults {
                offset,
                total,
                hits,
            }
        }
        Response::AccountStatuses { mut statuses } => {
            statuses.retain(|s| access.allows_account(&s.account_id));
            Response::AccountStatuses { statuses }
        }
        other => other,
    }
}
