//! The diagnostics bundle's content (ADRs 0094, 0096, 0099): the core
//! collects, `nail-portability` redacts and writes.
//!
//! | Item | Entries |
//! | --- | --- |
//! | system-info | `system/info.json`: Nail version, protocol, OS / architecture, language, installation (desktop) |
//! | logs | `logs/nail.log` (this process's recent lines, [`crate::logs`]), `logs/plugins.log` |
//! | config | `config/<file>` for theme, layout, accounts and rules files that exist |
//! | account-status | `accounts/status.json`: protocol, state, last error per account |
//! | store-stats | `store/stats.json`: schema version, counts, encryption at rest |
//! | operation-queue | `store/operations.json`: kinds, states, attempts, errors (no content) |
//! | plugins | `plugins/plugins.json`: ids, versions, sources, state, last errors |
//! | mail-headers / mail-content | `mail/<n>-headers.txt` / `mail/<n>.eml` of the chosen messages (opt-in, not redacted) |
//!
//! The preview's entries are kept for a while: the bundle written for the
//! same options holds exactly what the preview showed, even when new log
//! lines arrived in between.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use nail_api::diagnostics::{DiagnosticsItem, DiagnosticsManifest, DiagnosticsOptions};
use nail_portability::diagnostics::{CollectedEntry, PreparedEntry, Redactor, prepare};
use serde_json::json;

use crate::error::Result;
use crate::service::{Service, lock};

/// How long a preview's entries are what the bundle writes.
const PREVIEW_TTL: Duration = Duration::from_mins(30);

/// A preview: its options, manifest, entries and when it was made.
type Preview = (
    DiagnosticsOptions,
    DiagnosticsManifest,
    Vec<PreparedEntry>,
    Instant,
);

/// The last preview.
#[derive(Default)]
pub(crate) struct DiagnosticsCache {
    last: Mutex<Option<Preview>>,
}

impl std::fmt::Debug for DiagnosticsCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DiagnosticsCache").finish_non_exhaustive()
    }
}

impl DiagnosticsCache {
    pub(crate) fn remember(
        &self,
        options: &DiagnosticsOptions,
        manifest: &DiagnosticsManifest,
        entries: &[PreparedEntry],
    ) {
        *lock(&self.last) = Some((
            options.clone(),
            manifest.clone(),
            entries.to_vec(),
            Instant::now(),
        ));
    }

    pub(crate) fn take(
        &self,
        options: &DiagnosticsOptions,
    ) -> Option<(DiagnosticsManifest, Vec<PreparedEntry>)> {
        let mut last = lock(&self.last);
        match last.as_ref() {
            Some((o, m, e, at)) if o == options && at.elapsed() < PREVIEW_TTL => {
                Some((m.clone(), e.clone()))
            }
            _ => {
                *last = None;
                None
            }
        }
    }
}

fn entry(name: &str, item: DiagnosticsItem, content: impl Into<Vec<u8>>) -> CollectedEntry {
    CollectedEntry {
        name: name.to_owned(),
        item,
        content: content.into(),
    }
}

fn pretty(value: &serde_json::Value) -> Vec<u8> {
    serde_json::to_vec_pretty(value).unwrap_or_default()
}

