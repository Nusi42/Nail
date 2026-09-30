//! Installed plug-ins, install sources and previews, logs and storage
//! limits (plan M2d WP60: plug-in admin requests).
//!
//! These types travel in the protocol (snake_case JSON), not to plug-in
//! code.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::manifest::{PluginManifest, PluginManifestIssue};
use super::permission::{PluginGrant, PluginPermission};
use crate::ids::PluginId;
use crate::localized::LocalizedMessage;

/// Per-plug-in storage quota by default (bytes of keys + JSON values).
pub const DEFAULT_STORAGE_QUOTA: u64 = 5 * 1024 * 1024;
/// Longest storage key (bytes).
pub const MAX_STORAGE_KEY: usize = 256;
/// Largest single storage value (bytes of JSON).
pub const MAX_STORAGE_VALUE: usize = 1024 * 1024;
/// Log lines kept per plug-in (the oldest go first).
pub const MAX_LOG_LINES: u32 = 2000;
/// Longest log line (bytes; longer ones are cut).
pub const MAX_LOG_MESSAGE: usize = 8 * 1024;
/// Largest `.nailplugin` archive (bytes), and largest unpacked size.
pub const MAX_PACKAGE_BYTES: u64 = 20 * 1024 * 1024;
/// File extension of plug-in packages.
pub const PACKAGE_EXTENSION: &str = "nailplugin";

/// Where a plug-in comes from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum PluginPackageSource {
    /// A `.nailplugin` file on the machine running the core.
    File {
        /// Absolute path.
        path: String,
    },
    /// A `.nailplugin` downloaded by the core (`https:`; `http:` only for
    /// loopback addresses).
    Url {
        /// The URL.
        url: String,
    },
    /// An unpacked plug-in folder loaded by `nail plugin dev` (reloaded
    /// when its files change).
    Dev {
        /// Absolute path of the folder holding `manifest.json`.
        folder: String,
    },
}

impl PluginPackageSource {
    /// The store's `source_kind` value.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::File { .. } => "file",
            Self::Url { .. } => "url",
            Self::Dev { .. } => "dev",
        }
    }

    /// The path, URL or folder.
    pub fn location(&self) -> &str {
        match self {
            Self::File { path } => path,
            Self::Url { url } => url,
            Self::Dev { folder } => folder,
        }
    }

    /// Rebuilds a source from [`Self::kind`] and [`Self::location`].
    pub fn from_parts(kind: &str, location: impl Into<String>) -> Option<Self> {
        let location = location.into();
        Some(match kind {
            "file" => Self::File { path: location },
            "url" => Self::Url { url: location },
            "dev" => Self::Dev { folder: location },
            _ => return None,
        })
    }
}

/// Whether a plug-in runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum PluginRunState {
    /// Switched off by the user.
    Disabled,
    /// Enabled; its runtime is starting.
    Starting,
    /// Enabled and running.
    Running,
    /// Enabled but not running (the host has not started it, or it was
    /// stopped for a reason other than a crash).
    Stopped,
    /// Enabled but stopped after an error, a time-out or a limit
    /// ([`PluginInfo::error`] says why); enabling it again restarts it.
    Crashed,
    /// Written for an API version this Nail does not run.
    Incompatible,
}

/// Why a plug-in's runtime stopped ([`crate::Event::PluginStopped`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum PluginStopReason {
    /// The user disabled it.
    Disabled,
    /// It was uninstalled.
    Uninstalled,
    /// A new version or a dev reload replaces it.
    Reloaded,
    /// Nail is shutting down.
    Shutdown,
    /// It threw an uncaught error while starting or in a handler.
    Error,
    /// It used more than its CPU time budget (e.g. an endless loop).
    CpuBudget,
    /// It used more than its memory limit.
    MemoryLimit,
}

impl PluginStopReason {
    /// Whether the plug-in is flagged as crashed.
    pub fn is_crash(self) -> bool {
        matches!(self, Self::Error | Self::CpuBudget | Self::MemoryLimit)
    }

    /// All reasons.
    pub const ALL: [PluginStopReason; 7] = [
        Self::Disabled,
        Self::Uninstalled,
        Self::Reloaded,
        Self::Shutdown,
        Self::Error,
        Self::CpuBudget,
        Self::MemoryLimit,
    ];

