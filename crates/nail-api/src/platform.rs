//! The platform the core runs on (ADR 0202): its operating system and the
//! features that exist only on some systems. Part of the
//! [`crate::Response::Config`] answer, so the UI hides what the system
//! cannot do (a Windows-only setting on macOS or Linux) instead of showing
//! it and failing. Each feature is decided here, at compile time, from the
//! same `cfg` that builds its implementation.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// The argument every localized message gets: the core's [`Os`] (its wire
/// name), so a catalog can name the system's own parts (`{ $os -> [windows]
/// the Windows Credential Manager *[other] the system keychain }`). The UI
/// passes the same value; a message's own arguments win.
pub const OS_MESSAGE_ARG: &str = "os";

/// An operating system the core is built for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum Os {
    /// Windows.
    Windows,
    /// macOS.
    Macos,
    /// Linux.
    Linux,
    /// Any other system (BSDs, …).
    Other,
}

impl Os {
    /// The wire name (`windows`, `macos`, `linux`, `other`).
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Windows => "windows",
            Self::Macos => "macos",
            Self::Linux => "linux",
            Self::Other => "other",
        }
    }

    /// The system this binary was built for.
    pub const fn current() -> Self {
        if cfg!(windows) {
            Self::Windows
        } else if cfg!(target_os = "macos") {
            Self::Macos
        } else if cfg!(target_os = "linux") {
            Self::Linux
        } else {
            Self::Other
        }
    }
}

/// A feature only some systems have.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum PlatformFeature {
    /// Exchange sign-in with the Windows account (SSPI: Negotiate,
    /// Kerberos, NTLM).
    WindowsSignIn,
    /// S/MIME certificates from the Windows certificate store
    /// ([`crate::Request::ListWindowsCertificates`]).
    WindowsCertificateStore,
    /// Starting with the system (the Run key). Desktop app only.
    StartWithSystem,
    /// Default apps for `mailto:`, calendar and contact files, opened in the
    /// system's settings. Desktop app only.
    DefaultApps,
    /// The Simple MAPI provider ("Send to → Mail recipient"). Desktop app
    /// only.
    SimpleMapi,
}

impl PlatformFeature {
    /// The features of the core itself (also in `nail serve`).
    pub const IN_CORE: &[Self] = if cfg!(windows) {
        &[Self::WindowsSignIn, Self::WindowsCertificateStore]
    } else {
        &[]
    };

    /// The features only the desktop app can have, on any system (`nail
    /// serve` and remote sessions never report them).
    pub const OF_DESKTOP_APP: &[Self] =
        &[Self::StartWithSystem, Self::DefaultApps, Self::SimpleMapi];

    /// The features the desktop app adds on this system.
    pub const IN_DESKTOP_APP: &[Self] = if cfg!(windows) {
        Self::OF_DESKTOP_APP
    } else {
        &[]
    };
}

/// The platform facts of a core.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct PlatformInfo {
    /// The system the core runs on.
    pub os: Os,
    /// What this system offers here, sorted.
    pub features: Vec<PlatformFeature>,
}

impl PlatformInfo {
    /// This build's platform; `desktop` adds the desktop app's features
    /// (left out for `nail serve` and remote sessions, which cannot reach
    /// the desktop host).
    pub fn current(desktop: bool) -> Self {
        let mut features = PlatformFeature::IN_CORE.to_vec();
        if desktop {
            features.extend_from_slice(PlatformFeature::IN_DESKTOP_APP);
        }
        Self {
            os: Os::current(),
            features,
        }
    }

    /// Whether `feature` is available.
    pub fn has(&self, feature: PlatformFeature) -> bool {
        self.features.contains(&feature)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_wire_names_are_stable() {
        let info = PlatformInfo {
            os: Os::Macos,
            features: vec![
                PlatformFeature::WindowsSignIn,
                PlatformFeature::WindowsCertificateStore,
                PlatformFeature::StartWithSystem,
                PlatformFeature::DefaultApps,
                PlatformFeature::SimpleMapi,
            ],
        };
        assert_eq!(
            serde_json::to_value(&info).unwrap(),
            json!({"os": "macos", "features": [
                "windows-sign-in", "windows-certificate-store",
                "start-with-system", "default-apps", "simple-mapi"
            ]})
        );
    }

    #[test]
    fn os_names_match_the_wire() {
        for os in [Os::Windows, Os::Macos, Os::Linux, Os::Other] {
            assert_eq!(serde_json::to_value(os).unwrap(), json!(os.as_str()));
        }
    }

    #[test]
    fn this_build_reports_its_system() {
        let os = Os::current();
        assert_eq!(os == Os::Windows, cfg!(windows));
        assert_eq!(os == Os::Macos, cfg!(target_os = "macos"));
        assert_eq!(os == Os::Linux, cfg!(target_os = "linux"));
    }

    #[test]
    fn desktop_features_need_the_desktop_app() {
        let served = PlatformInfo::current(false);
        let desktop = PlatformInfo::current(true);
        for feature in PlatformFeature::IN_DESKTOP_APP {
            assert!(!served.has(*feature), "{feature:?}");
            assert!(desktop.has(*feature), "{feature:?}");
        }
        for feature in PlatformFeature::IN_CORE {
            assert!(served.has(*feature) && desktop.has(*feature), "{feature:?}");
        }
    }

    #[test]
    fn windows_only_features_exist_only_on_windows() {
        let desktop = PlatformInfo::current(true);
        assert_eq!(desktop.features.is_empty(), !cfg!(windows));
    }
}
