//! Installation facts (M4b WP113, ADR 0087): which package installed this
//! copy, for whom, whether the in-app updater may run and from where.
//!
//! The installers (WP112) write `Software\Nail\Install` in `HKCU` (NSIS per
//! user) or `HKLM` (MSI, NSIS per machine); group policy / Intune writes
//! `Software\Policies\Nail` in either hive. The copy that runs is matched
//! by its directory: a development build on a machine where Nail is also
//! installed stays a development build. The portable zip is recognised by
//! its marker file ([`crate::portable`]).
//!
//! The update source is the `plugins.updater` section of `tauri.conf.json`
//! (endpoints with a `{{channel}}` placeholder, and the minisign public
//! key); the release pipeline fills it in. `NAIL_UPDATE_ENDPOINT` /
//! `NAIL_UPDATE_PUBKEY` replace it for tests, but only in builds with the
//! `test-overrides` feature ([`crate::shell::test_env`]; never an installer
//! build), only with a loopback endpoint (the e2e update server), and the
//! policy still wins.
//!
//! When both hives name the running copy's directory, the machine's facts
//! win: `HKLM` is written by an administrator, `HKCU` by the user, who must
//! not turn an MSI install (`DisableUpdates`) into a self-updating one.

use std::path::{Path, PathBuf};

use nail_api::desktop::{
    InstallKind, InstallScope, InstallationInfo, UpdateChannel, UpdateDisabledReason, WindowsArch,
    registry as keys,
};

use crate::registry::{Hive, Registry};

/// Test override of the update endpoint (loopback only; may contain
/// `{{channel}}`).
pub const UPDATE_ENDPOINT_ENV: &str = "NAIL_UPDATE_ENDPOINT";
/// Test override of the update public key (only with
/// [`UPDATE_ENDPOINT_ENV`]).
pub const UPDATE_PUBKEY_ENV: &str = "NAIL_UPDATE_PUBKEY";
/// The placeholder for the channel in an update endpoint.
pub const CHANNEL_PLACEHOLDER: &str = "{{channel}}";

/// Where updates come from.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UpdateSource {
    /// Manifest URLs; `{{channel}}` stands for `stable` / `beta`.
    pub endpoints: Vec<String>,
    /// The minisign public key (base64, as `tauri signer generate` prints it).
    pub pubkey: String,
    /// Set from the test variables (enables updates in development builds).
    pub test_override: bool,
}

