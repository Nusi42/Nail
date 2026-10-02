//! PDF files of print jobs (M4c, ADRs 0094, 0099): the print view
//! (`#/print/<job>`) is loaded in a hidden window of the normal UI; once it
//! reports `PrintViewReady` (all data loaded and laid out), WebView2's
//! `PrintToPdf` writes it with the job's paper size and orientation, and
//! the window closes.

#![allow(unsafe_code)] // WebView2 COM calls, each with its SAFETY note.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use nail_api::print::{PageOrientation, PageSize, PrintJob, PrintOptions};
use nail_api::{ApiError, LocalizedMessage, PrintJobId};
use tokio::sync::oneshot;

/// How long the print view may take to load its data.
pub const READY_TIMEOUT: Duration = Duration::from_secs(60);

/// How long WebView2 may take to write the PDF.
pub const PDF_TIMEOUT: Duration = Duration::from_secs(120);

/// Millimetres per inch (WebView2 print settings are in inches).
const MM_PER_INCH: f64 = 25.4;

/// The page in inches, portrait (WebView2 turns it for landscape).
pub fn page_inches(size: PageSize) -> (f64, f64) {
    let (w, h) = size.millimetres();
    (w / MM_PER_INCH, h / MM_PER_INCH)
}

/// Whether the job prints landscape.
pub fn landscape(options: &PrintOptions) -> bool {
    options.orientation == PageOrientation::Landscape
}

/// The label of a job's hidden window (letters, digits, `-` only).
pub fn window_label(job: &PrintJobId) -> String {
    let clean: String = job
        .as_str()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    format!("print-{clean}")
}

/// The app URL of a job's print view (`index.html#/print/<id>`).
pub fn view_url(job: &PrintJob) -> String {
    let route = job.view.trim_start_matches('#');
    format!("index.html#{route}")
}

/// Print views waiting for their ready signal.
#[derive(Debug, Default)]
pub struct ReadySignals {
    waiting: Mutex<HashMap<PrintJobId, oneshot::Sender<()>>>,
}

impl ReadySignals {
    /// Starts waiting for `job`.
    pub fn expect(&self, job: &PrintJobId) -> oneshot::Receiver<()> {
        let (tx, rx) = oneshot::channel();
        self.lock().insert(job.clone(), tx);
        rx
    }

    /// The view of `job` is ready (from the core's `PrintViewReady`).
    pub fn ready(&self, job: &PrintJobId) {
        if let Some(tx) = self.lock().remove(job) {
            let _ = tx.send(());
        }
    }

