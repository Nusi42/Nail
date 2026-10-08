//! Small Win32 helpers of the desktop shell (M4b WP113): the current
//! user's SID (the Simple MAPI pipe name and its ACL), a security
//! descriptor from SDDL, random tokens, and opening `ms-settings:` URIs.

#![expect(unsafe_code, reason = "Win32 calls, each with its SAFETY note")]

use std::fmt::Write as _;

/// A random token: 32 bytes from the system RNG, hex.
pub fn random_token() -> Result<String, String> {
    let mut bytes = [0u8; 32];
    fill_random(&mut bytes)?;
    Ok(bytes.iter().fold(String::with_capacity(64), |mut out, b| {
        // Writing to a String cannot fail.
        let _ = write!(out, "{b:02x}");
        out
    }))
}

#[cfg(windows)]
fn fill_random(bytes: &mut [u8]) -> Result<(), String> {
    use windows::Win32::Security::Cryptography::{
        BCRYPT_USE_SYSTEM_PREFERRED_RNG, BCryptGenRandom,
    };
    // SAFETY: no algorithm handle with the system-preferred flag; the
    // buffer is a valid, writable slice.
    let status = unsafe { BCryptGenRandom(None, bytes, BCRYPT_USE_SYSTEM_PREFERRED_RNG) };
    if status.is_ok() {
        Ok(())
    } else {
        Err(format!("BCryptGenRandom failed: {:#x}", status.0))
    }
}

#[cfg(not(windows))]
fn fill_random(bytes: &mut [u8]) -> Result<(), String> {
    use std::io::Read;
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(bytes))
        .map_err(|e| e.to_string())
}

/// Constant-time comparison of two secrets.
pub fn secrets_equal(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// The SDDL of the Simple MAPI pipe: a protected DACL granting the user
/// (and nobody else, not even administrators) full access.
pub fn pipe_sddl(user_sid: &str) -> String {
    format!("D:P(A;;GA;;;{user_sid})")
}

/// The pipe's SDDL with a mandatory label at `label` (`ME`, `HI`, …) and
/// no-write-up: a client below that integrity level cannot send to the
/// pipe (M4b review — an elevated Nail must not take attachment paths from
/// the user's unelevated programs and read files only admins may read).
pub fn pipe_sddl_at(user_sid: &str, label: &str) -> String {
    format!("{}S:(ML;;NW;;;{label})", pipe_sddl(user_sid))
}

/// The SDDL integrity label for a mandatory-label RID.
pub fn integrity_label(rid: u32) -> &'static str {
    match rid {
        0x4000.. => "SI",
        0x3000.. => "HI",
        0x2100.. => "MP",
        0x2000.. => "ME",
        _ => "LW",
    }
}

/// The SDDL of the Simple MAPI pipe for this process: the user-only DACL
/// labelled at this process's integrity level (medium if unknown).
pub fn server_pipe_sddl(user_sid: &str) -> String {
    let rid = current_integrity_rid()
        .map_err(|err| tracing::warn!(%err, "integrity level unknown, labelling the pipe medium"))
        .unwrap_or(0x2000);
    pipe_sddl_at(user_sid, integrity_label(rid))
}

/// One class of information about the current process's token, in a
/// buffer of `u64`s (aligned for the structure the class returns).
#[cfg(windows)]
fn process_token_information(
    class: windows::Win32::Security::TOKEN_INFORMATION_CLASS,
) -> Result<Vec<u64>, String> {
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::Security::{GetTokenInformation, TOKEN_QUERY};
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    let mut token = HANDLE::default();
    // SAFETY: the pseudo handle of the current process; `token` receives a
    // handle closed below.
    unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &raw mut token) }
        .map_err(|e| format!("OpenProcessToken: {e}"))?;
    let result = (|| {
        let mut len = 0u32;
        // SAFETY: a size query (no buffer); it fails with
        // ERROR_INSUFFICIENT_BUFFER and sets `len`.
        drop(unsafe { GetTokenInformation(token, class, None, 0, &raw mut len) });
        if len == 0 {
            return Err("GetTokenInformation returned no size".to_owned());
        }
        let mut buffer = vec![0u64; (len as usize).div_ceil(8)];
        // SAFETY: the buffer holds at least `len` bytes.
        unsafe {
            GetTokenInformation(
                token,
                class,
                Some(buffer.as_mut_ptr().cast()),
                len,
                &raw mut len,
            )
        }
        .map_err(|e| format!("GetTokenInformation: {e}"))?;
        Ok(buffer)
    })();
    // SAFETY: the token handle opened above.
    drop(unsafe { CloseHandle(token) });
    result
}