impl UpdateSource {
    /// From the `plugins.updater` section of `tauri.conf.json`.
    pub fn from_plugin_config(config: Option<&serde_json::Value>) -> Self {
        let Some(config) = config else {
            return Self::default();
        };
        let endpoints = config
            .get("endpoints")
            .and_then(serde_json::Value::as_array)
            .map(|list| {
                list.iter()
                    .filter_map(serde_json::Value::as_str)
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default();
        let pubkey = config
            .get("pubkey")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_owned();
        Self {
            endpoints,
            pubkey,
            test_override: false,
        }
    }

    /// Applies the test variables: a loopback endpoint and its key replace
    /// the configured source. Anything else is ignored (logged).
    #[must_use]
    pub fn with_test_override(self, endpoint: Option<String>, pubkey: Option<String>) -> Self {
        let (Some(endpoint), Some(pubkey)) = (endpoint, pubkey) else {
            return self;
        };
        if !is_loopback_url(&endpoint.replace(CHANNEL_PLACEHOLDER, "stable")) {
            tracing::warn!(
                endpoint,
                "ignoring {UPDATE_ENDPOINT_ENV}: only loopback endpoints are accepted"
            );
            return self;
        }
        Self {
            endpoints: vec![endpoint],
            pubkey: pubkey.trim().to_owned(),
            test_override: true,
        }
    }

    /// Whether the build can update at all (endpoints and a key).
    pub fn is_configured(&self) -> bool {
        !self.endpoints.is_empty() && !self.pubkey.is_empty()
    }

    /// The manifest URLs of a channel: `{{channel}}` replaced, or a
    /// channel manifest (`<base>/stable/latest.json`, how the release
    /// pipeline writes it) switched to this channel's
    /// ([`UpdateChannel::manifest_path`]). Invalid URLs are dropped.
    pub fn endpoints_for(&self, channel: UpdateChannel) -> Vec<tauri::Url> {
        self.endpoints
            .iter()
            .filter_map(|endpoint| {
                let text = if endpoint.contains(CHANNEL_PLACEHOLDER) {
                    endpoint.replace(CHANNEL_PLACEHOLDER, channel.as_str())
                } else {
                    UpdateChannel::ALL
                        .into_iter()
                        .find_map(|c| endpoint.strip_suffix(&c.manifest_path()))
                        .filter(|base| base.ends_with('/'))
                        .map_or_else(
                            || endpoint.clone(),
                            |base| format!("{base}{}", channel.manifest_path()),
                        )
                };
                tauri::Url::parse(&text).ok()
            })
            .collect()
    }
}

/// `http(s)://127.0.0.1…`, `[::1]` or `localhost`.
pub fn is_loopback_url(text: &str) -> bool {
    let Ok(url) = tauri::Url::parse(text) else {
        return false;
    };
    if !matches!(url.scheme(), "http" | "https") {
        return false;
    }
    let Some(host) = url.host_str() else {
        return false;
    };
    let host = host.trim_start_matches('[').trim_end_matches(']');
    host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
}

/// What the shell knows about itself before reading the registry.
#[derive(Debug, Clone)]
pub struct LocalFacts {
    /// The app's version.
    pub version: String,
    /// The running executable.
    pub exe: Option<PathBuf>,
    /// The portable root, when the marker is there.
    pub portable: bool,
    /// The WebView2 runtime's version.
    pub webview_version: Option<String>,
}

/// Case-insensitive comparison of two directories (trailing separators and
/// `/` vs `\` ignored).
fn same_dir(a: &str, b: &Path) -> bool {
    let norm = |s: &str| {
        s.replace('/', "\\")
            .trim_end_matches('\\')
            .to_ascii_lowercase()
    };
    norm(a) == norm(&b.to_string_lossy())
}

/// The policy value in `HKLM` (wins) or `HKCU`.
fn policy(registry: &dyn Registry, name: &str) -> Option<crate::registry::RegValue> {
    [Hive::LocalMachine, Hive::CurrentUser]
        .into_iter()
        .find_map(|hive| registry.value(hive, keys::POLICY_KEY, name))
}

/// Reads the installation facts.
pub fn installation(
    registry: &dyn Registry,
    local: &LocalFacts,
    source: &UpdateSource,
) -> InstallationInfo {
    let exe_dir = local.exe.as_deref().and_then(Path::parent);
    // The hive whose installation directory is the one we run from; the
    // machine's (admin-written) facts first.
    let installed = [Hive::LocalMachine, Hive::CurrentUser]
        .into_iter()
        .find(|hive| {
            registry
                .string(*hive, keys::INSTALL_KEY, keys::INSTALL_DIR_VALUE)
                .zip(exe_dir)
                .is_some_and(|(dir, exe_dir)| same_dir(&dir, exe_dir))
        });
    let (kind, scope, install_dir, mapi_installed, msi_disables) = match installed {
        _ if local.portable => (
            InstallKind::Portable,
            None,
            exe_dir.map(|d| d.display().to_string()),
            false,
            false,
        ),
        Some(hive) => {
            let kind = registry
                .string(hive, keys::INSTALL_KEY, keys::INSTALLER_VALUE)
                .and_then(|k| InstallKind::parse(k.trim()))
                .unwrap_or(InstallKind::Nsis);
            let scope = registry
                .string(hive, keys::INSTALL_KEY, keys::SCOPE_VALUE)
                .and_then(|s| InstallScope::parse(s.trim()))
                .unwrap_or(match hive {
                    Hive::CurrentUser => InstallScope::PerUser,
                    Hive::LocalMachine => InstallScope::PerMachine,
                });
            // The MSI's DISABLEUPDATES defaults to on.
            let msi_disables = kind == InstallKind::Msi
                && registry
                    .flag(hive, keys::INSTALL_KEY, keys::DISABLE_UPDATES_VALUE)
                    .unwrap_or(true);
            (
                kind,
                Some(scope),
                registry.string(hive, keys::INSTALL_KEY, keys::INSTALL_DIR_VALUE),
                registry
                    .flag(hive, keys::INSTALL_KEY, keys::MAPI_VALUE)
                    .unwrap_or(false),
                msi_disables,
            )
        }
        None => (
            InstallKind::Development,
            None,
            exe_dir.map(|d| d.display().to_string()),
            false,
            false,
        ),
    };
    let policy_disables = policy(registry, keys::POLICY_DISABLE_UPDATES_VALUE)
        .and_then(|v| v.as_flag())
        .unwrap_or(false);
    let forced_channel = policy(registry, keys::POLICY_UPDATE_CHANNEL_VALUE)
        .and_then(|v| UpdateChannel::parse(&v.as_string()));
    let updates_disabled = if policy_disables {
        Some(UpdateDisabledReason::Policy)
    } else if msi_disables {
        Some(UpdateDisabledReason::MsiManaged)
    } else if kind == InstallKind::Portable {
        Some(UpdateDisabledReason::Portable)
    } else if kind == InstallKind::Development && !source.test_override {
        Some(UpdateDisabledReason::Development)
    } else if !source.is_configured() {
        Some(UpdateDisabledReason::NotConfigured)
    } else {
        None
    };
    InstallationInfo {
        version: local.version.clone(),
        kind,
        scope,
        architecture: WindowsArch::current().unwrap_or(WindowsArch::X64),
        install_dir,
        updates_disabled,
        forced_channel,
        mapi_installed,
        webview_version: local.webview_version.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::{MemoryRegistry, RegValue};
    use pretty_assertions::assert_eq;

    const DIR: &str = r"C:\Users\a\AppData\Local\Programs\Nail";

    fn local(dir: &str) -> LocalFacts {
        LocalFacts {
            version: "1.0.0".into(),
            exe: Some(PathBuf::from(dir).join("Nail.exe")),
            portable: false,
            webview_version: Some("130.0".into()),
        }
    }

    fn configured() -> UpdateSource {
        UpdateSource {
            endpoints: vec!["https://updates.nail.example/{{channel}}/latest.json".into()],
            pubkey: "KEY".into(),
            test_override: false,
        }
    }

    fn install(reg: &MemoryRegistry, hive: Hive, installer: &str, dir: &str) {
        reg.put(
            hive,
            keys::INSTALL_KEY,
            keys::INSTALLER_VALUE,
            RegValue::Str(installer.into()),
        );
        reg.put(
            hive,
            keys::INSTALL_KEY,
            keys::INSTALL_DIR_VALUE,
            RegValue::Str(dir.into()),
        );
    }

    #[test]
    fn a_per_user_nsis_install_updates_itself() {
        let reg = MemoryRegistry::new();
        install(&reg, Hive::CurrentUser, "nsis", &format!("{DIR}\\"));
        reg.put(
            Hive::CurrentUser,
            keys::INSTALL_KEY,
            keys::SCOPE_VALUE,
            RegValue::Str("per-user".into()),
        );
        let info = installation(&reg, &local(DIR), &configured());
        assert_eq!(info.kind, InstallKind::Nsis);
        assert_eq!(info.scope, Some(InstallScope::PerUser));
        assert_eq!(info.updates_disabled, None);
        assert_eq!(info.install_dir.as_deref(), Some(&*format!("{DIR}\\")));
        assert_eq!(info.webview_version.as_deref(), Some("130.0"));
        assert!(!info.mapi_installed);
        // Without a source the build cannot update.
        let info = installation(&reg, &local(DIR), &UpdateSource::default());
        assert_eq!(
            info.updates_disabled,
            Some(UpdateDisabledReason::NotConfigured)
        );
    }

    #[test]
    fn the_msi_disables_updates_unless_told_otherwise() {
        let dir = r"C:\Program Files\Nail";
        let reg = MemoryRegistry::new();
        install(&reg, Hive::LocalMachine, "msi", dir);
        reg.put(
            Hive::LocalMachine,
            keys::INSTALL_KEY,
            keys::MAPI_VALUE,
            RegValue::Num(1),
        );
        let info = installation(&reg, &local(dir), &configured());
        assert_eq!(info.kind, InstallKind::Msi);
        assert_eq!(info.scope, Some(InstallScope::PerMachine), "from the hive");
        assert_eq!(
            info.updates_disabled,
            Some(UpdateDisabledReason::MsiManaged)
        );
        assert!(info.mapi_installed);
        // DISABLEUPDATES=0: IT lets the app update itself.
        reg.put(
            Hive::LocalMachine,
            keys::INSTALL_KEY,
            keys::DISABLE_UPDATES_VALUE,
            RegValue::Str("0".into()),
        );
        assert_eq!(
            installation(&reg, &local(dir), &configured()).updates_disabled,
            None
        );
    }

    #[test]
    fn user_facts_cannot_override_the_machine_install() {
        // The MSI wrote HKLM; the user forges HKCU facts for the same folder.
        let dir = r"C:\Program Files\Nail";
        let reg = MemoryRegistry::new();
        install(&reg, Hive::LocalMachine, "msi", dir);
        install(&reg, Hive::CurrentUser, "nsis", &format!("{dir}\\"));
        reg.put(
            Hive::CurrentUser,
            keys::INSTALL_KEY,
            keys::SCOPE_VALUE,
            RegValue::Str("per-user".into()),
        );
        let info = installation(&reg, &local(dir), &configured());
        assert_eq!(info.kind, InstallKind::Msi);
        assert_eq!(info.scope, Some(InstallScope::PerMachine));
        assert_eq!(
            info.updates_disabled,
            Some(UpdateDisabledReason::MsiManaged)
        );
        // A per-user install elsewhere still reads its own HKCU facts.
        let info = installation(&reg, &local(DIR), &configured());
        assert_eq!(info.kind, InstallKind::Development);
        install(&reg, Hive::CurrentUser, "nsis", DIR);
        assert_eq!(
            installation(&reg, &local(DIR), &configured()).kind,
            InstallKind::Nsis
        );
    }

    #[test]
    fn the_policy_wins() {
        let reg = MemoryRegistry::new();
        install(&reg, Hive::CurrentUser, "nsis", DIR);
        reg.put(
            Hive::CurrentUser,
            keys::POLICY_KEY,
            keys::POLICY_DISABLE_UPDATES_VALUE,
            RegValue::Num(0),
        );
        reg.put(
            Hive::LocalMachine,
            keys::POLICY_KEY,
            keys::POLICY_UPDATE_CHANNEL_VALUE,
            RegValue::Str("Beta".into()),
        );
        let info = installation(&reg, &local(DIR), &configured());
        assert_eq!(info.updates_disabled, None);
        assert_eq!(info.forced_channel, Some(UpdateChannel::Beta));
        // HKLM's policy beats HKCU's.
        reg.put(
            Hive::LocalMachine,
            keys::POLICY_KEY,
            keys::POLICY_DISABLE_UPDATES_VALUE,
            RegValue::Num(1),
        );
        let info = installation(&reg, &local(DIR), &configured());
        assert_eq!(info.updates_disabled, Some(UpdateDisabledReason::Policy));
        // Even a test endpoint does not get past the policy.
        let source = configured().with_test_override(
            Some("http://127.0.0.1:9/{{channel}}/latest.json".into()),
            Some("K".into()),
        );
        let info = installation(&reg, &local(DIR), &source);
        assert_eq!(info.updates_disabled, Some(UpdateDisabledReason::Policy));
    }

    #[test]
    fn development_and_portable_copies_do_not_update() {
        let reg = MemoryRegistry::new();
        // Nail is installed, but this executable runs from elsewhere.
        install(&reg, Hive::CurrentUser, "nsis", DIR);
        let dev = local(r"D:\build\debug");
        let info = installation(&reg, &dev, &configured());
        assert_eq!(info.kind, InstallKind::Development);
        assert_eq!(info.scope, None);
        assert_eq!(
            info.updates_disabled,
            Some(UpdateDisabledReason::Development)
        );
        assert_eq!(info.install_dir.as_deref(), Some(r"D:\build\debug"));
        // The e2e update server enables a development build.
        let source = UpdateSource::default().with_test_override(
            Some("http://127.0.0.1:4999/{{channel}}/latest.json".into()),
            Some("K".into()),
        );
        assert!(source.test_override);
        assert_eq!(installation(&reg, &dev, &source).updates_disabled, None);

        let portable = LocalFacts {
            portable: true,
            ..local(DIR)
        };
        let info = installation(&reg, &portable, &configured());
        assert_eq!(info.kind, InstallKind::Portable);
        assert_eq!(info.updates_disabled, Some(UpdateDisabledReason::Portable));
    }

    #[test]
    fn the_update_source_comes_from_the_plugin_config() {
        let config = serde_json::json!({
            "pubkey": " KEY ",
            "endpoints": ["https://updates.nail.example/{{channel}}/latest.json", 7]
        });
        let source = UpdateSource::from_plugin_config(Some(&config));
        assert!(source.is_configured());
        assert_eq!(source.pubkey, "KEY");
        assert_eq!(
            source.endpoints_for(UpdateChannel::Beta)[0].as_str(),
            "https://updates.nail.example/beta/latest.json"
        );
        // The release pipeline writes the stable manifest; beta swaps it.
        let pipeline = UpdateSource {
            endpoints: vec![
                "https://updates.nail.example/nail/stable/latest.json".into(),
                "https://mirror.nail.example/latest.json".into(),
            ],
            ..source.clone()
        };
        let urls: Vec<String> = pipeline
            .endpoints_for(UpdateChannel::Beta)
            .iter()
            .map(ToString::to_string)
            .collect();
        assert_eq!(
            urls,
            [
                "https://updates.nail.example/nail/beta/latest.json",
                "https://mirror.nail.example/latest.json"
            ]
        );
        assert_eq!(
            pipeline.endpoints_for(UpdateChannel::Stable)[0].as_str(),
            "https://updates.nail.example/nail/stable/latest.json"
        );
        let empty = UpdateSource::from_plugin_config(Some(&serde_json::json!({"pubkey": ""})));
        assert!(!empty.is_configured());
        assert!(!UpdateSource::from_plugin_config(None).is_configured());
    }

    #[test]
    fn only_loopback_test_endpoints_are_accepted() {
        assert!(is_loopback_url("http://127.0.0.1:5000/x"));
        assert!(is_loopback_url("http://[::1]:5000/x"));
        assert!(is_loopback_url("http://localhost/x"));
        assert!(!is_loopback_url("http://updates.evil.example/x"));
        assert!(!is_loopback_url("file:///C:/x"));
        let source = configured().with_test_override(
            Some("https://updates.evil.example/{{channel}}/latest.json".into()),
            Some("EVIL".into()),
        );
        assert_eq!(source, configured());
        // An endpoint without its key changes nothing.
        let source = configured().with_test_override(Some("http://127.0.0.1:1/".into()), None);
        assert_eq!(source, configured());
    }

    /// The installer writes the facts; the shell reads them from the real
    /// registry (below a throw-away test root).
    #[cfg(windows)]
    #[test]
    fn facts_are_read_from_the_windows_registry() {
        let root = format!(r"Software\NailTests\install-{}", std::process::id());
        let reg = crate::registry::WindowsRegistry::below(&root);
        reg.set_string(
            Hive::LocalMachine,
            keys::INSTALL_KEY,
            keys::INSTALLER_VALUE,
            "msi",
        )
        .unwrap();
        reg.set_string(
            Hive::LocalMachine,
            keys::INSTALL_KEY,
            keys::INSTALL_DIR_VALUE,
            r"C:\Program Files\Nail",
        )
        .unwrap();
        let info = installation(&reg, &local(r"C:\Program Files\Nail"), &configured());
        reg.remove_root().unwrap();
        assert_eq!(info.kind, InstallKind::Msi);
        assert_eq!(
            info.updates_disabled,
            Some(UpdateDisabledReason::MsiManaged)
        );
    }
}
