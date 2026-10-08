//! The connection log, error details and the application log in the core
//! (ADR 0190).
//!
//! * [`ConnLogCtl`] owns the per-scope rings ([`ConnectionLogs`]); engines
//!   get their scope's [`WireTap`] from [`Service::connlog_tap`] and hand it
//!   to their protocol clients. A background task turns new entries into
//!   throttled `ConnectionLogChanged` events (no content).
//! * "Keep connection log on disk" (app setting [`SETTINGS_KEY`], off by
//!   default) mirrors new entries to `<data-dir>/logs/connections/`;
//!   turning it off deletes the folder.
//! * [`error_details`] builds the `ErrorDetails` of a failure; the last
//!   ones per account and folder are stored ([`Service::record_sync_error`]).
//! * `GetAppLog` reads [`crate::logs`].

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use nail_api::connlog::{
    APP_LOG_LINES, CONNECTION_LOG_DIR, ConnectionLogScope, ConnectionLogSettings, ErrorDetails,
    ErrorStep, LOG_DIR, WireProtocol,
};
use nail_api::{AccountId, ApiError, ErrorCode, Event, FolderId, Request, Response};
use nail_connlog::{ConnectionLogs, WireTap};

use crate::error::{CoreError, Result};
use crate::service::Service;

/// The app setting holding [`ConnectionLogSettings`].
pub(crate) const SETTINGS_KEY: &str = "connlog.settings";

/// How often new entries are announced (at most).
const NOTICE_INTERVAL: Duration = Duration::from_millis(250);

/// The core's connection logs.
#[derive(Debug)]
pub(crate) struct ConnLogCtl {
    pub(crate) logs: Arc<ConnectionLogs>,
    /// `<data-dir>/logs/connections` (`None` for an in-memory service).
    dir: Option<PathBuf>,
}

impl ConnLogCtl {
    pub(crate) fn new(data_dir: Option<PathBuf>) -> Self {
        Self {
            logs: ConnectionLogs::new(),
            dir: data_dir.map(|d| d.join(LOG_DIR).join(CONNECTION_LOG_DIR)),
        }
    }
}

/// What Nail was doing when an IMAP error happened, from the error itself
/// where it says so (connection, TLS, sign-in), else `step`.
pub(crate) fn imap_step(err: &nail_proto_imap::ImapError, step: ErrorStep) -> ErrorStep {
    use nail_proto_imap::ImapError as E;
    match err {
        E::Connect { .. } => ErrorStep::Connect,
        E::Tls(_) | E::InvalidServerName(_) | E::StartTlsUnavailable => ErrorStep::Tls,
        E::AuthFailed(_) | E::AuthUnsupported(_) => ErrorStep::Login,
        _ => step,
    }
}

/// Where and how a failure happened (everything but the error itself).
#[derive(Debug, Clone, Default)]
pub(crate) struct ErrorContext {
    pub(crate) protocol: Option<WireProtocol>,
    pub(crate) step: Option<ErrorStep>,
    pub(crate) folder: Option<String>,
    pub(crate) command: Option<String>,
    pub(crate) server_text: Option<String>,
}

impl ErrorContext {
    /// An IMAP failure on `client`: the step from its last command, its
    /// last command and answer, the open folder (or `folder`).
    pub(crate) fn imap(client: &nail_proto_imap::ImapClient, folder: Option<&str>) -> Self {
        let command = client.last_command();
        Self {
            protocol: Some(WireProtocol::Imap),
            step: Some(
                command
                    .as_deref()
                    .map_or(ErrorStep::Other, imap_command_step),
            ),
            folder: folder
                .or_else(|| client.selected_mailbox())
                .map(str::to_owned),
            command,
            server_text: client.last_response(),
        }
    }

