//! Keeps Nail's webviews alive (ADR 0207).
//!
//! After about two days hidden in the tray the main window once showed only
//! white; F5 did nothing and only restarting Nail helped, while the core
//! kept syncing and logged nothing unusual. Nothing in the shell noticed a
//! webview's page dying. Now:
//!
//! * **WebView2 `ProcessFailed`** (Windows) is watched for every webview
//!   the shell creates (main window, pop-outs, print windows): every
//!   failure is logged with its kind, reason, exit code and process. A
//!   render process that exited or hangs gets its page reloaded, with a
//!   back-off ([`RELOAD_BACKOFF`]) and at most [`MAX_RELOADS`] reloads per
//!   [`RELOAD_WINDOW`]; after that, or when the browser process itself
//!   exited (the webview cannot come back), Nail gives up and says so in a
//!   native message (once per window).
//! * **A ping** when the main window comes up (from the tray, a second
//!   start, focus): a tiny script must answer within [`PING_TIMEOUT`],
//!   otherwise the page counts as hung and is reloaded the same way. Pings
//!   run at most every [`PING_INTERVAL`] per window.
//!
//! F5 never reloads Nail (it checks for new mail, ADR 0183): it runs in the
//! page, so a dead or hung page ignores it. Ctrl+Shift+R is "Reload window"
//! in the UI's command system; a page that cannot run it is reloaded here.

use std::collections::{BTreeMap, VecDeque};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use tauri::{Manager, Runtime, Webview};

/// Reloads a window may get within [`RELOAD_WINDOW`] before Nail gives up.
pub const MAX_RELOADS: usize = 3;

/// The span [`MAX_RELOADS`] counts in.
pub const RELOAD_WINDOW: Duration = Duration::from_mins(15);

/// The wait before the first, second and third reload of a window within
/// [`RELOAD_WINDOW`].
pub const RELOAD_BACKOFF: [Duration; MAX_RELOADS] = [
    Duration::ZERO,
    Duration::from_secs(5),
    Duration::from_secs(30),
];

/// How long a reload may take to load a page before it counts as lost (a
/// new failure then reloads again).
pub const RELOAD_GRACE: Duration = Duration::from_mins(1);

/// How long the page may take to answer a ping.
pub const PING_TIMEOUT: Duration = Duration::from_secs(10);

/// The shortest time between two pings of one window.
pub const PING_INTERVAL: Duration = Duration::from_mins(1);

/// What failed in a webview (WebView2's `COREWEBVIEW2_PROCESS_FAILED_KIND`,
/// plus an unanswered ping).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Failure {
    /// The browser process exited: the webview is gone for good.
    BrowserExited,
    /// The page's render process exited.
    RenderExited,
    /// The page's render process hangs.
    RenderUnresponsive,
    /// The render process of a frame (a mail body) exited; the page lives.
    FrameRenderExited,
    /// The GPU process exited (WebView2 starts a new one).
    GpuExited,
    /// Another helper process exited.
    OtherExited,
    /// The page did not answer a ping within [`PING_TIMEOUT`].
    PingTimeout,
}

impl Failure {
    /// The name in the log.
    pub const fn name(self) -> &'static str {
        match self {
            Self::BrowserExited => "browser-process-exited",
            Self::RenderExited => "render-process-exited",
            Self::RenderUnresponsive => "render-process-unresponsive",
            Self::FrameRenderExited => "frame-render-process-exited",
            Self::GpuExited => "gpu-process-exited",
            Self::OtherExited => "helper-process-exited",
            Self::PingTimeout => "ping-timeout",
        }
    }
}

/// What to do about a failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// Only log it (the page lives, or WebView2 recovers by itself).
    LogOnly,
    /// Reload the page after `after`.
    Reload {
        /// The back-off.
        after: Duration,
    },
    /// Stop trying; `tell` = say so to the user (the first time only).
    GiveUp {
        /// Whether this is the first give-up of the window.
        tell: bool,
    },
}

/// The recovery state of one window.
#[derive(Debug, Default)]
pub struct Recovery {
    reloads: VecDeque<Instant>,
    gave_up: bool,
    /// When a pending reload counts as lost (no page loaded by then).
    reloading_until: Option<Instant>,
    last_ping: Option<Instant>,
    /// Whether `ProcessFailed` is subscribed (a ping may create the entry
    /// before the first page load).
    watched: bool,
}

