//! Windows integration and distribution (M4b, ADR 0087; design.md
//! "Windows installer & distribution"; plan section 9, wave M4b).
//!
//! What the desktop app tells the UI about itself and lets the user
//! change, all desktop only (a remote-UI session gets `forbidden`, `nail
//! serve` answers `unsupported` because it has no [`DesktopHost`]):
//!
//! * **Installation facts** ([`InstallationInfo`]): version, installer
//!   ([`InstallKind`] NSIS per user / MSI per machine / portable /
//!   development), scope, architecture, whether the in-app updater is
//!   disabled and why ([`UpdateDisabledReason`]: MSI-managed, group
//!   policy, portable, development build, no update key in the build), a
//!   channel forced by policy, whether the Simple MAPI provider is
//!   installed. The shell reads them from the registry values the
//!   installers write ([`registry`]).
//! * **Updates** ([`UpdateSettings`]: channel stable / beta, automatic
//!   checks and downloads; [`UpdateStatus`] with the [`UpdateState`]
//!   machine: checking, up to date, available with version and release
//!   notes, downloading with progress, ready to install on restart,
//!   failed, disabled by policy). The Tauri updater runs in the shell; the
//!   core stores the settings and composes the status.
//! * **Tray and startup** ([`DesktopSettings`]: close to tray, start
//!   hidden, start with Windows, the local endpoint for `nail plugin dev`).
//! * **Default apps** ([`DefaultAppsStatus`]): which of `mailto:`,
//!   `webcal:`, `webcals:`, `.eml`, `.ics`, `.vcf` Nail is registered for
//!   and whether it is the default where Windows tells (Windows never lets
//!   an app make itself the default: the UI opens Settings → Default apps,
//!   [`default_apps_settings_uri`]); the Simple MAPI client registration.
//! * **First run** ([`FirstRunState`]): whether the first-run screen
//!   (default apps, start with Windows, import offer) was completed.
//!
//! The shell implements [`DesktopHost`] and passes it to the core
//! (`ServiceOptions::desktop`); the core serves every request of this
//! module from its store plus the host. Simple MAPI ("Send to → Mail
//! recipient") is in [`crate::mapi`].

use std::fmt;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::engine::BoxFuture;
use crate::error::ApiError;
use crate::ids::PrintJobId;
use crate::print::PrintJob;

// ---------------------------------------------------------------------------
// Installation facts

/// Which package installed this copy of Nail.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum InstallKind {
    /// `Nail-setup.exe` (NSIS; also what the winget package wraps).
    Nsis,
    /// `Nail.msi` (WiX, per machine, managed by IT).
    Msi,
    /// The portable zip (a `portable` marker beside the executable).
    Portable,
    /// Not installed: a development build or a copied executable.
    Development,
}

impl InstallKind {
    /// Every kind.
    pub const ALL: [InstallKind; 4] = [Self::Nsis, Self::Msi, Self::Portable, Self::Development];

    /// The wire / registry name (`nsis`, `msi`, …).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Nsis => "nsis",
            Self::Msi => "msi",
            Self::Portable => "portable",
            Self::Development => "development",
        }
    }

    /// Parses [`Self::as_str`] (the registry value `Installer`).
    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.as_str() == text)
    }

    /// Fluent id of its display name (`install-kind-*`, core-desktop.ftl).
    pub fn message_id(self) -> String {
        format!("install-kind-{}", self.as_str())
    }
}

/// Per user (`%LOCALAPPDATA%\Programs\Nail`, `HKCU`) or per machine
/// (`Program Files`, `HKLM`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum InstallScope {
    /// Installed for the current user only.
    PerUser,
    /// Installed for every user of the machine.
    PerMachine,
}

impl InstallScope {
    /// Both scopes.
    pub const ALL: [InstallScope; 2] = [Self::PerUser, Self::PerMachine];

    /// The wire / registry name (`per-user`, `per-machine`).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PerUser => "per-user",
            Self::PerMachine => "per-machine",
        }
    }

    /// Parses [`Self::as_str`] (the registry value `Scope`).
    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.as_str() == text)
    }

    /// Fluent id of its display name (`install-scope-*`).
    pub fn message_id(self) -> String {
        format!("install-scope-{}", self.as_str())
    }
}

/// A Windows processor architecture Nail ships for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum WindowsArch {
    /// 64-bit x86.
    X64,
    /// 32-bit x86 (only the Simple MAPI DLL, for 32-bit Office).
    X86,
    /// 64-bit ARM.
    Arm64,
}

impl WindowsArch {
    /// Every architecture.
    pub const ALL: [WindowsArch; 3] = [Self::X64, Self::X86, Self::Arm64];

    /// The name used in paths and file names (`x64`, `x86`, `arm64`).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::X64 => "x64",
            Self::X86 => "x86",
            Self::Arm64 => "arm64",
        }
    }

    /// The architecture this binary was built for (`None` for targets Nail
    /// does not ship).
    pub fn current() -> Option<Self> {
        if cfg!(target_arch = "x86_64") {
            Some(Self::X64)
        } else if cfg!(target_arch = "x86") {
            Some(Self::X86)
        } else if cfg!(target_arch = "aarch64") {
            Some(Self::Arm64)
        } else {
            None
        }
    }
}