    /// A failure without a connection to ask: the step and the server's
    /// text from the error itself where it has them.
    pub(crate) fn of_error(protocol: WireProtocol, err: &CoreError) -> Self {
        let (step, server_text) = match err {
            CoreError::Imap(imap) => {
                let context = Self::of_imap(imap);
                (context.step, context.server_text)
            }
            CoreError::Pop3(_) => (Some(ErrorStep::Fetch), None),
            CoreError::Engine(engine) if engine.code == ErrorCode::AuthFailed => {
                (Some(ErrorStep::Login), None)
            }
            CoreError::Store(_) | CoreError::Search(_) => (Some(ErrorStep::Store), None),
            _ => (None, None),
        };
        Self {
            protocol: Some(protocol),
            step,
            server_text,
            ..Self::default()
        }
    }
}

impl ErrorContext {
    /// An IMAP error without a connection to ask (connecting, signing in).
    pub(crate) fn of_imap(err: &nail_proto_imap::ImapError) -> Self {
        use nail_proto_imap::ImapError as E;
        Self {
            protocol: Some(WireProtocol::Imap),
            step: Some(imap_step(err, ErrorStep::Other)),
            server_text: match err {
                E::No(text) | E::AuthFailed(text) | E::Protocol(text) => Some(text.clone()),
                _ => None,
            },
            ..Self::default()
        }
    }
}

/// What an IMAP command was for (the step of [`ErrorDetails`]).
pub(crate) fn imap_command_step(command: &str) -> ErrorStep {
    let words: Vec<String> = command
        .split_whitespace()
        .skip(1)
        .take(2)
        .map(str::to_ascii_uppercase)
        .collect();
    let verb = match words.first().map(String::as_str) {
        Some("UID") => words.get(1).map_or("", String::as_str),
        Some(verb) => verb,
        None => "",
    };
    match verb {
        "LOGIN" | "AUTHENTICATE" | "CAPABILITY" | "ID" | "ENABLE" | "COMPRESS" => ErrorStep::Login,
        "STARTTLS" => ErrorStep::Tls,
        "LIST" | "LSUB" | "XLIST" | "NAMESPACE" | "STATUS" => ErrorStep::List,
        "SELECT" | "EXAMINE" => ErrorStep::Select,
        "SEARCH" => ErrorStep::Search,
        "FETCH" if command.contains("BODY") || command.contains("RFC822.SIZE") => ErrorStep::Fetch,
        "FETCH" => ErrorStep::Flags,
        "IDLE" | "NOOP" | "DONE" => ErrorStep::Idle,
        "APPEND" | "STORE" | "MOVE" | "COPY" | "EXPUNGE" | "CREATE" | "DELETE" | "RENAME" => {
            ErrorStep::Write
        }
        _ => ErrorStep::Other,
    }
}

/// The details of a failure: the error's class and text (scrubbed with the
/// scope's secrets), the context and the log position.
pub(crate) fn error_details(
    tap: &WireTap,
    log_seq: u64,
    code: ErrorCode,
    detail: &str,
    context: ErrorContext,
) -> ErrorDetails {
    ErrorDetails {
        code,
        protocol: context.protocol,
        step: context.step.unwrap_or(ErrorStep::Other),
        folder: context.folder,
        command: context.command.map(|c| tap.scrub(&c)),
        server_text: context.server_text.map(|t| tap.scrub(&t)),
        detail: tap.scrub(detail),
        at: crate::engine::now_rfc3339(),
        log_seq: (log_seq > 0).then_some(log_seq),
    }
}

impl Service {
    /// The connection logs (tests and the diagnostics bundle).
    pub fn connection_logs(&self) -> &Arc<ConnectionLogs> {
        &self.inner.connlog.logs
    }

    /// The tap of a scope.
    pub(crate) fn connlog_tap(&self, scope: &ConnectionLogScope) -> WireTap {
        self.inner.connlog.logs.tap(scope)
    }

    /// The tap of an account's scope.
    pub(crate) fn account_tap(&self, account: &AccountId) -> WireTap {
        self.connlog_tap(&ConnectionLogScope::account(account))
    }