impl Recovery {
    /// Decides what to do about `failure` at `now`.
    pub fn on_failure(&mut self, failure: Failure, now: Instant) -> Decision {
        match failure {
            Failure::FrameRenderExited | Failure::GpuExited | Failure::OtherExited => {
                return Decision::LogOnly;
            }
            Failure::BrowserExited => return self.give_up(),
            Failure::RenderExited | Failure::RenderUnresponsive | Failure::PingTimeout => {}
        }
        if self.gave_up {
            return Decision::GiveUp { tell: false };
        }
        if self.reloading(now) {
            // A reload is already on its way.
            return Decision::LogOnly;
        }
        while self
            .reloads
            .front()
            .is_some_and(|at| now.duration_since(*at) >= RELOAD_WINDOW)
        {
            self.reloads.pop_front();
        }
        let Some(after) = RELOAD_BACKOFF.get(self.reloads.len()).copied() else {
            return self.give_up();
        };
        self.reloads.push_back(now);
        self.reloading_until = Some(now + after + RELOAD_GRACE);
        Decision::Reload { after }
    }

    fn give_up(&mut self) -> Decision {
        let tell = !self.gave_up;
        self.gave_up = true;
        self.reloading_until = None;
        Decision::GiveUp { tell }
    }

    /// The page finished loading: a reload (or a ping) succeeded.
    pub fn page_loaded(&mut self) {
        self.reloading_until = None;
    }

    /// Whether a reload is on its way at `now`.
    fn reloading(&self, now: Instant) -> bool {
        self.reloading_until.is_some_and(|until| now < until)
    }

    /// Whether to ping at `now` (not while reloading, gave up, or pinged
    /// within [`PING_INTERVAL`]); remembers the ping.
    pub fn should_ping(&mut self, now: Instant) -> bool {
        if self.gave_up || self.reloading(now) {
            return false;
        }
        if self
            .last_ping
            .is_some_and(|at| now.duration_since(at) < PING_INTERVAL)
        {
            return false;
        }
        self.last_ping = Some(now);
        true
    }
}

/// The windows watched so far, by label.
static RECOVERIES: Mutex<BTreeMap<String, Recovery>> = Mutex::new(BTreeMap::new());

fn with_recovery<T>(label: &str, f: impl FnOnce(&mut Recovery) -> T) -> T {
    let mut windows = RECOVERIES.lock().unwrap_or_else(PoisonError::into_inner);
    f(windows.entry(label.to_owned()).or_default())
}

/// What the desktop e2e tests check (`nail_test_shell "webview-health"`):
/// per window label, the reloads within [`RELOAD_WINDOW`] and whether Nail
/// gave up.
pub fn report() -> serde_json::Value {
    let windows = RECOVERIES.lock().unwrap_or_else(PoisonError::into_inner);
    windows
        .iter()
        .map(|(label, r)| {
            (
                label.clone(),
                serde_json::json!({ "reloads": r.reloads.len(), "gave_up": r.gave_up }),
            )
        })
        .collect::<serde_json::Map<_, _>>()
        .into()
}

/// A window closed: its label may come back as a new webview.
pub fn forget(label: &str) {
    RECOVERIES
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .remove(label);
}

/// A page of `webview` loaded (`on_page_load`): watches the webview (once
/// per window) and ends a pending reload once the page finished.
pub fn page_load<R: Runtime>(webview: &Webview<R>, finished: bool) {
    let label = webview.label().to_owned();
    let first = with_recovery(&label, |recovery| {
        if finished {
            recovery.page_loaded();
        }
        !std::mem::replace(&mut recovery.watched, true)
    });
    if first {
        watch(webview);
    }
}