    /// Stops waiting for `job`.
    pub fn forget(&self, job: &PrintJobId) {
        self.lock().remove(job);
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<PrintJobId, oneshot::Sender<()>>> {
        self.waiting
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

/// A PDF failure as the core's answer.
pub fn pdf_error(detail: impl std::fmt::Display) -> ApiError {
    ApiError::internal(format!("cannot write the PDF: {detail}"))
        .with_localized(LocalizedMessage::new("print-pdf-failed"))
}

/// Checks the target: absolute, its folder exists, not a folder itself.
pub fn check_target(path: &str) -> Result<PathBuf, ApiError> {
    let target = PathBuf::from(path);
    if !target.is_absolute() {
        return Err(ApiError::invalid_request("the PDF needs a full path")
            .with_localized(LocalizedMessage::new("portability-error-path-not-absolute")));
    }
    if target.is_dir() {
        return Err(ApiError::invalid_request("the PDF path is a folder"));
    }
    match target.parent() {
        Some(parent) if parent.as_os_str().is_empty() || parent.is_dir() => Ok(target),
        _ => Err(ApiError::not_found("the folder of the PDF does not exist")
            .with_localized(LocalizedMessage::new("portability-error-missing-path"))),
    }
}

/// Writes the page shown in `webview` as PDF to `path` (WebView2
/// `PrintToPdf`); answers through `done`. Runs on the UI thread.
#[cfg(windows)]
pub fn print_webview_to_pdf(
    webview: &tauri::webview::PlatformWebview,
    path: &Path,
    options: &PrintOptions,
    done: oneshot::Sender<Result<(), String>>,
) {
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        COREWEBVIEW2_PRINT_ORIENTATION_LANDSCAPE, COREWEBVIEW2_PRINT_ORIENTATION_PORTRAIT,
        ICoreWebView2_7, ICoreWebView2Environment6,
    };
    use webview2_com::PrintToPdfCompletedHandler;
    use windows::core::{HSTRING, Interface};

    let done = std::sync::Arc::new(Mutex::new(Some(done)));
    let finish = {
        let done = std::sync::Arc::clone(&done);
        move |result: Result<(), String>| {
            if let Some(tx) = done
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take()
            {
                let _ = tx.send(result);
            }
        }
    };
    let (width, height) = page_inches(options.page_size);
    let backgrounds = options.backgrounds;
    let orientation = if landscape(options) {
        COREWEBVIEW2_PRINT_ORIENTATION_LANDSCAPE
    } else {
        COREWEBVIEW2_PRINT_ORIENTATION_PORTRAIT
    };
    let path = HSTRING::from(path.as_os_str());
    // SAFETY: called on the UI thread with the webview's own controller
    // and environment; every interface comes from WebView2 and lives for
    // the call; the completion handler is kept alive by WebView2.
    let started = unsafe {
        (|| -> windows::core::Result<()> {
            let core = webview.controller().CoreWebView2()?;
            let core7: ICoreWebView2_7 = core.cast()?;
            let environment: ICoreWebView2Environment6 = webview.environment().cast()?;
            let settings = environment.CreatePrintSettings()?;
            settings.SetOrientation(orientation)?;
            settings.SetPageWidth(width)?;
            settings.SetPageHeight(height)?;
            settings.SetShouldPrintBackgrounds(backgrounds)?;
            settings.SetShouldPrintHeaderAndFooter(false)?;
            let on_done = finish.clone();
            let handler = PrintToPdfCompletedHandler::create(Box::new(move |result, ok| {
                on_done(match result {
                    Ok(()) if ok => Ok(()),
                    Ok(()) => Err("WebView2 could not write the PDF".to_owned()),
                    Err(err) => Err(err.to_string()),
                });
                Ok(())
            }));
            core7.PrintToPdf(&path, &settings, &handler)
        })()
    };
    if let Err(err) = started {
        finish(Err(err.to_string()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pages_labels_and_urls() {
        let (w, h) = page_inches(PageSize::Letter);
        assert!((w - 8.5).abs() < 1e-9 && (h - 11.0).abs() < 1e-9);
        let (w, h) = page_inches(PageSize::A4);
        assert!((w - 8.267).abs() < 0.001 && (h - 11.693).abs() < 0.001);
        assert_eq!(
            window_label(&PrintJobId::new("print-42-7")),
            "print-print-42-7"
        );
        assert_eq!(window_label(&PrintJobId::new("a/b c")), "print-a-b-c");
        let job = PrintJob {
            job_id: PrintJobId::new("j1"),
            target: nail_api::print::PrintTarget::Message {
                message_id: "m1".into(),
            },
            options: PrintOptions::default(),
            created_at: String::new(),
            view: "#/print/j1".into(),
            ready: false,
        };
        assert_eq!(view_url(&job), "index.html#/print/j1");
        assert!(!landscape(&job.options));
    }

    #[tokio::test]
    async fn ready_signals_wake_the_waiting_job() {
        let signals = ReadySignals::default();
        let job = PrintJobId::new("j1");
        let rx = signals.expect(&job);
        signals.ready(&PrintJobId::new("other"));
        signals.ready(&job);
        assert!(rx.await.is_ok());
        let rx = signals.expect(&job);
        signals.forget(&job);
        assert!(rx.await.is_err());
    }

    #[test]
    fn targets_must_be_absolute_files_in_existing_folders() {
        assert!(check_target("relative.pdf").is_err());
        let dir = tempfile::tempdir().unwrap();
        assert!(check_target(&dir.path().display().to_string()).is_err());
        let ok = dir.path().join("x.pdf");
        assert_eq!(check_target(&ok.display().to_string()).unwrap(), ok);
        let missing = dir.path().join("nope").join("x.pdf");
        assert!(check_target(&missing.display().to_string()).is_err());
    }
}