/// Why the in-app updater does not run. The UI shows the reason and hides
/// "check now".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum UpdateDisabledReason {
    /// Installed by the MSI (its `DISABLEUPDATES` property, on by
    /// default): IT deploys updates.
    MsiManaged,
    /// `DisableUpdates` under `Software\Policies\Nail` (group policy,
    /// Intune).
    Policy,
    /// The portable zip updates by hand.
    Portable,
    /// A development build (not installed).
    Development,
    /// This build has no update endpoint or update key.
    NotConfigured,
}

impl UpdateDisabledReason {
    /// Every reason.
    pub const ALL: [UpdateDisabledReason; 5] = [
        Self::MsiManaged,
        Self::Policy,
        Self::Portable,
        Self::Development,
        Self::NotConfigured,
    ];

    /// Fluent id of the explanation (`update-disabled-*`).
    pub fn message_id(self) -> &'static str {
        match self {
            Self::MsiManaged => "update-disabled-msi-managed",
            Self::Policy => "update-disabled-policy",
            Self::Portable => "update-disabled-portable",
            Self::Development => "update-disabled-development",
            Self::NotConfigured => "update-disabled-not-configured",
        }
    }
}

/// Facts about this installation for the UI ("About", update settings,
/// default apps). Answer of [`crate::Request::GetInstallation`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct InstallationInfo {
    /// The app's version (semver, e.g. `1.0.0-beta.2`).
    pub version: String,
    /// Which package installed it. MSI-managed = `msi`.
    pub kind: InstallKind,
    /// Per user / per machine; `null` for portable and development copies.
    pub scope: Option<InstallScope>,
    /// The architecture of the running app.
    pub architecture: WindowsArch,
    /// The installation directory; `null` when unknown.
    pub install_dir: Option<String>,
    /// Why the in-app updater is off; `null` = it runs.
    pub updates_disabled: Option<UpdateDisabledReason>,
    /// A channel forced by policy (`UpdateChannel` under
    /// `Software\Policies\Nail`); the user cannot change it.
    pub forced_channel: Option<UpdateChannel>,
    /// Whether the Simple MAPI provider was installed (optional
    /// component; per-machine registration).
    pub mapi_installed: bool,
    /// The WebView2 runtime's version, when known.
    pub webview_version: Option<String>,
}

impl InstallationInfo {
    /// Whether IT manages this installation (installed by the MSI).
    pub fn is_msi_managed(&self) -> bool {
        self.kind == InstallKind::Msi
    }
}

// ---------------------------------------------------------------------------
// Updates

/// Which releases the updater offers.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum UpdateChannel {
    /// Releases only.
    #[default]
    Stable,
    /// Releases and pre-releases (betas).
    Beta,
}

impl UpdateChannel {
    /// Both channels.
    pub const ALL: [UpdateChannel; 2] = [Self::Stable, Self::Beta];

    /// The wire / registry / manifest name (`stable`, `beta`).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Stable => "stable",
            Self::Beta => "beta",
        }
    }

    /// Parses [`Self::as_str`] (case-insensitive; the policy value
    /// `UpdateChannel`).
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        Self::ALL
            .into_iter()
            .find(|c| c.as_str().eq_ignore_ascii_case(text))
    }

    /// Fluent id of its display name (`update-channel-*`).
    pub fn message_id(self) -> String {
        format!("update-channel-{}", self.as_str())
    }

    /// The update manifest of this channel below the release host's base
    /// URL: `<base>/<channel>/latest.json` (WP112 publishes them, the
    /// shell's updater reads them).
    pub fn manifest_path(self) -> String {
        format!("{}/{UPDATE_MANIFEST_FILE}", self.as_str())
    }
}

/// File name of a channel's signed update manifest (Tauri updater format).
pub const UPDATE_MANIFEST_FILE: &str = "latest.json";

/// How often the updater checks while the app runs (seconds; plus one
/// check shortly after start).
pub const UPDATE_CHECK_INTERVAL_SECS: u64 = 24 * 60 * 60;

/// The user's update settings (stored by the core).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct UpdateSettings {
    /// Stable or beta.
    #[serde(default)]
    pub channel: UpdateChannel,
    /// Check on start and daily ([`UPDATE_CHECK_INTERVAL_SECS`]).
    #[serde(default = "default_true")]
    pub auto_check: bool,
    /// Download a found update in the background (else the UI offers
    /// "Download").
    #[serde(default = "default_true")]
    pub auto_download: bool,
}

impl Default for UpdateSettings {
    fn default() -> Self {
        Self {
            channel: UpdateChannel::Stable,
            auto_check: true,
            auto_download: true,
        }
    }
}

/// A release the updater found.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct UpdateInfo {
    /// Its version (semver).
    pub version: String,
    /// The channel it came from.
    pub channel: UpdateChannel,
    /// Publication time (RFC 3339), when the manifest says.
    pub published_at: Option<String>,
    /// Release notes from the manifest (Markdown; untrusted text: the UI
    /// renders it as text / sanitized Markdown, never as HTML).
    pub notes: Option<String>,
    /// Download size in bytes, when known.
    pub size_bytes: Option<u64>,
}