/// Acts on a failure of `webview`'s page (`detail`: what WebView2 said).
pub fn on_failure<R: Runtime>(webview: &Webview<R>, failure: Failure, detail: &str) {
    let label = webview.label().to_owned();
    let decision = with_recovery(&label, |r| r.on_failure(failure, Instant::now()));
    match decision {
        Decision::LogOnly => {
            tracing::warn!(window = %label, failure = failure.name(), detail, "webview process failed");
        }
        Decision::Reload { after } => {
            tracing::error!(
                window = %label,
                failure = failure.name(),
                detail,
                after_ms = u64::try_from(after.as_millis()).unwrap_or(u64::MAX),
                "webview page failed; reloading it"
            );
            let webview = webview.clone();
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(after).await;
                if let Err(err) = webview.reload() {
                    tracing::error!(%err, window = webview.label(), "cannot reload the webview");
                }
            });
        }
        Decision::GiveUp { tell } => {
            tracing::error!(
                window = %label,
                failure = failure.name(),
                detail,
                "webview page failed; giving up"
            );
            if tell {
                tell_user(webview);
            }
        }
    }
}

/// Says that the window cannot be restored (a native message box, on a
/// thread of its own so the UI thread keeps running).
fn tell_user<R: Runtime>(webview: &Webview<R>) {
    let text = webview.try_state::<nail_core::Service>().map_or_else(
        || WINDOW_FAILED_MESSAGE.to_owned(),
        |service| service.render_localized(&nail_api::LocalizedMessage::new(WINDOW_FAILED_MESSAGE)),
    );
    std::thread::spawn(move || crate::error_box(&text));
}

/// The Fluent id of the give-up message (`core-desktop.ftl`).
pub const WINDOW_FAILED_MESSAGE: &str = "shell-window-failed";

/// Pings `webview`'s page (at most every [`PING_INTERVAL`]): a page that
/// does not answer within [`PING_TIMEOUT`] counts as hung.
pub fn ping<R: Runtime>(webview: &Webview<R>) {
    if !with_recovery(webview.label(), |r| r.should_ping(Instant::now())) {
        return;
    }
    let (tx, rx) = tokio::sync::oneshot::channel::<()>();
    let tx = Mutex::new(Some(tx));
    let sent = webview.eval_with_callback("1", move |_| {
        if let Some(tx) = tx.lock().unwrap_or_else(PoisonError::into_inner).take() {
            // The waiter may have timed out already.
            tx.send(()).unwrap_or(());
        }
    });
    if let Err(err) = sent {
        tracing::warn!(%err, window = webview.label(), "cannot ping the webview");
        return;
    }
    let webview = webview.clone();
    tauri::async_runtime::spawn(async move {
        if tokio::time::timeout(PING_TIMEOUT, rx).await.is_err() {
            on_failure(&webview, Failure::PingTimeout, "no answer");
        }
    });
}

/// Subscribes to WebView2's `ProcessFailed` of `webview`.
#[cfg(windows)]
#[expect(unsafe_code, reason = "WebView2 COM calls, each with its SAFETY note")]
fn watch<R: Runtime>(webview: &Webview<R>) {
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        COREWEBVIEW2_PROCESS_FAILED_KIND, COREWEBVIEW2_PROCESS_FAILED_REASON,
        ICoreWebView2ProcessFailedEventArgs2,
    };
    use webview2_com::ProcessFailedEventHandler;
    use windows::core::{Interface, PWSTR};

    let target = webview.clone();
    let queued = webview.with_webview(move |platform| {
        let handler = ProcessFailedEventHandler::create(Box::new(move |_sender, args| {
            let Some(args) = args else {
                return Ok(());
            };
            let mut kind = COREWEBVIEW2_PROCESS_FAILED_KIND::default();
            // SAFETY: WebView2's event arguments, valid for the call.
            unsafe { args.ProcessFailedKind(&raw mut kind)? };
            let mut detail = String::new();
            if let Ok(more) = args.cast::<ICoreWebView2ProcessFailedEventArgs2>() {
                let mut reason = COREWEBVIEW2_PROCESS_FAILED_REASON::default();
                let mut exit_code = 0_i32;
                let mut description = PWSTR::null();
                // SAFETY: as above; the description is WebView2's string,
                // freed by `take_pwstr`.
                unsafe {
                    drop(more.Reason(&raw mut reason));
                    drop(more.ExitCode(&raw mut exit_code));
                    drop(more.ProcessDescription(&raw mut description));
                }
                let description = webview2_com::take_pwstr(description);
                detail = format!(
                    "reason={} exit_code={exit_code} process={description:?}",
                    reason_name(reason)
                );
            }
            on_failure(&target, failure_of(kind), &detail);
            Ok(())
        }));
        let mut token = 0_i64;
        // SAFETY: runs on the UI thread with the webview's own controller;
        // WebView2 keeps the handler alive while subscribed.
        let added = unsafe {
            platform
                .controller()
                .CoreWebView2()
                .and_then(|core| core.add_ProcessFailed(&handler, &raw mut token))
        };
        if let Err(err) = added {
            tracing::warn!(%err, "cannot watch the webview's processes");
        }
    });
    if let Err(err) = queued {
        tracing::warn!(%err, "cannot reach the webview to watch its processes");
    }
}