    /// Fluent message id (`plugin-stopped-<reason>`, argument `$plugin`).
    pub fn message_id(self) -> &'static str {
        match self {
            Self::Disabled => "plugin-stopped-disabled",
            Self::Uninstalled => "plugin-stopped-uninstalled",
            Self::Reloaded => "plugin-stopped-reloaded",
            Self::Shutdown => "plugin-stopped-shutdown",
            Self::Error => "plugin-stopped-error",
            Self::CpuBudget => "plugin-stopped-cpu-budget",
            Self::MemoryLimit => "plugin-stopped-memory-limit",
        }
    }
}

/// How much storage a plug-in uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
pub struct PluginStorageUsage {
    /// Stored keys.
    pub keys: u32,
    /// Bytes used (keys + JSON values).
    pub used_bytes: u64,
    /// The quota.
    pub quota_bytes: u64,
}

/// An installed plug-in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct PluginInfo {
    /// Its id.
    pub id: PluginId,
    /// Its manifest; `name`, `description` and contribution titles are
    /// already resolved for the core's locale (no `%id%` texts left).
    pub manifest: PluginManifest,
    /// Where it was installed from.
    pub source: PluginPackageSource,
    /// Whether the user enabled it.
    pub enabled: bool,
    /// Whether it runs.
    pub state: PluginRunState,
    /// The granted permissions (a subset of the manifest's).
    pub grants: Vec<PluginGrant>,
    /// Its storage.
    pub storage: PluginStorageUsage,
    /// SHA-256 (hex) of the installed package; `null` for dev plug-ins.
    pub package_sha256: Option<String>,
    /// When it was installed (RFC 3339).
    pub installed_at: String,
    /// When it was last updated or reloaded (RFC 3339).
    pub updated_at: String,
    /// Why it crashed or failed to start (English, for logs).
    pub error: Option<String>,
    /// The same for users (ADR 0043).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub localized: Option<Box<LocalizedMessage>>,
}

impl PluginInfo {
    /// The declared permissions that are not granted.
    pub fn missing_grants(&self) -> Vec<&PluginPermission> {
        self.manifest
            .permissions
            .iter()
            .filter(|p| !self.grants.iter().any(|g| &g.permission == *p))
            .collect()
    }

    /// The grant of `permission`, if granted.
    pub fn grant(&self, permission: &PluginPermission) -> Option<&PluginGrant> {
        self.grants.iter().find(|g| &g.permission == permission)
    }
}

/// What installing a package would do (the install dialog).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct PluginInstallPreview {
    /// Where it comes from.
    pub source: PluginPackageSource,
    /// Its manifest (texts resolved); `null` when it cannot be read.
    pub manifest: Option<PluginManifest>,
    /// Problems; installing is refused while there are any.
    pub issues: Vec<PluginManifestIssue>,
    /// SHA-256 (hex) of the package; pass it to
    /// [`crate::Request::InstallPlugin`] so exactly this package is
    /// installed.
    pub package_sha256: String,
    /// Package size (bytes).
    pub size: u64,
    /// The version installed now, when this is an update (its grants are
    /// kept for permissions it still declares).
    pub installed_version: Option<String>,
}

/// The level of a log line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum PluginLogLevel {
    /// `nail.log.debug`.
    Debug,
    /// `nail.log.info` / `console.log`.
    Info,
    /// `nail.log.warn`.
    Warn,
    /// `nail.log.error`, uncaught errors, host errors.
    Error,
}

impl PluginLogLevel {
    /// The store's value.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Debug => "debug",
            Self::Info => "info",
            Self::Warn => "warn",
            Self::Error => "error",
        }
    }

    /// Parses [`Self::as_str`].
    pub fn parse(text: &str) -> Option<Self> {
        Some(match text {
            "debug" => Self::Debug,
            "info" => Self::Info,
            "warn" => Self::Warn,
            "error" => Self::Error,
            _ => return None,
        })
    }
}

/// Who wrote a log line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum PluginLogSource {
    /// The plug-in (`nail.log`, `console`).
    Plugin,
    /// The host (start, stop, crashes, refused trees).
    Host,
    /// An API call and its outcome (`nail plugin dev` shows them).
    Api,
}