    /// The details of an account's failure (its scope's tap and log
    /// position).
    pub(crate) fn account_error_details(
        &self,
        account: &AccountId,
        code: ErrorCode,
        detail: &str,
        context: ErrorContext,
    ) -> ErrorDetails {
        let scope = ConnectionLogScope::account(account);
        error_details(
            &self.connlog_tap(&scope),
            self.inner.connlog.logs.last_seq(&scope),
            code,
            detail,
            context,
        )
    }

    /// Stores the last error of an account (`folder` `None`) or folder and
    /// announces it.
    pub(crate) async fn record_sync_error(
        &self,
        account: &AccountId,
        folder: Option<&FolderId>,
        details: &ErrorDetails,
    ) {
        let (id, folder, details) = (account.clone(), folder.cloned(), details.clone());
        let stored = self
            .store()
            .call(move |s| s.set_sync_error(&id, folder.as_ref(), &details))
            .await;
        match stored {
            Ok(()) => self.broadcast(Event::SyncErrorsChanged {
                account_id: account.clone(),
            }),
            Err(err) => tracing::warn!(%err, "cannot store a sync error"),
        }
    }

    /// Forgets the stored error of an account (`folder` `None`) or folder
    /// after a success; announces it when there was one.
    pub(crate) async fn clear_sync_error(&self, account: &AccountId, folder: Option<&FolderId>) {
        let (id, folder) = (account.clone(), folder.cloned());
        match self
            .store()
            .call(move |s| s.clear_sync_error(&id, folder.as_ref()))
            .await
        {
            Ok(true) => self.broadcast(Event::SyncErrorsChanged {
                account_id: account.clone(),
            }),
            Ok(false) => {}
            Err(err) => tracing::warn!(%err, "cannot clear a sync error"),
        }
    }

    /// The stored connection log settings.
    async fn connlog_settings(&self) -> Result<ConnectionLogSettings> {
        Ok(self
            .store()
            .call(|s| s.setting(SETTINGS_KEY))
            .await?
            .unwrap_or_default())
    }

    /// Applies the settings: mirrors to disk, or stops and deletes the
    /// files.
    fn apply_connlog_settings(&self, settings: ConnectionLogSettings) -> Result<()> {
        let ctl = &self.inner.connlog;
        let Some(dir) = &ctl.dir else {
            return Ok(());
        };
        let result = if settings.keep_on_disk {
            ctl.logs.set_disk(Some(dir.clone()), None)
        } else {
            ctl.logs.set_disk(None, Some(dir))
        };
        result.map_err(|err| {
            CoreError::from(ApiError::internal(format!(
                "connection log folder {}: {err}",
                dir.display()
            )))
        })
    }

    /// Reads and applies the settings at start.
    pub(crate) async fn start_connlog(&self) {
        match self.connlog_settings().await {
            Ok(settings) => {
                if let Err(err) = self.apply_connlog_settings(settings) {
                    tracing::warn!(%err, "cannot apply the connection log settings");
                }
            }
            Err(err) => tracing::warn!(%err, "cannot read the connection log settings"),
        }
    }
}

/// Announces new connection log entries, at most every
/// [`NOTICE_INTERVAL`] per scope (the events carry no content).
pub(crate) async fn run_notices(service: Service) {
    let mut interval = tokio::time::interval(NOTICE_INTERVAL);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        interval.tick().await;
        for (scope, last_seq) in service.inner.connlog.logs.take_changed() {
            service.broadcast(Event::ConnectionLogChanged { scope, last_seq });
        }
    }
}