/// Elsewhere there is no `ProcessFailed`; the ping still works.
#[cfg(not(windows))]
fn watch<R: Runtime>(webview: &Webview<R>) {
    tracing::debug!(window = webview.label(), "no process watch on this system");
}

#[cfg(windows)]
fn failure_of(
    kind: webview2_com::Microsoft::Web::WebView2::Win32::COREWEBVIEW2_PROCESS_FAILED_KIND,
) -> Failure {
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        COREWEBVIEW2_PROCESS_FAILED_KIND_BROWSER_PROCESS_EXITED,
        COREWEBVIEW2_PROCESS_FAILED_KIND_FRAME_RENDER_PROCESS_EXITED,
        COREWEBVIEW2_PROCESS_FAILED_KIND_GPU_PROCESS_EXITED,
        COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_EXITED,
        COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_UNRESPONSIVE,
    };
    match kind {
        COREWEBVIEW2_PROCESS_FAILED_KIND_BROWSER_PROCESS_EXITED => Failure::BrowserExited,
        COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_EXITED => Failure::RenderExited,
        COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_UNRESPONSIVE => Failure::RenderUnresponsive,
        COREWEBVIEW2_PROCESS_FAILED_KIND_FRAME_RENDER_PROCESS_EXITED => Failure::FrameRenderExited,
        COREWEBVIEW2_PROCESS_FAILED_KIND_GPU_PROCESS_EXITED => Failure::GpuExited,
        _ => Failure::OtherExited,
    }
}