impl PluginLogSource {
    /// The store's value.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Plugin => "plugin",
            Self::Host => "host",
            Self::Api => "api",
        }
    }

    /// Parses [`Self::as_str`].
    pub fn parse(text: &str) -> Option<Self> {
        Some(match text {
            "plugin" => Self::Plugin,
            "host" => Self::Host,
            "api" => Self::Api,
            _ => return None,
        })
    }
}

/// A log line of a plug-in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct PluginLogEntry {
    /// Increasing sequence number (across plug-ins).
    pub seq: u64,
    /// The plug-in.
    pub plugin_id: PluginId,
    /// When (RFC 3339, milliseconds).
    pub at: String,
    /// The level.
    pub level: PluginLogLevel,
    /// Who wrote it.
    pub source: PluginLogSource,
    /// The text (the plug-in's own words, or an English host diagnostic).
    pub message: String,
    /// For host lines: the user-facing text (ADR 0043).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub localized: Option<Box<LocalizedMessage>>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn sources_round_trip() {
        for source in [
            PluginPackageSource::File {
                path: "C:/p/x.nailplugin".into(),
            },
            PluginPackageSource::Url {
                url: "https://example.com/x.nailplugin".into(),
            },
            PluginPackageSource::Dev {
                folder: "C:/dev/x".into(),
            },
        ] {
            assert_eq!(
                PluginPackageSource::from_parts(source.kind(), source.location()),
                Some(source.clone())
            );
            let value = serde_json::to_value(&source).unwrap();
            assert_eq!(value["kind"], source.kind());
            assert_eq!(
                serde_json::from_value::<PluginPackageSource>(value).unwrap(),
                source
            );
        }
        assert_eq!(PluginPackageSource::from_parts("ftp", "x"), None);
    }

    #[test]
    fn levels_and_sources() {
        for level in [
            PluginLogLevel::Debug,
            PluginLogLevel::Info,
            PluginLogLevel::Warn,
            PluginLogLevel::Error,
        ] {
            assert_eq!(PluginLogLevel::parse(level.as_str()), Some(level));
            assert_eq!(serde_json::to_value(level).unwrap(), json!(level.as_str()));
        }
        assert!(PluginLogLevel::Debug < PluginLogLevel::Error);
        for source in [
            PluginLogSource::Plugin,
            PluginLogSource::Host,
            PluginLogSource::Api,
        ] {
            assert_eq!(PluginLogSource::parse(source.as_str()), Some(source));
        }
        assert_eq!(PluginLogLevel::parse("fatal"), None);
    }

    #[test]
    fn stop_reasons() {
        assert!(PluginStopReason::CpuBudget.is_crash());
        assert!(PluginStopReason::MemoryLimit.is_crash());
        assert!(PluginStopReason::Error.is_crash());
        assert!(!PluginStopReason::Disabled.is_crash());
        assert_eq!(
            serde_json::to_value(PluginStopReason::CpuBudget).unwrap(),
            json!("cpu-budget")
        );
        for reason in PluginStopReason::ALL {
            assert!(crate::localized::is_message_id(reason.message_id()));
        }
    }

    #[test]
    fn missing_grants() {
        let manifest: PluginManifest = serde_json::from_value(json!({
            "id": "x", "name": "X", "version": "1.0.0", "apiVersion": "1", "main": "i.js",
            "permissions": ["mail.read", "storage"]
        }))
        .unwrap();
        let info = PluginInfo {
            id: "x".into(),
            manifest,
            source: PluginPackageSource::Dev {
                folder: "/x".into(),
            },
            enabled: true,
            state: PluginRunState::Stopped,
            grants: vec![PluginGrant::all(PluginPermission::Storage)],
            storage: PluginStorageUsage::default(),
            package_sha256: None,
            installed_at: "2026-09-30T00:00:00Z".into(),
            updated_at: "2026-09-30T00:00:00Z".into(),
            error: None,
            localized: None,
        };
        assert_eq!(info.missing_grants(), [&PluginPermission::MailRead]);
        assert!(info.grant(&PluginPermission::Storage).is_some());
        let value = serde_json::to_value(&info).unwrap();
        assert_eq!(value["state"], "stopped");
        assert_eq!(value["source"], json!({"kind": "dev", "folder": "/x"}));
    }
}
