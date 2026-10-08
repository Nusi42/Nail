//! No browser context menus in Nail's webviews (ADR 0184).
//!
//! A right-click opens Nail's own menu for the place under the pointer, or
//! nothing; WebView2's own menu (Back, Refresh, Save as, Print, Inspect)
//! never shows. The UI's one `contextmenu` handler already cancels every
//! event; this turns the menu off in WebView2 itself as well, so the mail
//! frames and a page still loading are covered too. Applied to every
//! webview the shell creates (main window, pop-outs, print windows) from
//! [`tauri::Builder::on_page_load`]; the developer tools stay reachable in
//! debug builds through F12 (Tauri's `devtools` default).
//!
//! The value WebView2 reports back is kept per window label for the
//! desktop smoke test (`nail_test_shell "default-context-menus"`).
#![cfg_attr(
    windows,
    expect(
        unsafe_code,
        reason = "one WebView2 settings call, with its SAFETY note"
    )
)]

use std::collections::BTreeMap;
use std::sync::Mutex;

use tauri::{Runtime, Webview};

/// `AreDefaultContextMenusEnabled` as WebView2 reported it after Nail
/// turned the menus off, per window label.
static REPORTED: Mutex<BTreeMap<String, bool>> = Mutex::new(BTreeMap::new());

/// Turns WebView2's default context menus off for `webview`.
pub fn disable<R: Runtime>(webview: &Webview<R>) {
    let label = webview.label().to_owned();
    let queued = webview.with_webview(move |platform| {
        #[cfg(windows)]
        // SAFETY: runs on the UI thread with the webview's own controller;
        // every interface comes from WebView2 and lives for the call.
        let reported = unsafe {
            (|| -> windows::core::Result<bool> {
                let settings = platform.controller().CoreWebView2()?.Settings()?;
                settings.SetAreDefaultContextMenusEnabled(false)?;
                let mut enabled = windows::core::BOOL::default();
                settings.AreDefaultContextMenusEnabled(&raw mut enabled)?;
                Ok(enabled.as_bool())
            })()
        };
        #[cfg(not(windows))]
        let reported: Result<bool, &str> = {
            drop(platform);
            Err("WebView2 only")
        };
        match reported {
            Ok(enabled) => {
                REPORTED
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .insert(label, enabled);
            }
            Err(err) => tracing::warn!(%err, %label, "cannot turn the default context menus off"),
        }
    });
    if let Err(err) = queued {
        tracing::warn!(%err, "cannot reach the webview to turn its context menus off");
    }
}

/// What WebView2 reported for each window (label → default menus enabled).
pub fn reported() -> BTreeMap<String, bool> {
    REPORTED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
}