/// The connection log, error and application log requests.
pub(crate) async fn handle(service: &Service, request: Request) -> Result<Response> {
    let logs = Arc::clone(&service.inner.connlog.logs);
    Ok(match request {
        Request::GetConnectionLog {
            scope,
            after,
            limit,
        } => Response::ConnectionLog {
            log: logs.get(
                &scope,
                after,
                limit.map(|l| usize::try_from(l).unwrap_or(usize::MAX)),
            ),
        },
        Request::ClearConnectionLog { scope } => {
            logs.clear(&scope);
            service.broadcast(Event::ConnectionLogChanged {
                last_seq: logs.last_seq(&scope),
                scope,
            });
            Response::Done
        }
        Request::ExportConnectionLog { scope, path } => {
            let text = logs.text(&scope);
            let file_name = nail_connlog::export_name(&scope);
            match path {
                None => Response::ConnectionLogExport {
                    file_name,
                    text: Some(text),
                    path: None,
                },
                Some(path) => {
                    let target = PathBuf::from(&path);
                    if !target.is_absolute() {
                        return Err(ApiError::invalid_request(format!(
                            "{path} is not an absolute path"
                        ))
                        .into());
                    }
                    tokio::task::spawn_blocking(move || {
                        nail_portability::files::write_file(&target, text.as_bytes(), true)
                            .map_err(|e| CoreError::from(e.to_api_error()))
                    })
                    .await??;
                    Response::ConnectionLogExport {
                        file_name,
                        text: None,
                        path: Some(path),
                    }
                }
            }
        }
        Request::GetConnectionLogSettings => Response::ConnectionLogSettings {
            settings: service.connlog_settings().await?,
        },
        Request::SetConnectionLogSettings { settings } => {
            service.apply_connlog_settings(settings)?;
            service
                .store()
                .call(move |s| s.set_setting(SETTINGS_KEY, &settings))
                .await?;
            Response::ConnectionLogSettings { settings }
        }
        Request::ListSyncErrors { account_id } => {
            let (errors, skipped) = service
                .store()
                .call(move |s| {
                    Ok((
                        s.sync_errors(account_id.as_ref())?,
                        s.skipped_messages(account_id.as_ref())?,
                    ))
                })
                .await?;
            Response::SyncErrors { errors, skipped }
        }
        Request::GetAppLog => Response::AppLog {
            log: nail_api::connlog::AppLog {
                path: crate::logs::file_path().map(|p| p.display().to_string()),
                lines: crate::logs::recent_lines(APP_LOG_LINES),
            },
        },
        other => {
            return Err(ApiError::internal(format!(
                "{} is not a connection log request",
                crate::service::request_name(&other)
            ))
            .into());
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn imap_commands_name_their_step() {
        for (command, step) in [
            ("A0001 LOGIN s*** <redacted>", ErrorStep::Login),
            ("A0004 EXAMINE \"INBOX\" (CONDSTORE)", ErrorStep::Select),
            ("A0006 UID SEARCH UID 151:*", ErrorStep::Search),
            ("A0007 UID FETCH 1:50 (UID RFC822.SIZE)", ErrorStep::Fetch),
            (
                "A0008 UID FETCH 151:200 (UID FLAGS INTERNALDATE RFC822.SIZE BODY.PEEK[])",
                ErrorStep::Fetch,
            ),
            (
                "A0009 UID FETCH 1:* (UID FLAGS) (CHANGEDSINCE 4)",
                ErrorStep::Flags,
            ),
            ("A0010 LIST \"\" \"*\"", ErrorStep::List),
            ("A0011 UID MOVE 4 \"Archive\"", ErrorStep::Write),
            ("A0012 IDLE", ErrorStep::Idle),
            ("A0013 XYZZY", ErrorStep::Other),
        ] {
            assert_eq!(imap_command_step(command), step, "{command}");
        }
    }

    #[test]
    fn errors_without_a_connection() {
        let err = CoreError::Imap(nail_proto_imap::ImapError::AuthFailed(
            "[AUTHENTICATIONFAILED] Invalid credentials (Failure)".into(),
        ));
        let context = ErrorContext::of_error(WireProtocol::Imap, &err);
        assert_eq!(context.step, Some(ErrorStep::Login));
        assert!(context.server_text.unwrap().contains("Invalid credentials"));
        let tap = WireTap::off();
        let details = error_details(
            &tap,
            0,
            err.code(),
            "x sam@example.net",
            ErrorContext::default(),
        );
        assert_eq!(details.step, ErrorStep::Other);
        assert_eq!(details.detail, "x s***@example.net");
        assert_eq!(details.log_seq, None);
    }
}