/// Why an update step failed (the UI shows `update-failure-*`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum UpdateFailure {
    /// The update host could not be reached.
    Network,
    /// The manifest is malformed or names no package for this system.
    Manifest,
    /// A signature did not verify (manifest or package): nothing is
    /// installed.
    Signature,
    /// The download broke off or the disk is full.
    Download,
    /// The installer failed or was cancelled.
    Install,
}

impl UpdateFailure {
    /// Every failure.
    pub const ALL: [UpdateFailure; 5] = [
        Self::Network,
        Self::Manifest,
        Self::Signature,
        Self::Download,
        Self::Install,
    ];

    /// Fluent id (`update-failure-*`).
    pub fn message_id(self) -> &'static str {
        match self {
            Self::Network => "update-failure-network",
            Self::Manifest => "update-failure-manifest",
            Self::Signature => "update-failure-signature",
            Self::Download => "update-failure-download",
            Self::Install => "update-failure-install",
        }
    }
}

/// Where the updater is (tagged by `state`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum UpdateState {
    /// Nothing checked yet in this run.
    Idle,
    /// Asking the update host.
    Checking,
    /// The newest release of the channel is installed.
    UpToDate,
    /// A newer release exists (not downloaded yet: `auto_download` off,
    /// or about to start).
    Available {
        /// The release.
        update: UpdateInfo,
    },
    /// Downloading in the background.
    Downloading {
        /// The release.
        update: UpdateInfo,
        /// Bytes so far.
        downloaded_bytes: u64,
        /// Total bytes, when the server says.
        total_bytes: Option<u64>,
    },
    /// Downloaded and verified: installs when Nail restarts (the UI offers
    /// "Restart to install", [`crate::Request::InstallUpdate`]).
    ReadyToInstall {
        /// The release.
        update: UpdateInfo,
    },
    /// The installer runs (Nail is about to quit).
    Installing {
        /// The release.
        update: UpdateInfo,
    },
    /// The last step failed; the next check starts over.
    Failed {
        /// What failed.
        failure: UpdateFailure,
        /// English detail for logs (never shown as the message).
        detail: Option<String>,
        /// The release concerned, if one was found.
        update: Option<UpdateInfo>,
    },
    /// The updater is off ([`InstallationInfo::updates_disabled`]).
    Disabled {
        /// Why.
        reason: UpdateDisabledReason,
    },
}

impl UpdateState {
    /// The release this state is about, if any.
    pub fn update(&self) -> Option<&UpdateInfo> {
        match self {
            Self::Available { update }
            | Self::Downloading { update, .. }
            | Self::ReadyToInstall { update }
            | Self::Installing { update } => Some(update),
            Self::Failed { update, .. } => update.as_ref(),
            Self::Idle | Self::Checking | Self::UpToDate | Self::Disabled { .. } => None,
        }
    }

    /// Whether [`crate::Request::CheckForUpdates`] may start a check now
    /// (not while checking, downloading or installing, never when off).
    pub fn can_check(&self) -> bool {
        matches!(
            self,
            Self::Idle
                | Self::UpToDate
                | Self::Available { .. }
                | Self::ReadyToInstall { .. }
                | Self::Failed { .. }
        )
    }

    /// Whether [`crate::Request::DownloadUpdate`] applies.
    pub fn can_download(&self) -> bool {
        matches!(self, Self::Available { .. })
    }

    /// Whether [`crate::Request::InstallUpdate`] applies.
    pub fn can_install(&self) -> bool {
        matches!(self, Self::ReadyToInstall { .. })
    }
}

/// Settings and state of the updater. Answer of
/// [`crate::Request::GetUpdateStatus`] and the update requests, payload
/// of [`crate::Event::UpdateStatusChanged`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct UpdateStatus {
    /// The running version.
    pub current_version: String,
    /// The effective settings (a forced channel replaces the stored one).
    pub settings: UpdateSettings,
    /// The channel policy forces, if any (the UI locks the choice).
    pub forced_channel: Option<UpdateChannel>,
    /// Where the updater is; [`UpdateState::Disabled`] whenever
    /// [`InstallationInfo::updates_disabled`] is set.
    pub state: UpdateState,
    /// When the last check finished (RFC 3339).
    pub last_checked_at: Option<String>,
}

impl UpdateStatus {
    /// Composes the status from the stored settings, the installation
    /// facts and the host's updater state: the policy wins over both (a
    /// disabled updater is [`UpdateState::Disabled`] whatever the host
    /// reports; a forced channel replaces the stored one).
    pub fn compose(
        stored: UpdateSettings,
        installation: &InstallationInfo,
        host: HostUpdateState,
    ) -> Self {
        let mut settings = stored;
        if let Some(channel) = installation.forced_channel {
            settings.channel = channel;
        }
        let state = match installation.updates_disabled {
            Some(reason) => UpdateState::Disabled { reason },
            None => host.state,
        };
        Self {
            current_version: installation.version.clone(),
            settings,
            forced_channel: installation.forced_channel,
            state,
            last_checked_at: host.last_checked_at,
        }
    }
}