/// The current process's integrity level RID (`0x2000` medium, `0x3000`
/// high, …).
#[cfg(windows)]
pub fn current_integrity_rid() -> Result<u32, String> {
    use windows::Win32::Security::{
        GetSidSubAuthority, GetSidSubAuthorityCount, TOKEN_MANDATORY_LABEL, TokenIntegrityLevel,
    };

    let buffer = process_token_information(TokenIntegrityLevel)?;
    // SAFETY: the call filled the buffer with a TOKEN_MANDATORY_LABEL whose
    // SID points into it; the last sub-authority is the level.
    unsafe {
        let label = &*buffer.as_ptr().cast::<TOKEN_MANDATORY_LABEL>();
        let sid = label.Label.Sid;
        let count = *GetSidSubAuthorityCount(sid);
        if count == 0 {
            return Err("integrity SID without sub-authority".to_owned());
        }
        Ok(*GetSidSubAuthority(sid, u32::from(count) - 1))
    }
}

/// Not on this system.
#[cfg(not(windows))]
pub fn current_integrity_rid() -> Result<u32, String> {
    Err("no integrity levels on this system".into())
}

/// The current process's user SID as a string (`S-1-5-21-…`).
#[cfg(windows)]
pub fn current_user_sid() -> Result<String, String> {
    use windows::Win32::Foundation::{HLOCAL, LocalFree};
    use windows::Win32::Security::Authorization::ConvertSidToStringSidW;
    use windows::Win32::Security::{TOKEN_USER, TokenUser};
    use windows::core::PWSTR;

    let buffer = process_token_information(TokenUser)?;
    // SAFETY: the call filled the buffer with a TOKEN_USER.
    let user = unsafe { &*buffer.as_ptr().cast::<TOKEN_USER>() };
    let mut text = PWSTR::null();
    // SAFETY: the SID points into `buffer`, alive here; `text` is freed
    // with LocalFree.
    unsafe { ConvertSidToStringSidW(user.User.Sid, &raw mut text) }
        .map_err(|e| format!("ConvertSidToStringSidW: {e}"))?;
    // SAFETY: a NUL-terminated string from the call above.
    let sid = unsafe { text.to_string() }.map_err(|e| e.to_string());
    // SAFETY: allocated by ConvertSidToStringSidW.
    unsafe { LocalFree(Some(HLOCAL(text.0.cast()))) };
    sid
}

/// Not on this system.
#[cfg(not(windows))]
pub fn current_user_sid() -> Result<String, String> {
    Err("no Windows SID on this system".into())
}

/// A security descriptor built from SDDL, freed on drop. Keeps a
/// `SECURITY_ATTRIBUTES` pointing at it for `CreateNamedPipeW`.
#[cfg(windows)]
pub struct SecurityAttributes {
    descriptor: windows::Win32::Security::PSECURITY_DESCRIPTOR,
    attributes: windows::Win32::Security::SECURITY_ATTRIBUTES,
}

#[cfg(windows)]
impl std::fmt::Debug for SecurityAttributes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SecurityAttributes").finish_non_exhaustive()
    }
}

#[cfg(windows)]
// SAFETY: the descriptor is immutable after creation and only read by the
// system while creating pipe instances.
unsafe impl Send for SecurityAttributes {}
#[cfg(windows)]
// SAFETY: see above.
unsafe impl Sync for SecurityAttributes {}

#[cfg(windows)]
impl SecurityAttributes {
    /// From an SDDL string.
    pub fn from_sddl(sddl: &str) -> Result<Self, String> {
        use windows::Win32::Security::Authorization::{
            ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
        };
        use windows::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};
        use windows::core::HSTRING;
        let mut descriptor = PSECURITY_DESCRIPTOR::default();
        // SAFETY: a valid NUL-terminated SDDL string; the descriptor is freed
        // with LocalFree on drop.
        unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                &HSTRING::from(sddl),
                SDDL_REVISION_1,
                &raw mut descriptor,
                None,
            )
        }
        .map_err(|e| format!("invalid security descriptor {sddl}: {e}"))?;
        Ok(Self {
            descriptor,
            attributes: SECURITY_ATTRIBUTES {
                nLength: u32::try_from(size_of::<SECURITY_ATTRIBUTES>()).unwrap_or(u32::MAX),
                lpSecurityDescriptor: descriptor.0,
                bInheritHandle: false.into(),
            },
        })
    }

    /// The `SECURITY_ATTRIBUTES` pointer (valid while `self` lives).
    pub fn as_ptr(&self) -> *mut std::ffi::c_void {
        std::ptr::from_ref(&self.attributes).cast_mut().cast()
    }
}

