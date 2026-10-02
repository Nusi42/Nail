//! Small Win32 helpers of the desktop shell (M4b WP113): the current
//! user's SID (the Simple MAPI pipe name and its ACL), a security
//! descriptor from SDDL, random tokens, and opening `ms-settings:` URIs.

#![allow(unsafe_code)] // Win32 calls, each with its SAFETY note.

/// A random token: 32 bytes from the system RNG, hex.
pub fn random_token() -> Result<String, String> {
    let mut bytes = [0u8; 32];
    fill_random(&mut bytes)?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
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

/// The current process's user SID as a string (`S-1-5-21-…`).
#[cfg(windows)]
pub fn current_user_sid() -> Result<String, String> {
    use windows::Win32::Foundation::{CloseHandle, HANDLE, HLOCAL, LocalFree};
    use windows::Win32::Security::Authorization::ConvertSidToStringSidW;
    use windows::Win32::Security::{GetTokenInformation, TOKEN_QUERY, TOKEN_USER, TokenUser};
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    use windows::core::PWSTR;

    let mut token = HANDLE::default();
    // SAFETY: the pseudo handle of the current process; `token` receives a
    // handle closed below.
    unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) }
        .map_err(|e| format!("OpenProcessToken: {e}"))?;
    let result = (|| {
        let mut len = 0u32;
        // SAFETY: a size query (no buffer); it fails with
        // ERROR_INSUFFICIENT_BUFFER and sets `len`.
        let _ = unsafe { GetTokenInformation(token, TokenUser, None, 0, &mut len) };
        if len == 0 {
            return Err("GetTokenInformation returned no size".to_owned());
        }
        // u64 elements keep the TOKEN_USER suitably aligned.
        let mut buffer = vec![0u64; (len as usize).div_ceil(8)];
        // SAFETY: the buffer holds at least `len` bytes.
        unsafe {
            GetTokenInformation(
                token,
                TokenUser,
                Some(buffer.as_mut_ptr().cast()),
                len,
                &mut len,
            )
        }
        .map_err(|e| format!("GetTokenInformation: {e}"))?;
        // SAFETY: the call filled the buffer with a TOKEN_USER.
        let user = unsafe { &*buffer.as_ptr().cast::<TOKEN_USER>() };
        let mut text = PWSTR::null();
        // SAFETY: the SID points into `buffer`, alive here; `text` is
        // freed with LocalFree.
        unsafe { ConvertSidToStringSidW(user.User.Sid, &mut text) }
            .map_err(|e| format!("ConvertSidToStringSidW: {e}"))?;
        // SAFETY: a NUL-terminated string from the call above.
        let sid = unsafe { text.to_string() }.map_err(|e| e.to_string());
        // SAFETY: allocated by ConvertSidToStringSidW.
        unsafe { LocalFree(Some(HLOCAL(text.0.cast()))) };
        sid
    })();
    // SAFETY: the token handle opened above.
    let _ = unsafe { CloseHandle(token) };
    result
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
                &mut descriptor,
                None,
            )
        }
        .map_err(|e| format!("invalid security descriptor {sddl}: {e}"))?;
        Ok(Self {
            descriptor,
            attributes: SECURITY_ATTRIBUTES {
                nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
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
        assert!(!secrets_equal(b"abc", b"abd"));
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
    }
}