/// What the shell's updater reports to the core.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostUpdateState {
    /// The updater's state.
    pub state: UpdateState,
    /// When the last check finished (RFC 3339).
    pub last_checked_at: Option<String>,
}

impl Default for HostUpdateState {
    fn default() -> Self {
        Self {
            state: UpdateState::Idle,
            last_checked_at: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Tray and startup

/// The desktop app's own settings (stored by the core; `start_with_windows`
/// is the per-user Run key, which the shell reads and writes).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct DesktopSettings {
    /// Closing the main window keeps Nail running in the tray (sync,
    /// rules, reminders and MCP go on); "Quit" in the tray menu ends it.
    #[serde(default = "default_true")]
    pub close_to_tray: bool,
    /// When started with Windows, start in the tray without a window.
    #[serde(default = "default_true")]
    pub start_hidden: bool,
    /// Start with Windows (the per-user Run key, value [`registry::RUN_VALUE`]).
    #[serde(default)]
    pub start_with_windows: bool,
    /// Run the local endpoint `nail plugin dev` connects to (loopback
    /// only, token in `ui-endpoint.json`; off by default).
    #[serde(default)]
    pub plugin_dev_endpoint: bool,
}

impl Default for DesktopSettings {
    fn default() -> Self {
        Self {
            close_to_tray: true,
            start_hidden: true,
            start_with_windows: false,
            plugin_dev_endpoint: false,
        }
    }
}

/// The command-line flag the Run key passes ("Start with Windows"); the
/// app then honours [`DesktopSettings::start_hidden`].
pub const AUTOSTART_ARG: &str = "--autostart";

// ---------------------------------------------------------------------------
// Default apps

/// A URL scheme or file type Nail registers for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum DefaultAppTarget {
    /// `mailto:` links.
    Mailto,
    /// `webcal:` calendar links.
    Webcal,
    /// `webcals:` calendar links.
    Webcals,
    /// `.eml` messages.
    Eml,
    /// `.ics` calendars.
    Ics,
    /// `.vcf` contacts.
    Vcf,
}

impl DefaultAppTarget {
    /// Every target, in the order the UI lists them.
    pub const ALL: [DefaultAppTarget; 6] = [
        Self::Mailto,
        Self::Webcal,
        Self::Webcals,
        Self::Eml,
        Self::Ics,
        Self::Vcf,
    ];

    /// How Windows names it: the scheme (`mailto`) or the extension with
    /// its dot (`.eml`).
    pub fn windows_name(self) -> &'static str {
        match self {
            Self::Mailto => "mailto",
            Self::Webcal => "webcal",
            Self::Webcals => "webcals",
            Self::Eml => ".eml",
            Self::Ics => ".ics",
            Self::Vcf => ".vcf",
        }
    }

    /// Whether it is a URL scheme (else a file type).
    pub fn is_scheme(self) -> bool {
        matches!(self, Self::Mailto | Self::Webcal | Self::Webcals)
    }

    /// Fluent id of its label (`default-app-*`).
    pub fn message_id(self) -> &'static str {
        match self {
            Self::Mailto => "default-app-mailto",
            Self::Webcal => "default-app-webcal",
            Self::Webcals => "default-app-webcals",
            Self::Eml => "default-app-eml",
            Self::Ics => "default-app-ics",
            Self::Vcf => "default-app-vcf",
        }
    }
}

/// Whether Nail is the default handler.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum DefaultAppState {
    /// Nail is the default.
    Default,
    /// Another app is (or none is chosen).
    Other,
    /// Windows does not tell.
    Unknown,
}

impl DefaultAppState {
    /// Every state.
    pub const ALL: [DefaultAppState; 3] = [Self::Default, Self::Other, Self::Unknown];

    /// Fluent id (`default-app-state-*`).
    pub fn message_id(self) -> &'static str {
        match self {
            Self::Default => "default-app-state-default",
            Self::Other => "default-app-state-other",
            Self::Unknown => "default-app-state-unknown",
        }
    }
}

/// One scheme or file type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct DefaultAppAssociation {
    /// The scheme or type.
    pub target: DefaultAppTarget,
    /// Nail is registered for it (Capabilities / "Open with").
    pub registered: bool,
    /// Whether Nail is the default.
    pub state: DefaultAppState,
}

/// Nail as the system's Simple MAPI mail client.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct MapiClientStatus {
    /// `Software\Clients\Mail\Nail` exists with the DLL paths.
    pub registered: bool,
    /// Whether Nail is the default mail client for Simple MAPI
    /// (`Software\Clients\Mail` default value, per user before per
    /// machine).
    pub state: DefaultAppState,
}

/// Default-app registration. Answer of [`crate::Request::GetDefaultApps`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct DefaultAppsStatus {
    /// Nail is listed under `RegisteredApplications` (so it appears in
    /// Settings → Default apps).
    pub registered: bool,
    /// One entry per [`DefaultAppTarget::ALL`], in that order.
    pub associations: Vec<DefaultAppAssociation>,
    /// The Simple MAPI client.
    pub mapi: MapiClientStatus,
}