#[cfg(windows)]
fn reason_name(
    reason: webview2_com::Microsoft::Web::WebView2::Win32::COREWEBVIEW2_PROCESS_FAILED_REASON,
) -> String {
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        COREWEBVIEW2_PROCESS_FAILED_REASON_CRASHED,
        COREWEBVIEW2_PROCESS_FAILED_REASON_LAUNCH_FAILED,
        COREWEBVIEW2_PROCESS_FAILED_REASON_OUT_OF_MEMORY,
        COREWEBVIEW2_PROCESS_FAILED_REASON_PROFILE_DELETED,
        COREWEBVIEW2_PROCESS_FAILED_REASON_TERMINATED,
        COREWEBVIEW2_PROCESS_FAILED_REASON_UNEXPECTED,
        COREWEBVIEW2_PROCESS_FAILED_REASON_UNRESPONSIVE,
    };
    match reason {
        COREWEBVIEW2_PROCESS_FAILED_REASON_UNEXPECTED => "unexpected".to_owned(),
        COREWEBVIEW2_PROCESS_FAILED_REASON_UNRESPONSIVE => "unresponsive".to_owned(),
        COREWEBVIEW2_PROCESS_FAILED_REASON_TERMINATED => "terminated".to_owned(),
        COREWEBVIEW2_PROCESS_FAILED_REASON_CRASHED => "crashed".to_owned(),
        COREWEBVIEW2_PROCESS_FAILED_REASON_LAUNCH_FAILED => "launch-failed".to_owned(),
        COREWEBVIEW2_PROCESS_FAILED_REASON_OUT_OF_MEMORY => "out-of-memory".to_owned(),
        COREWEBVIEW2_PROCESS_FAILED_REASON_PROFILE_DELETED => "profile-deleted".to_owned(),
        other => format!("code-{}", other.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn helper_processes_are_only_logged() {
        let mut r = Recovery::default();
        let now = Instant::now();
        for failure in [
            Failure::FrameRenderExited,
            Failure::GpuExited,
            Failure::OtherExited,
        ] {
            assert_eq!(r.on_failure(failure, now), Decision::LogOnly);
        }
    }

    #[test]
    fn a_dead_page_is_reloaded_with_back_off_then_given_up() {
        let mut r = Recovery::default();
        let start = Instant::now();
        let mut at = start;
        for expected in RELOAD_BACKOFF {
            assert_eq!(
                r.on_failure(Failure::RenderExited, at),
                Decision::Reload { after: expected }
            );
            // A second report while the reload runs starts no second one.
            assert_eq!(
                r.on_failure(Failure::RenderUnresponsive, at),
                Decision::LogOnly
            );
            r.page_loaded();
            at += Duration::from_mins(1);
        }
        assert_eq!(
            r.on_failure(Failure::PingTimeout, at),
            Decision::GiveUp { tell: true }
        );
        // Told once; later failures stay quiet, even after the window.
        assert_eq!(
            r.on_failure(Failure::RenderExited, at + RELOAD_WINDOW),
            Decision::GiveUp { tell: false }
        );
    }

    #[test]
    fn a_reload_that_never_loads_does_not_block_the_next() {
        let mut r = Recovery::default();
        let now = Instant::now();
        assert!(matches!(
            r.on_failure(Failure::RenderExited, now),
            Decision::Reload { .. }
        ));
        assert_eq!(r.on_failure(Failure::PingTimeout, now), Decision::LogOnly);
        assert!(!r.should_ping(now + Duration::from_secs(30)));
        let later = now + RELOAD_GRACE;
        assert!(r.should_ping(later));
        assert_eq!(
            r.on_failure(Failure::PingTimeout, later),
            Decision::Reload {
                after: RELOAD_BACKOFF[1]
            }
        );
    }

    #[test]
    fn reloads_spread_over_time_never_give_up() {
        let mut r = Recovery::default();
        let mut at = Instant::now();
        for _ in 0..10 {
            assert_eq!(
                r.on_failure(Failure::RenderExited, at),
                Decision::Reload {
                    after: Duration::ZERO
                }
            );
            r.page_loaded();
            at += RELOAD_WINDOW;
        }
    }

    #[test]
    fn a_dead_browser_process_gives_up_at_once() {
        let mut r = Recovery::default();
        let now = Instant::now();
        assert_eq!(
            r.on_failure(Failure::BrowserExited, now),
            Decision::GiveUp { tell: true }
        );
        assert_eq!(
            r.on_failure(Failure::BrowserExited, now),
            Decision::GiveUp { tell: false }
        );
        assert!(!r.should_ping(now));
    }

    #[test]
    fn pings_are_spaced_and_skipped_while_reloading() {
        let mut r = Recovery::default();
        let now = Instant::now();
        assert!(r.should_ping(now));
        assert!(!r.should_ping(now + Duration::from_secs(5)));
        assert!(r.should_ping(now + PING_INTERVAL));
        assert!(matches!(
            r.on_failure(Failure::PingTimeout, now + PING_INTERVAL),
            Decision::Reload { .. }
        ));
        assert!(!r.should_ping(now + PING_INTERVAL + Duration::from_secs(30)));
        r.page_loaded();
        assert!(r.should_ping(now + PING_INTERVAL * 3));
    }

    #[test]
    fn failure_names_are_distinct() {
        let all = [
            Failure::BrowserExited,
            Failure::RenderExited,
            Failure::RenderUnresponsive,
            Failure::FrameRenderExited,
            Failure::GpuExited,
            Failure::OtherExited,
            Failure::PingTimeout,
        ];
        let names: std::collections::BTreeSet<_> = all.iter().map(|f| f.name()).collect();
        assert_eq!(names.len(), all.len());
    }

    /// The give-up message exists in English (no raw id in the box).
    #[test]
    fn the_give_up_message_is_localized() {
        let data = tempfile::tempdir().unwrap();
        let config = tempfile::tempdir().unwrap();
        let service = crate::open_service(&crate::AppArgs::in_dirs(
            data.path(),
            config.path(),
            Some("demo"),
        ))
        .unwrap();
        let text =
            service.render_localized(&nail_api::LocalizedMessage::new(WINDOW_FAILED_MESSAGE));
        assert!(text.contains("Nail"), "{text}");
        assert_ne!(text, WINDOW_FAILED_MESSAGE);
    }
}
