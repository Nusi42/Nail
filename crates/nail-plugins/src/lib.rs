//! The plug-in host (plan M2d WP61; contract ADR 0050, host decisions ADR
//! 0051): one sandboxed QuickJS runtime per plug-in, the permission
//! gate in front of the core, the preview API (`nail.*`), `.nailplugin`
//! packages and dev reload.
//!
//! * [`PluginHost`] ([`host`]) starts, stops and reloads plug-ins and
//!   forwards the runtime requests (commands, dialog answers, UI actions,
//!   panels). Each plug-in runs on **its own thread** with its own QuickJS
//!   runtime (`runtime.rs`): a memory limit, an interrupt-based CPU budget
//!   per call ([`HostLimits`]), no filesystem, process or socket access
//!   (QuickJS's `std` / `os` modules are never loaded; the module loader
//!   serves only the plug-in's own files). An uncaught error, an exceeded
//!   budget or exhausted memory stops that plug-in and flags it
//!   (`PluginStopped`, state `crashed`); the others keep running.
//! * [`gate`]: every API call is checked against the grants
//!   (`mail.read` per account, `mail.read.body` never for end-to-end
//!   encrypted mail, `ui.dialogs`, `notifications`, `storage`,
//!   `network:<host>`); refusals fail with `forbidden` and are logged.
//! * [`PluginServices`] ([`services`]): what the core implements (mail
//!   reads, storage, HTTP, events, log lines); this crate never touches
//!   the store or the network itself.
//! * The API a plug-in sees is built by `prelude.js`: `nail.mail` (`get`,
//!   `getBody`, `search`, `thread`), `nail.ui` (`registerCommand`,
//!   `registerPanel`, `showDialog`, `notify`, `updatePanel`),
//!   `nail.commands.register`, `nail.storage`, `nail.http.fetch`,
//!   `nail.settings`, `nail.log`, `console`, `setTimeout` /
//!   `clearTimeout`, `queueMicrotask`. The entry module's default export
//!   (or its named `activate`) is started with a context
//!   (`ctx.commands.register`, `ctx.panels.register`, `ctx.settings`).
//! * [`package`]: `.nailplugin` archives (checked for traversal, links,
//!   size and zip bombs), installed packages kept packed and verified by
//!   hash, dev folders.
//! * [`dev`]: the dev folder watcher behind `nail plugin dev`.
//! * [`l10n`]: the host's Fluent ids and the plug-ins' own catalogs
//!   (`%fluent-id%` manifest texts).
//! * [`tree`]: component trees on their way to the UI.

#![warn(missing_docs)]

pub mod dev;
pub mod gate;
pub mod host;
pub mod l10n;
pub mod limits;
pub mod package;
pub(crate) mod runtime;
pub mod services;
pub mod tree;

pub use host::{DevLoadError, PluginHost, PluginSpec, effective_settings};
pub use l10n::PluginCatalogs;
pub use limits::HostLimits;
pub use package::{PackageError, PackageErrorKind, PackageFiles, PackageStore, PluginPackage};
pub use services::{
    AccountScope, BoxFuture, HostLogLine, HttpRequest, HttpResponse, PluginServices, ServiceResult,
};

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

    /// M4a (ADR 0079): binary bodies cross the boundary as typed arrays
    /// (`array-buffer` feature): bytes from Rust arrive as a `Uint8Array`,
    /// and a plug-in's `ArrayBuffer` comes back as bytes.
    #[test]
    fn quickjs_typed_arrays_carry_bytes() {
        use rquickjs::{Function, TypedArray};
        let rt = Runtime::new().unwrap();
        let ctx = Context::full(&rt).unwrap();
        ctx.with(|ctx| {
            let bytes = TypedArray::<u8>::new(ctx.clone(), vec![0u8, 1, 2, 255]).unwrap();
            let sum: Function<'_> = ctx
                .eval("(a) => a instanceof Uint8Array ? a.reduce((x, y) => x + y, 0) : -1")
                .unwrap();
            let total: i32 = sum.call((bytes,)).unwrap();
            assert_eq!(total, 258);
            let back: Vec<u8> = ctx.eval("Array.from(new Uint8Array([104, 105]))").unwrap();
            assert_eq!(back, b"hi");
            let buffer: rquickjs::ArrayBuffer<'_> =
                ctx.eval("new Uint8Array([7, 8, 9]).buffer").unwrap();
            assert_eq!(buffer.len(), 3);
        });
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