impl DefaultAppsStatus {
    /// Nothing registered (development builds).
    pub fn unregistered() -> Self {
        Self {
            registered: false,
            associations: DefaultAppTarget::ALL
                .into_iter()
                .map(|target| DefaultAppAssociation {
                    target,
                    registered: false,
                    state: DefaultAppState::Unknown,
                })
                .collect(),
            mapi: MapiClientStatus {
                registered: false,
                state: DefaultAppState::Unknown,
            },
        }
    }

    /// Whether Nail is the default for `mailto:` (what the first-run screen
    /// asks about).
    pub fn is_default_mail_app(&self) -> bool {
        self.associations
            .iter()
            .any(|a| a.target == DefaultAppTarget::Mailto && a.state == DefaultAppState::Default)
    }
}

/// The application name under `RegisteredApplications` and in the
/// Settings URI.
pub const REGISTERED_APP_NAME: &str = "Nail";

/// The Settings page where the user makes Nail the default
/// (`ms-settings:defaultapps?registeredAppUser=Nail` for per-user
/// installs, `registeredAppMachine` for per-machine ones; the plain page
/// otherwise).
pub fn default_apps_settings_uri(scope: Option<InstallScope>) -> String {
    match scope {
        Some(InstallScope::PerUser) => {
            format!("ms-settings:defaultapps?registeredAppUser={REGISTERED_APP_NAME}")
        }
        Some(InstallScope::PerMachine) => {
            format!("ms-settings:defaultapps?registeredAppMachine={REGISTERED_APP_NAME}")
        }
        None => "ms-settings:defaultapps".to_owned(),
    }
}

// ---------------------------------------------------------------------------
// First run

/// Whether the first-run screen was completed (stored by the core).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct FirstRunState {
    /// The user finished or skipped the first-run screen.
    pub completed: bool,
    /// When (RFC 3339).
    pub completed_at: Option<String>,
    /// The app version that showed it.
    pub completed_version: Option<String>,
}

// ---------------------------------------------------------------------------
// `nail plugin dev` endpoint

/// The file in the data directory that tells `nail plugin dev` where the
/// running Nail listens (`nail serve` and, with
/// [`DesktopSettings::plugin_dev_endpoint`], the desktop app write it;
/// removed on exit).
pub const UI_ENDPOINT_FILE: &str = "ui-endpoint.json";

/// The content of [`UI_ENDPOINT_FILE`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct UiEndpointFile {
    /// The base URL (`http://127.0.0.1:<port>/`); the WebSocket is
    /// `/api/ws` below it.
    pub url: String,
    /// The process that wrote it.
    pub pid: u32,
    /// The desktop endpoint's token, sent as `Authorization: Bearer
    /// <token>` on the WebSocket upgrade; `null` for `nail serve` (whose
    /// local UI needs none).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub token: Option<String>,
}

/// Whether the desktop's `nail plugin dev` endpoint serves a request: only
/// loading a development folder. Everything else is `forbidden` there.
pub fn dev_endpoint_allows(request: &crate::Request) -> bool {
    matches!(request, crate::Request::LoadDevPlugin { .. })
}

/// Whether the desktop's `nail plugin dev` endpoint forwards an event: only
/// plug-in log lines and plug-in list changes.
pub fn dev_endpoint_forwards(event: &crate::Event) -> bool {
    matches!(
        event,
        crate::Event::PluginLogLine { .. } | crate::Event::PluginsChanged
    )
}

// ---------------------------------------------------------------------------
// Registry facts written by the installers (read by the shell)

/// Registry keys and values the installers write and the shell reads
/// (WP112 writes, WP113 reads). Paths are relative to `HKCU` (per-user
/// install) or `HKLM` (per-machine install; the policy key in either
/// hive).
pub mod registry {
    /// Installation facts: `HKCU` / `HKLM` `Software\Nail\Install`.
    pub const INSTALL_KEY: &str = r"Software\Nail\Install";
    /// `REG_SZ` [`super::InstallKind::as_str`] (`nsis`, `msi`).
    pub const INSTALLER_VALUE: &str = "Installer";
    /// `REG_SZ` [`super::InstallScope::as_str`].
    pub const SCOPE_VALUE: &str = "Scope";
    /// `REG_SZ` installation directory.
    pub const INSTALL_DIR_VALUE: &str = "InstallDir";
    /// `REG_SZ` installed version.
    pub const VERSION_VALUE: &str = "Version";
    /// `REG_DWORD` 1 = the MSI's `DISABLEUPDATES` was set (the default for
    /// the MSI).
    pub const DISABLE_UPDATES_VALUE: &str = "DisableUpdates";
    /// `REG_DWORD` 1 = the Simple MAPI component was installed.
    pub const MAPI_VALUE: &str = "Mapi";
    /// Group policy / Intune: `HKLM` or `HKCU` `Software\Policies\Nail`.
    pub const POLICY_KEY: &str = r"Software\Policies\Nail";
    /// `REG_DWORD` 1 under [`POLICY_KEY`]: no in-app updates.
    pub const POLICY_DISABLE_UPDATES_VALUE: &str = "DisableUpdates";
    /// `REG_SZ` `stable` / `beta` under [`POLICY_KEY`]: forced channel.
    pub const POLICY_UPDATE_CHANNEL_VALUE: &str = "UpdateChannel";
    /// The per-user Run key ("Start with Windows").
    pub const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
    /// Nail's value under [`RUN_KEY`]: `"<Nail.exe>" --autostart`.
    pub const RUN_VALUE: &str = "Nail";
    /// `RegisteredApplications` (value [`super::REGISTERED_APP_NAME`]).
    pub const REGISTERED_APPLICATIONS_KEY: &str = r"Software\RegisteredApplications";
    /// Nail's Capabilities key (what `RegisteredApplications` points to).
    pub const CAPABILITIES_KEY: &str = r"Software\Nail\Capabilities";
}

