//! The page's own diagnostics in `app.log` (ADR 0207).
//!
//! A webview page can die or hang while the core keeps running (the
//! desktop window going white after days in the tray left no trace). The
//! UI therefore reports its start, its uncaught errors and rejections and,
//! now and then, its memory use ([`Request::ReportUiDiagnostics`]); this
//! writes them as [`UI_LOG_TARGET`] lines into the application log, where they pass
//! the log's redactor like every other line. Problems are capped per minute
//! here as well as in the UI, so no page (a remote one included) can flood
//! the log.

use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use nail_api::connlog::{
    UI_LOG_TARGET, UI_PROBLEMS_PER_MINUTE, UI_REPORT_MAX_CHARS, UiProblemKind, UiReport,
};
use nail_api::{Request, Response};

/// How severe a line is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Severity {
    /// Starts and memory samples.
    Info,
    /// Uncaught problems.
    Warn,
}

/// Cuts `text` to [`UI_REPORT_MAX_CHARS`] characters and keeps it on one
/// line (a stack's line breaks become ` | `).
fn one_line(text: &str) -> String {
    let joined = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join(" | ");
    match joined.char_indices().nth(UI_REPORT_MAX_CHARS) {
        Some((end, _)) => format!("{}…", &joined[..end]),
        None => joined,
    }
}

fn kind_name(kind: UiProblemKind) -> &'static str {
    match kind {
        UiProblemKind::Error => "error",
        UiProblemKind::UnhandledRejection => "unhandled-rejection",
        UiProblemKind::RenderError => "render-error",
    }
}

fn kib(value: Option<u32>) -> String {
    value.map_or_else(|| "-".to_owned(), |v| v.to_string())
}

/// The log line of a report (without the level and target).
pub(crate) fn line(report: &UiReport) -> (Severity, String) {
    match report {
        UiReport::Started { route } => (
            Severity::Info,
            format!("page started route={}", one_line(route)),
        ),
        UiReport::Problem {
            kind,
            message,
            stack,
            suppressed,
        } => {
            let mut text = format!("page {}: {}", kind_name(*kind), one_line(message));
            if let Some(stack) = stack.as_deref().filter(|s| !s.trim().is_empty()) {
                text.push_str(" stack=");
                text.push_str(&one_line(stack));
            }
            if *suppressed > 0 {
                text = format!("{text} suppressed_before={suppressed}");
            }
            (Severity::Warn, text)
        }
        UiReport::Memory {
            uptime_minutes,
            hidden,
            heap_used_kib,
            heap_total_kib,
            heap_limit_kib,
            dom_nodes,
        } => (
            Severity::Info,
            format!(
                "page memory uptime_min={uptime_minutes} hidden={hidden} heap_used_kib={} heap_total_kib={} heap_limit_kib={} dom_nodes={dom_nodes}",
                kib(*heap_used_kib),
                kib(*heap_total_kib),
                kib(*heap_limit_kib),
            ),
        ),
    }
}

/// The per-minute cap on problem lines.
#[derive(Debug)]
pub(crate) struct ProblemLimit {
    window_start: Option<Instant>,
    in_window: u32,
    dropped: u32,
}

impl ProblemLimit {
    pub(crate) const fn new() -> Self {
        Self {
            window_start: None,
            in_window: 0,
            dropped: 0,
        }
    }

    /// Whether a problem at `now` may be written; `Some(n)` = yes, `n`
    /// problems were dropped since the last written one.
    pub(crate) fn admit(&mut self, now: Instant) -> Option<u32> {
        let fresh = self
            .window_start
            .is_none_or(|start| now.duration_since(start) >= Duration::from_mins(1));
        if fresh {
            self.window_start = Some(now);
            self.in_window = 0;
        }
        if self.in_window >= UI_PROBLEMS_PER_MINUTE {
            self.dropped = self.dropped.saturating_add(1);
            return None;
        }
        self.in_window += 1;
        Some(std::mem::take(&mut self.dropped))
    }
}

static LIMIT: Mutex<ProblemLimit> = Mutex::new(ProblemLimit::new());

/// Writes one report to the log.
pub(crate) fn write(report: &UiReport) {
    let (severity, text) = line(report);
    match severity {
        Severity::Info => tracing::info!(target: UI_LOG_TARGET, "{text}"),
        Severity::Warn => {
            let admitted = LIMIT
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .admit(Instant::now());
            match admitted {
                Some(0) => tracing::warn!(target: UI_LOG_TARGET, "{text}"),
                Some(dropped) => {
                    tracing::warn!(target: UI_LOG_TARGET, dropped_by_core = dropped, "{text}");
                }
                None => {}
            }
        }
    }
}

/// `ReportUiDiagnostics`.
pub(crate) fn handle(request: Request) -> Response {
    if let Request::ReportUiDiagnostics { report } = request {
        write(&report);
    }
    Response::Done
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn reports_become_single_lines() {
        let (severity, text) = line(&UiReport::Problem {
            kind: UiProblemKind::UnhandledRejection,
            message: "boom".into(),
            stack: Some("Error: boom\n    at a (x.js:1:2)\n\n    at b (y.js:3:4)\n".into()),
            suppressed: 3,
        });
        assert_eq!(severity, Severity::Warn);
        assert_eq!(
            text,
            "page unhandled-rejection: boom stack=Error: boom | at a (x.js:1:2) | at b (y.js:3:4) suppressed_before=3"
        );
        let (severity, text) = line(&UiReport::Memory {
            uptime_minutes: 90,
            hidden: true,
            heap_used_kib: Some(1024),
            heap_total_kib: None,
            heap_limit_kib: Some(4096),
            dom_nodes: 1500,
        });
        assert_eq!(severity, Severity::Info);
        assert_eq!(
            text,
            "page memory uptime_min=90 hidden=true heap_used_kib=1024 heap_total_kib=- heap_limit_kib=4096 dom_nodes=1500"
        );
        let (_, text) = line(&UiReport::Started {
            route: "#/mail".into(),
        });
        assert_eq!(text, "page started route=#/mail");
    }

    #[test]
    fn long_texts_are_cut() {
        let long = "é".repeat(UI_REPORT_MAX_CHARS + 50);
        let cut = one_line(&long);
        assert_eq!(cut.chars().count(), UI_REPORT_MAX_CHARS + 1);
        assert!(cut.ends_with('…'));
        assert_eq!(one_line("short"), "short");
    }

    #[test]
    fn problems_are_capped_per_minute() {
        let mut limit = ProblemLimit::new();
        let start = Instant::now();
        for _ in 0..UI_PROBLEMS_PER_MINUTE {
            assert_eq!(limit.admit(start), Some(0));
        }
        assert_eq!(limit.admit(start + Duration::from_secs(10)), None);
        assert_eq!(limit.admit(start + Duration::from_secs(20)), None);
        // A new minute: the next line says how many were dropped.
        assert_eq!(limit.admit(start + Duration::from_mins(1)), Some(2));
        assert_eq!(limit.admit(start + Duration::from_mins(1)), Some(0));
    }

    #[test]
    fn the_request_is_answered_done() {
        let response = handle(Request::ReportUiDiagnostics {
            report: UiReport::Started { route: "#/".into() },
        });
        assert_eq!(response, Response::Done);
    }
}