#[cfg(windows)]
impl Drop for SecurityAttributes {
    fn drop(&mut self) {
        use windows::Win32::Foundation::{HLOCAL, LocalFree};
        // SAFETY: allocated by ConvertStringSecurityDescriptorToSecurityDescriptorW.
        unsafe { LocalFree(Some(HLOCAL(self.descriptor.0))) };
    }
}

/// Opens a URI with the shell (`ms-settings:…`). Only `ms-settings:` URIs
/// are accepted.
pub fn open_settings_uri(uri: &str) -> Result<(), String> {
    if !uri.starts_with("ms-settings:") {
        return Err(format!("not a Settings URI: {uri}"));
    }
    #[cfg(windows)]
    {
        use windows::Win32::UI::Shell::ShellExecuteW;
        use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
        use windows::core::{HSTRING, PCWSTR, w};
        // SAFETY: valid NUL-terminated strings; no owner window.
        let result = unsafe {
            ShellExecuteW(
                None,
                w!("open"),
                &HSTRING::from(uri),
                PCWSTR::null(),
                PCWSTR::null(),
                SW_SHOWNORMAL,
            )
        };
        // ShellExecuteW reports success with a value above 32.
        if result.0 as usize > 32 {
            Ok(())
        } else {
            Err(format!("ShellExecuteW failed with {}", result.0 as usize))
        }
    }
    #[cfg(not(windows))]
    {
        Err("no Settings app on this system".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_random_hex() {
        let a = random_token().unwrap();
        let b = random_token().unwrap();
        assert_eq!(a.len(), 64);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, b);
    }

    #[test]
    fn secrets_compare_exactly() {
        assert!(secrets_equal(b"abc", b"abc"));
        assert!(!secrets_equal(b"abc", b"abx"));
        assert!(!secrets_equal(b"abc", b"abcd"));
    }

    #[test]
    fn only_settings_uris_are_opened() {
        assert!(open_settings_uri("https://evil.example/").is_err());
        assert!(open_settings_uri("file:///C:/Windows/notepad.exe").is_err());
    }

    #[cfg(windows)]
    #[test]
    fn the_user_sid_makes_a_valid_descriptor() {
        let sid = current_user_sid().unwrap();
        assert!(sid.starts_with("S-1-"), "{sid}");
        let sddl = pipe_sddl(&sid);
        assert_eq!(sddl, format!("D:P(A;;GA;;;{sid})"));
        let attributes = SecurityAttributes::from_sddl(&sddl).unwrap();
        assert!(!attributes.as_ptr().is_null());
        assert!(SecurityAttributes::from_sddl("D:P(nonsense").is_err());
        // The server's descriptor carries its integrity label.
        let rid = current_integrity_rid().unwrap();
        assert!(rid >= 0x1000, "{rid:#x}");
        let labelled = server_pipe_sddl(&sid);
        assert_eq!(
            labelled,
            format!("D:P(A;;GA;;;{sid})S:(ML;;NW;;;{})", integrity_label(rid))
        );
        assert!(SecurityAttributes::from_sddl(&labelled).is_ok());
    }

    #[test]
    fn integrity_labels() {
        assert_eq!(integrity_label(0x1000), "LW");
        assert_eq!(integrity_label(0x2000), "ME");
        assert_eq!(integrity_label(0x2100), "MP");
        assert_eq!(integrity_label(0x3000), "HI");
        assert_eq!(integrity_label(0x4000), "SI");
        assert_eq!(integrity_label(0x5000), "SI");
        assert_eq!(
            pipe_sddl_at("S-1-5-21-1", "HI"),
            "D:P(A;;GA;;;S-1-5-21-1)S:(ML;;NW;;;HI)"
        );
    }
}