// ---------------------------------------------------------------------------
// The shell's side (Rust only)

/// What changed on the host's side; the core broadcasts the matching
/// event ([`crate::Event::UpdateStatusChanged`] with the composed status,
/// [`crate::Event::DesktopSettingsChanged`],
/// [`crate::Event::DefaultAppsChanged`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DesktopChange {
    /// The updater's state or progress changed.
    Update,
    /// The Run key or the dev endpoint changed outside a settings request.
    Settings,
    /// Default-app registration may have changed (e.g. the user came back
    /// from Settings).
    DefaultApps,
    /// The user moved, resized, focused or closed a pop-out window (M5c,
    /// ADR 0116); the core updates its pop-out list, remembers the geometry
    /// and sends [`crate::Event::PopOutChanged`].
    PopOut(crate::windows::PopOutReport),
}

/// The callback the core hands the host in [`DesktopHost::attach`].
pub type DesktopNotifier = Arc<dyn Fn(DesktopChange) + Send + Sync>;

/// The desktop shell's side of this module, implemented by
/// `app/src-tauri` and passed to the core (`ServiceOptions::desktop`).
/// Methods are quick (registry reads, state snapshots) and never block on
/// the network: long work (checks, downloads, starting the dev endpoint)
/// runs in the host's own tasks, which report through the notifier.
pub trait DesktopHost: Send + Sync + fmt::Debug {
    /// Called once when the core opens; keep the notifier for changes.
    fn attach(&self, notify: DesktopNotifier);
    /// The installation facts.
    fn installation(&self) -> InstallationInfo;
    /// The updater's state.
    fn update_state(&self) -> HostUpdateState;
    /// The stored update settings (at start and after every change).
    fn apply_update_settings(&self, settings: &UpdateSettings);
    /// Start a check now. `conflict` when one runs.
    fn check_for_updates(&self) -> Result<(), ApiError>;
    /// Start downloading the available update. `conflict` without one.
    fn download_update(&self) -> Result<(), ApiError>;
    /// Quit and run the downloaded update's installer, then restart.
    /// `conflict` unless ready.
    fn install_update(&self) -> Result<(), ApiError>;
    /// The Run key's state (`None` = unknown; the stored value stands).
    fn start_with_windows(&self) -> Option<bool>;
    /// Apply settings (Run key, dev endpoint); called after the core
    /// stored them and once at start.
    fn apply_desktop_settings(&self, settings: &DesktopSettings) -> Result<(), ApiError>;
    /// The running dev endpoint's URL.
    fn dev_endpoint_url(&self) -> Option<String>;
    /// Default-app registration.
    fn default_apps(&self) -> DefaultAppsStatus;
    /// Open Settings → Default apps ([`default_apps_settings_uri`]).
    fn open_default_apps_settings(&self) -> Result<(), ApiError>;
    /// Whether "start with Windows" is set for every user by the machine
    /// (the MSI's `HKLM` Run value), so the user cannot turn it off.
    fn start_with_windows_managed(&self) -> bool {
        false
    }

    // --- M4c (ADR 0094); defaults answer `unsupported` so a host without
    // them still compiles ---

    /// Shows the system's open / save / folder dialog and returns the
    /// chosen path (`None` = cancelled). May wait for the user.
    fn pick_path<'a>(
        &'a self,
        pick: &'a PathPick,
    ) -> BoxFuture<'a, Result<Option<String>, ApiError>> {
        let _ = pick;
        Box::pin(async { Err(desktop_unsupported()) })
    }

    /// Loads the print view of `job` ([`crate::print::PrintJob::view`]) in a
    /// hidden window, waits for [`DesktopHost::print_view_ready`] and
    /// writes it to `path` as PDF (WebView2 `PrintToPdf`). Returns the
    /// file's size.
    fn print_to_pdf<'a>(
        &'a self,
        job: &'a PrintJob,
        path: &'a str,
    ) -> BoxFuture<'a, Result<u64, ApiError>> {
        let _ = (job, path);
        Box::pin(async { Err(desktop_unsupported()) })
    }

    /// The print view of a job reported it is laid out
    /// ([`crate::Request::PrintViewReady`]).
    fn print_view_ready(&self, job_id: &PrintJobId) {
        let _ = job_id;
    }

    // --- M5c (ADR 0116): pop-out windows; defaults answer `unsupported` ---

    /// Opens a window as its own OS window: a new webview window labelled
    /// `open.label` that loads the UI at `open.route` and talks to the same
    /// core, at `open.geometry` (screen coordinates) when given. Report what
    /// the user does with it through the notifier
    /// ([`DesktopChange::PopOut`]).
    fn open_pop_out(&self, open: &crate::windows::PopOutOpen) -> Result<(), ApiError> {
        let _ = open;
        Err(desktop_unsupported())
    }

    /// Closes a pop-out's OS window (`ReturnPopOut` / `ClosePopOut`); the
    /// core has already removed it from its list.
    fn close_pop_out(&self, window: &str) -> Result<(), ApiError> {
        let _ = window;
        Err(desktop_unsupported())
    }

    /// Brings a pop-out's OS window to the front.
    fn focus_pop_out(&self, window: &str) -> Result<(), ApiError> {
        let _ = window;
        Err(desktop_unsupported())
    }
}

