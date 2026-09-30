//! The plug-in host (plan M2d WP61; contract ADR 0050): one sandboxed
//! QuickJS runtime per plug-in, the permission gate in front of the Client
//! API, the preview API bindings (`nail.*`) and `.nailplugin` packages.
//!
//! **Stub (WP60).** WP61 fills this crate against `nail_api::plugins`.
//! Expected shape (WP61 decides the details and records them in its ADR):
//!
//! * `runtime` — a QuickJS runtime per plug-in (`rquickjs`): a memory
//!   limit, an interrupt handler enforcing a CPU time budget per call
//!   (an endless loop is stopped and the plug-in flagged
//!   `PluginStopReason::CpuBudget`), no filesystem, process or socket
//!   access (no `std` / `os` modules; only the bundled entry module is
//!   loaded). Uncaught errors stop the plug-in (`PluginStopReason::Error`).
//! * `gate` — every API call checks the plug-in's grants
//!   (`nail_api::plugins::PluginGrant`): `mail.read` (headers, per
//!   account), `mail.read.body` (never decrypted end-to-end mail),
//!   `ui.dialogs`, `notifications`, `storage`, `network:<domain>`
//!   (`PluginPermission::allows_host`). A denied call rejects with an
//!   error whose code is `forbidden` and is logged (`PluginLogSource::Api`).
//! * `api` — the bindings: `nail.mail` (read; `PluginMessage`,
//!   `PluginMessageBody`), `nail.ui.showDialog` (`PluginDialogSpec` →
//!   `PluginDialogResult`), `nail.ui.notify`, `nail.commands.register` /
//!   `ctx.commands.register` (`PluginCommandContext`), panels
//!   (`PluginUiNode` trees, `nail_api::plugins::validate_tree`),
//!   `nail.storage` (quota), `nail.http.fetch` (declared domains only,
//!   through the core), `nail.settings`, `nail.log` / `console`.
//!   The host talks to the core through a trait (implemented by
//!   `nail-core`, WP64), so this crate never depends on the store or the
//!   core.
//! * `package` — `.nailplugin` archives (zip: `manifest.json`, the bundled
//!   JS, assets, locales; `nail_api::plugins::admin::MAX_PACKAGE_BYTES`,
//!   no paths outside the package), SHA-256 of the package, unpacking to
//!   `<data-dir>/plugins/<id>/`.
//! * `dev` — dev folders (`nail plugin dev <folder>`): load unpacked,
//!   reload on change (`notify-debouncer-mini`), log lines as
//!   `PluginLogLine` events.

#![warn(missing_docs)]

/// Name of the crate, for diagnostics.
pub const CRATE: &str = env!("CARGO_PKG_NAME");

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::io::{Read, Write};
    use std::rc::Rc;
    use std::time::{Duration, Instant};

    use nail_api::plugins::PluginManifest;
    use rquickjs::{Context, Runtime};

    #[test]
    fn crate_name() {
        assert_eq!(super::CRATE, "nail-plugins");
    }

    /// The engine builds and runs on this target (QuickJS C sources through
    /// `cc`, MSVC on Windows).
    #[test]
    fn quickjs_evaluates() {
        let rt = Runtime::new().unwrap();
        let ctx = Context::full(&rt).unwrap();
        let sum: i32 = ctx.with(|ctx| ctx.eval("[1, 2, 3].reduce((a, b) => a + b, 0)").unwrap());
        assert_eq!(sum, 6);
        let json: String = ctx.with(|ctx| {
            ctx.eval(r#"JSON.stringify({type: "text", value: "hi"})"#)
                .unwrap()
        });
        assert_eq!(json, r#"{"type":"text","value":"hi"}"#);
    }

    /// The two sandbox limits the host relies on: the interrupt handler
    /// stops an endless loop, the memory limit stops runaway allocation.
    #[test]
    fn quickjs_limits_work() {
        let rt = Runtime::new().unwrap();
        let deadline = Rc::new(Cell::new(Instant::now() + Duration::from_millis(100)));
        let d = Rc::clone(&deadline);
        rt.set_interrupt_handler(Some(Box::new(move || Instant::now() > d.get())));
        let ctx = Context::full(&rt).unwrap();
        let started = Instant::now();
        let looped: rquickjs::Result<()> = ctx.with(|ctx| ctx.eval("for (;;) {}"));
        assert!(looped.is_err());
        assert!(started.elapsed() < Duration::from_secs(5));

        // The runtime stays usable after an interrupt.
        deadline.set(Instant::now() + Duration::from_secs(60));
        let two: i32 = ctx.with(|ctx| ctx.eval("1 + 1").unwrap());
        assert_eq!(two, 2);

        let small = Runtime::new().unwrap();
        small.set_memory_limit(8 * 1024 * 1024);
        let ctx = Context::full(&small).unwrap();
        let grown: rquickjs::Result<()> =
            ctx.with(|ctx| ctx.eval("let a = []; for (;;) { a.push('x'.repeat(1024)); }"));
        assert!(grown.is_err());
    }

    /// No host capabilities by default: a full context has no `require`,
    /// `fetch`, `process` or `std` / `os` objects.
    #[test]
    fn quickjs_context_has_no_host_access() {
        let rt = Runtime::new().unwrap();
        let ctx = Context::full(&rt).unwrap();
        let kinds: String = ctx.with(|ctx| {
            ctx.eval(
                "[typeof require, typeof fetch, typeof process, typeof std, typeof os, typeof XMLHttpRequest].join()",
            )
            .unwrap()
        });
        assert_eq!(
            kinds,
            "undefined,undefined,undefined,undefined,undefined,undefined"
        );
    }

    /// `.nailplugin` archives are zip files with deflate.
    #[test]
    fn zip_round_trip() {
        let manifest =
            r#"{"id":"x","name":"X","version":"1.0.0","apiVersion":"1","main":"index.js"}"#;
        let mut buf = std::io::Cursor::new(Vec::new());
        {
            let mut zip = zip::ZipWriter::new(&mut buf);
            let options = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);
            zip.start_file("manifest.json", options).unwrap();
            zip.write_all(manifest.as_bytes()).unwrap();
            zip.start_file("index.js", options).unwrap();
            zip.write_all(b"export default {}").unwrap();
            zip.finish().unwrap();
        }
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(buf.into_inner())).unwrap();
        assert_eq!(archive.len(), 2);
        let mut text = String::new();
        archive
            .by_name("manifest.json")
            .unwrap()
            .read_to_string(&mut text)
            .unwrap();
        let parsed = PluginManifest::parse(text.as_bytes()).unwrap();
        assert_eq!(parsed.id.as_str(), "x");
    }
}
