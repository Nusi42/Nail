//! Windows sign-in through SSPI (plan M3c WP93; contract ADR 0072).
//!
//! **Stub (WP90).** WP93 adds `SspiNegotiate`, the
//! [`nail_api::exchange::engine::NegotiateAuth`] the EWS engine uses for
//! `auth "windows"`. The stub proves the platform layer links and answers:
//! [`package_info`] asks SSPI about a security package (no network, no
//! credentials).

use nail_api::ErrorCode;
use nail_api::accounts::SspiPackage;
use nail_api::engine::EngineError;

/// What SSPI says about a security package.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageInfo {
    /// The package's name as SSPI reports it (`Negotiate`, `Kerberos`,
    /// `NTLM`).
    pub name: String,
    /// The largest token the package produces (bytes).
    pub max_token: u32,
}

/// Asks SSPI about `package`. Off Windows: `unsupported`.
pub fn package_info(package: SspiPackage) -> Result<PackageInfo, EngineError> {
    imp::package_info(package)
}

#[cfg(windows)]
mod imp {
    #![allow(unsafe_code)]

    use windows::Win32::Security::Authentication::Identity::{
        FreeContextBuffer, QuerySecurityPackageInfoW,
    };
    use windows::core::{HSTRING, PCWSTR};

    use super::{EngineError, ErrorCode, PackageInfo, SspiPackage};

    pub(super) fn package_info(package: SspiPackage) -> Result<PackageInfo, EngineError> {
        let name = HSTRING::from(package.sspi_name());
        // SAFETY: `name` outlives the call; on success SSPI returns a
        // buffer it owns, which is read once and freed with
        // `FreeContextBuffer` below.
        let info = unsafe { QuerySecurityPackageInfoW(&name) }.map_err(|e| {
            EngineError::permanent(
                ErrorCode::Unsupported,
                format!("SSPI package {}: {e}", package.sspi_name()),
            )
        })?;
        if info.is_null() {
            return Err(EngineError::permanent(
                ErrorCode::Unsupported,
                format!("SSPI package {} not found", package.sspi_name()),
            ));
        }
        // SAFETY: `info` is a valid `SecPkgInfoW` returned by SSPI; its
        // `Name` is a NUL-terminated wide string inside the same buffer.
        let result = unsafe {
            let record = &*info;
            PackageInfo {
                name: PCWSTR(record.Name).to_string().unwrap_or_default(),
                max_token: record.cbMaxToken,
            }
        };
        // SAFETY: `info` came from `QuerySecurityPackageInfoW` and is
        // freed exactly once.
        let _ = unsafe { FreeContextBuffer(info.cast()) };
        Ok(result)
    }
}

#[cfg(not(windows))]
mod imp {
    use super::{EngineError, ErrorCode, PackageInfo, SspiPackage};

    pub(super) fn package_info(package: SspiPackage) -> Result<PackageInfo, EngineError> {
        Err(EngineError::permanent(
            ErrorCode::Unsupported,
            format!("Windows sign-in ({}) needs Windows", package.sspi_name()),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn sspi_knows_negotiate_kerberos_and_ntlm() {
        for package in [
            SspiPackage::Negotiate,
            SspiPackage::Kerberos,
            SspiPackage::Ntlm,
        ] {
            let info = package_info(package).unwrap();
            assert!(
                info.name.eq_ignore_ascii_case(package.sspi_name()),
                "{info:?}"
            );
            assert!(info.max_token > 0, "{info:?}");
        }
    }

    #[cfg(not(windows))]
    #[test]
    fn sspi_is_windows_only() {
        assert_eq!(
            package_info(SspiPackage::Negotiate).unwrap_err().code,
            ErrorCode::Unsupported
        );
    }
}