/// Collects the chosen items (blocking).
pub(crate) fn collect(
    service: &Service,
    options: &DiagnosticsOptions,
) -> Result<Vec<CollectedEntry>> {
    let store = service.store();
    let mut out = Vec::new();
    let wants = |item: DiagnosticsItem| options.items.contains(&item);
    if wants(DiagnosticsItem::SystemInfo) {
        let installation = service
            .inner
            .desktop
            .as_ref()
            .map(|host| serde_json::to_value(host.installation()).unwrap_or_default());
        out.push(entry(
            "system/info.json",
            DiagnosticsItem::SystemInfo,
            pretty(&json!({
                "nail_version": env!("CARGO_PKG_VERSION"),
                "protocol": nail_api::PROTOCOL_VERSION,
                "os": std::env::consts::OS,
                "arch": std::env::consts::ARCH,
                "family": std::env::consts::FAMILY,
                "language": service.inner.locale,
                "desktop_app": service.inner.desktop.is_some(),
                "installation": installation,
            })),
        ));
    }
    if wants(DiagnosticsItem::Logs) {
        let mut text = crate::logs::recent().join("\n");
        text.push('\n');
        out.push(entry("logs/nail.log", DiagnosticsItem::Logs, text));
        let mut plugin_lines = Vec::new();
        for plugin in store.plugins()? {
            for line in store.plugin_logs(Some(&plugin.id), None, 200)? {
                plugin_lines.push(format!(
                    "{} {} {} {}",
                    line.at,
                    plugin.id,
                    line.level.as_str(),
                    line.message
                ));
            }
        }
        if !plugin_lines.is_empty() {
            plugin_lines.push(String::new());
            out.push(entry(
                "logs/plugins.log",
                DiagnosticsItem::Logs,
                plugin_lines.join("\n"),
            ));
        }
    }
    if wants(DiagnosticsItem::Config) {
        for file in nail_api::customize::ConfigFile::ALL {
            if let Some(text) = service.config().file_text(file)? {
                out.push(entry(
                    &format!("config/{}", file.file_name()),
                    DiagnosticsItem::Config,
                    text,
                ));
            }
        }
    }
    if wants(DiagnosticsItem::AccountStatus) {
        let statuses: Vec<serde_json::Value> = service
            .account_statuses()
            .into_iter()
            .map(|s| {
                json!({
                    "account": s.account_id,
                    "state": s.state,
                    "error": s.error,
                    "message": s.message,
                })
            })
            .collect();
        let accounts: Vec<serde_json::Value> = service
            .inner
            .accounts
            .config()
            .accounts
            .iter()
            .map(|a| {
                json!({
                    "account": a.id,
                    "protocol": a.incoming.protocol(),
                    "enabled": a.enabled,
                })
            })
            .collect();
        out.push(entry(
            "accounts/status.json",
            DiagnosticsItem::AccountStatus,
            pretty(&json!({ "accounts": accounts, "statuses": statuses })),
        ));
    }
    if wants(DiagnosticsItem::StoreStats) {
        let accounts = store.list_accounts()?;
        let mut folders = 0usize;
        let mut messages = 0u64;
        for account in &accounts {
            let list = store.list_folders(&account.id)?;
            folders += list.len();
            messages += list.iter().map(|f| u64::from(f.total)).sum::<u64>();
        }
        let vault = service.inner.vault.status().ok();
        out.push(entry(
            "store/stats.json",
            DiagnosticsItem::StoreStats,
            pretty(&json!({
                "schema_version": store.schema_version()?,
                "accounts": accounts.len(),
                "folders": folders,
                "messages": messages,
                "encryption_at_rest": vault.map(|v| v.state.as_str().to_owned()),
            })),
        ));
    }
    if wants(DiagnosticsItem::OperationQueue) {
        let ops: Vec<serde_json::Value> = store
            .list_ops(None)?
            .into_iter()
            .map(|op| {
                json!({
                    "id": op.id.0,
                    "account": op.account_id,
                    "kind": op.operation.kind(),
                    "state": op.state,
                    "attempts": op.attempts,
                    "error": op.last_error,
                    "created_at": op.created_at,
                    "updated_at": op.updated_at,
                })
            })
            .collect();
        out.push(entry(
            "store/operations.json",
            DiagnosticsItem::OperationQueue,
            pretty(&json!({ "operations": ops })),
        ));
    }
    if wants(DiagnosticsItem::Plugins) {
        let plugins: Vec<serde_json::Value> = store
            .plugins()?
            .into_iter()
            .map(|p| {
                json!({
                    "id": p.id,
                    "version": p.manifest.version,
                    "enabled": p.enabled,
                    "dev": matches!(p.source, nail_api::plugins::PluginPackageSource::Dev { .. }),
                    "installed_at": p.installed_at,
                    "last_error": p.last_error.map(|e| e.message),
                })
            })
            .collect();
        out.push(entry(
            "plugins/plugins.json",
            DiagnosticsItem::Plugins,
            pretty(&json!({ "plugins": plugins })),
        ));
    }
    if options.includes_mail() {
        for (n, id) in options.message_ids.iter().enumerate() {
            let stored = store.get_message(id)?;
            let Some(hash) = stored.blob_hash else {
                continue;
            };
            let raw = service.blobs().get(&hash)?;
            if wants(DiagnosticsItem::MailContent) {
                out.push(entry(
                    &format!("mail/{}.eml", n + 1),
                    DiagnosticsItem::MailContent,
                    raw,
                ));
            } else if wants(DiagnosticsItem::MailHeaders) {
                let (fields, _) = crate::crypto::split_header(&raw);
                let mut header: Vec<u8> = Vec::new();
                for (_, line) in fields {
                    header.extend_from_slice(&line);
                }
                out.push(entry(
                    &format!("mail/{}-headers.txt", n + 1),
                    DiagnosticsItem::MailHeaders,
                    header,
                ));
            }
        }
    }
    Ok(out)
}

/// Collects, redacts and builds the manifest (blocking).
pub(crate) fn preview(
    service: &Service,
    options: &DiagnosticsOptions,
) -> Result<(DiagnosticsManifest, Vec<PreparedEntry>)> {
    let collected = collect(service, options)?;
    let user = std::env::var("USERNAME")
        .or_else(|_| std::env::var("USER"))
        .ok()
        .filter(|u| !u.trim().is_empty());
    let mut redactor = Redactor::new(options, user);
    let (manifest, entries) = prepare(
        collected,
        options,
        &mut redactor,
        &crate::rules::rfc3339(crate::engine::unix_now()),
        env!("CARGO_PKG_VERSION"),
    )
    .map_err(|e| e.to_api_error())?;
    Ok((manifest, entries))
}
