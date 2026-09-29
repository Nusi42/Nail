//! The Nail desktop shell.
//!
//! Stub created by M0 WP0. WP5 adds the `nail_call` command (one
//! `nail_api::Request` in, one `nail_api::Response` out) and forwards core
//! events to the webview on the `nail://event` channel.

/// Name of the Tauri event channel carrying `nail_api::Event`s.
pub const EVENT_CHANNEL: &str = "nail://event";

/// Builds and runs the Tauri application.
///
/// # Panics
///
/// Panics when the Tauri runtime fails to start (e.g. no WebView2 runtime).
pub fn run() {
    tauri::Builder::default()
        .run(tauri::generate_context!())
        .expect("failed to run the Tauri application");
}

#[cfg(test)]
mod tests {
    #[test]
    fn event_channel_name() {
        assert_eq!(super::EVENT_CHANNEL, "nail://event");
    }
}