fn desktop_unsupported() -> ApiError {
    ApiError::new(
        crate::error::ErrorCode::Unsupported,
        "not supported by this desktop host",
    )
}

// ---------------------------------------------------------------------------
// File dialogs (M4c, ADR 0094)

/// Which system dialog [`crate::Request::PickPath`] shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum PathPickKind {
    /// Choose an existing file.
    OpenFile,
    /// Choose an existing folder.
    OpenFolder,
    /// Choose where to save a file.
    SaveFile,
}

/// A file-type filter of a file dialog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct PathFilter {
    /// Fluent id of the filter's name (e.g. `path-filter-mbox`); the shell
    /// shows the English text when it has no catalog.
    pub label_id: String,
    /// Extensions without the dot (`mbox`, `eml`); empty = all files.
    pub extensions: Vec<String>,
}

/// What [`crate::Request::PickPath`] asks for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct PathPick {
    /// The dialog.
    pub kind: PathPickKind,
    /// Fluent id of the dialog title.
    pub title_id: String,
    /// File-type filters (open / save file).
    #[serde(default)]
    pub filters: Vec<PathFilter>,
    /// Suggested file name (save) or start folder.
    #[serde(default)]
    pub default_path: Option<String>,
}

fn default_true() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    fn installation() -> InstallationInfo {
        InstallationInfo {
            version: "1.0.0".into(),
            kind: InstallKind::Nsis,
            scope: Some(InstallScope::PerUser),
            architecture: WindowsArch::X64,
            install_dir: Some(r"C:\Users\a\AppData\Local\Programs\Nail".into()),
            updates_disabled: None,
            forced_channel: None,
            mapi_installed: false,
            webview_version: None,
        }
    }

    fn update() -> UpdateInfo {
        UpdateInfo {
            version: "1.1.0".into(),
            channel: UpdateChannel::Stable,
            published_at: Some("2026-10-01T10:00:00Z".into()),
            notes: Some("Fixes".into()),
            size_bytes: Some(1000),
        }
    }

    #[test]
    fn wire_shapes() {
        assert_eq!(
            serde_json::to_value(installation()).unwrap(),
            json!({
                "version": "1.0.0", "kind": "nsis", "scope": "per-user",
                "architecture": "x64",
                "install_dir": r"C:\Users\a\AppData\Local\Programs\Nail",
                "updates_disabled": null, "forced_channel": null,
                "mapi_installed": false, "webview_version": null
            })
        );
        let state = UpdateState::Downloading {
            update: update(),
            downloaded_bytes: 10,
            total_bytes: None,
        };
        let value = serde_json::to_value(&state).unwrap();
        assert_eq!(value["state"], "downloading");
        assert_eq!(value["update"]["version"], "1.1.0");
        assert_eq!(serde_json::from_value::<UpdateState>(value).unwrap(), state);
        assert_eq!(
            serde_json::to_value(UpdateState::Disabled {
                reason: UpdateDisabledReason::MsiManaged
            })
            .unwrap(),
            json!({"state": "disabled", "reason": "msi-managed"})
        );
        assert_eq!(
            serde_json::to_value(UpdateState::ReadyToInstall { update: update() }).unwrap()["state"],
            "ready-to-install"
        );
        assert_eq!(
            serde_json::to_value(DefaultAppTarget::Webcals).unwrap(),
            json!("webcals")
        );
        assert_eq!(
            serde_json::to_value(WindowsArch::Arm64).unwrap(),
            json!("arm64")
        );
    }

    #[test]
    fn settings_default_and_accept_missing_fields() {
        let settings: UpdateSettings = serde_json::from_value(json!({})).unwrap();
        assert_eq!(settings, UpdateSettings::default());
        assert!(settings.auto_check && settings.auto_download);
        assert_eq!(settings.channel, UpdateChannel::Stable);
        let desktop: DesktopSettings = serde_json::from_value(json!({})).unwrap();
        assert_eq!(desktop, DesktopSettings::default());
        assert!(desktop.close_to_tray && desktop.start_hidden);
        assert!(!desktop.start_with_windows && !desktop.plugin_dev_endpoint);
    }

    #[test]
    fn the_policy_wins_over_settings_and_host_state() {
        let host = HostUpdateState {
            state: UpdateState::Available { update: update() },
            last_checked_at: Some("2026-10-02T08:00:00Z".into()),
        };
        let stored = UpdateSettings {
            channel: UpdateChannel::Beta,
            ..UpdateSettings::default()
        };
        let free = UpdateStatus::compose(stored.clone(), &installation(), host.clone());
        assert_eq!(free.settings.channel, UpdateChannel::Beta);
        assert_eq!(free.state, host.state);
        assert_eq!(free.current_version, "1.0.0");

        let mut managed = installation();
        managed.kind = InstallKind::Msi;
        managed.scope = Some(InstallScope::PerMachine);
        managed.updates_disabled = Some(UpdateDisabledReason::MsiManaged);
        managed.forced_channel = Some(UpdateChannel::Stable);
        assert!(managed.is_msi_managed());
        let status = UpdateStatus::compose(stored, &managed, host);
        assert_eq!(
            status.state,
            UpdateState::Disabled {
                reason: UpdateDisabledReason::MsiManaged
            }
        );
        assert_eq!(status.settings.channel, UpdateChannel::Stable);
        assert_eq!(status.forced_channel, Some(UpdateChannel::Stable));
        assert!(!status.state.can_check());
    }

    #[test]
    fn state_transitions_allowed() {
        assert!(UpdateState::Idle.can_check());
        assert!(!UpdateState::Checking.can_check());
        let available = UpdateState::Available { update: update() };
        assert!(available.can_download() && !available.can_install());
        let ready = UpdateState::ReadyToInstall { update: update() };
        assert!(ready.can_install() && !ready.can_download());
        assert_eq!(ready.update().unwrap().version, "1.1.0");
        let downloading = UpdateState::Downloading {
            update: update(),
            downloaded_bytes: 1,
            total_bytes: Some(2),
        };
        assert!(!downloading.can_check() && !downloading.can_install());
        assert_eq!(UpdateState::UpToDate.update(), None);
    }

    #[test]
    fn names_parse_back() {
        for kind in InstallKind::ALL {
            assert_eq!(InstallKind::parse(kind.as_str()), Some(kind));
        }
        for scope in InstallScope::ALL {
            assert_eq!(InstallScope::parse(scope.as_str()), Some(scope));
        }
        assert_eq!(UpdateChannel::parse(" Beta "), Some(UpdateChannel::Beta));
        assert_eq!(UpdateChannel::parse("nightly"), None);
        assert_eq!(UpdateChannel::Beta.manifest_path(), "beta/latest.json");
        assert_eq!(DefaultAppTarget::Eml.windows_name(), ".eml");
        assert!(DefaultAppTarget::Mailto.is_scheme() && !DefaultAppTarget::Vcf.is_scheme());
        assert!(WindowsArch::current().is_some());
    }

    #[test]
    fn settings_uri_per_scope() {
        assert_eq!(
            default_apps_settings_uri(Some(InstallScope::PerUser)),
            "ms-settings:defaultapps?registeredAppUser=Nail"
        );
        assert_eq!(
            default_apps_settings_uri(Some(InstallScope::PerMachine)),
            "ms-settings:defaultapps?registeredAppMachine=Nail"
        );
        assert_eq!(default_apps_settings_uri(None), "ms-settings:defaultapps");
    }

    #[test]
    fn unregistered_lists_every_target() {
        let status = DefaultAppsStatus::unregistered();
        let targets: Vec<_> = status.associations.iter().map(|a| a.target).collect();
        assert_eq!(targets, DefaultAppTarget::ALL);
        assert!(!status.is_default_mail_app());
        let mut status = status;
        status.associations[0].state = DefaultAppState::Default;
        assert!(status.is_default_mail_app());
    }

    #[test]
    fn the_dev_endpoint_serves_only_dev_loading() {
        assert!(dev_endpoint_allows(&crate::Request::LoadDevPlugin {
            folder: "C:/p".into(),
            grants: vec![],
        }));
        assert!(!dev_endpoint_allows(&crate::Request::ListAccounts));
        assert!(!dev_endpoint_allows(&crate::Request::GetUpdateStatus));
        assert!(dev_endpoint_forwards(&crate::Event::PluginsChanged));
        assert!(!dev_endpoint_forwards(&crate::Event::LaunchRequested));
    }

    #[test]
    fn endpoint_file_token_is_optional() {
        let serve: UiEndpointFile =
            serde_json::from_value(json!({"url": "http://127.0.0.1:4780/", "pid": 7})).unwrap();
        assert_eq!(serve.token, None);
        let desktop = UiEndpointFile {
            url: "http://127.0.0.1:5000/".into(),
            pid: 8,
            token: Some("t0k".into()),
        };
        let value = serde_json::to_value(&desktop).unwrap();
        assert_eq!(value["token"], "t0k");
        assert_eq!(
            serde_json::to_value(&serve).unwrap(),
            json!({"url": "http://127.0.0.1:4780/", "pid": 7})
        );
    }
}
