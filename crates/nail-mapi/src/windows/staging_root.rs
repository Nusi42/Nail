//! The staging root as a pinned, checked directory (M4b review).
//!
//! The root (`%TEMP%\nail-mapi`, for a service `C:\Windows\Temp\nail-mapi`)
//! may have been prepared by someone else: a junction to a protected folder
//! would make the stale sweep delete there and the copies land there, with
//! the caller's rights (an elevated program, a service). [`RootGuard::open`]
//! therefore opens the root itself (`FILE_FLAG_OPEN_REPARSE_POINT`: a
//! junction or symlink is not followed) and refuses it unless it
//!
//! * is a real directory, not a reparse point, and
//! * is owned by the calling user, the Administrators group or SYSTEM
//!   (an administrator's elevated programs own what they create as
//!   Administrators; nobody else can make one of those the owner).
//!
//! The handle is held while the DLL works below the root, opened without
//! `FILE_SHARE_WRITE` / `FILE_SHARE_DELETE`: nobody can rename, delete or
//! turn the root into a junction meanwhile, while other calls (which open
//! it the same way) and the app reading the copies are not blocked.

use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::path::Path;

use windows_sys::Win32::Foundation::{ERROR_SUCCESS, INVALID_HANDLE_VALUE, LocalFree};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, GetSecurityInfo, SE_FILE_OBJECT,
};
use windows_sys::Win32::Security::OWNER_SECURITY_INFORMATION;
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT, FILE_ATTRIBUTE_TAG_INFO,
    FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_LIST_DIRECTORY,
    FILE_READ_ATTRIBUTES, FILE_SHARE_READ, FileAttributeTagInfo, GetFileInformationByHandleEx,
    OPEN_EXISTING, READ_CONTROL,
};

use super::client::current_user;
use super::text::to_wide;

/// Owners other than the calling user that the root may have: the
/// Administrators group and SYSTEM.
pub const TRUSTED_OWNERS: [&str; 2] = ["S-1-5-32-544", "S-1-5-18"];

/// Why a root was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RootRefused {
    /// It could not be opened or queried.
    Unreadable,
    /// A junction, symlink or other reparse point.
    ReparsePoint,
    /// Not a directory.
    NotADirectory,
    /// Owned by someone else (the owner's SID).
    ForeignOwner(String),
}

/// Whether `owner` may own the staging root of `user`.
pub fn owner_acceptable(owner: &str, user: &str) -> bool {
    owner.eq_ignore_ascii_case(user) || TRUSTED_OWNERS.contains(&owner)
}

/// The checked root, pinned while this lives.
#[derive(Debug)]
pub struct RootGuard {
    _handle: OwnedHandle,
}

impl RootGuard {
    /// Opens and checks `root` (which must exist).
    pub fn open(root: &Path) -> Result<Self, RootRefused> {
        let wide = to_wide(&root.to_string_lossy());
        // SAFETY: `wide` is zero-terminated; no security attributes. Only
        // FILE_SHARE_READ: the root cannot be renamed, deleted or given a
        // reparse point while the handle is open.
        let raw = unsafe {
            CreateFileW(
                wide.as_ptr(),
                FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES | READ_CONTROL,
                FILE_SHARE_READ,
                std::ptr::null(),
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
                std::ptr::null_mut(),
            )
        };
        if raw.is_null() || raw == INVALID_HANDLE_VALUE {
            return Err(RootRefused::Unreadable);
        }
        // SAFETY: a fresh handle owned by nobody else.
        let handle = unsafe { OwnedHandle::from_raw_handle(raw) };
        let mut info = FILE_ATTRIBUTE_TAG_INFO::default();
        // SAFETY: `info` is a FILE_ATTRIBUTE_TAG_INFO of the given size.
        let ok = unsafe {
            GetFileInformationByHandleEx(
                handle.as_raw_handle(),
                FileAttributeTagInfo,
                (&raw mut info).cast(),
                super::size_u32::<FILE_ATTRIBUTE_TAG_INFO>(),
            )
        };
        if ok == 0 {
            return Err(RootRefused::Unreadable);
        }
        if info.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(RootRefused::ReparsePoint);
        }
        if info.FileAttributes & FILE_ATTRIBUTE_DIRECTORY == 0 {
            return Err(RootRefused::NotADirectory);
        }
        let owner = owner_sid(&handle).ok_or(RootRefused::Unreadable)?;
        let user = current_user()
            .and_then(|u| u.to_string_sid())
            .ok_or(RootRefused::Unreadable)?;
        if !owner_acceptable(&owner, &user) {
            return Err(RootRefused::ForeignOwner(owner));
        }
        Ok(Self { _handle: handle })
    }
}

/// The owner of an open file or directory, as `S-1-…`.
pub fn owner_sid(handle: &OwnedHandle) -> Option<String> {
    let mut owner = std::ptr::null_mut();
    let mut descriptor = std::ptr::null_mut();
    // SAFETY: `handle` is open with READ_CONTROL; Windows allocates the
    // descriptor `owner` points into, freed below.
    let status = unsafe {
        GetSecurityInfo(
            handle.as_raw_handle(),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION,
            &raw mut owner,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &raw mut descriptor,
        )
    };
    if status != ERROR_SUCCESS || owner.is_null() {
        if !descriptor.is_null() {
            // SAFETY: allocated by GetSecurityInfo.
            unsafe { LocalFree(descriptor) };
        }
        return None;
    }
    let mut text: *mut u16 = std::ptr::null_mut();
    // SAFETY: `owner` is a valid SID inside `descriptor`.
    let converted = unsafe { ConvertSidToStringSidW(owner, &raw mut text) } != 0 && !text.is_null();
    let result = converted.then(|| {
        let mut len = 0;
        // SAFETY: a zero-terminated string from Windows.
        while unsafe { *text.add(len) } != 0 {
            len += 1;
        }
        // SAFETY: `len` units were just scanned.
        String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(text, len) })
    });
    // SAFETY: both were allocated by Windows with LocalAlloc.
    unsafe {
        if !text.is_null() {
            LocalFree(text.cast());
        }
        LocalFree(descriptor);
    }
    result
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::process::Command;

    /// A junction (no admin rights needed, unlike a symlink).
    pub(crate) fn junction(link: &Path, target: &Path) {
        let status = Command::new("cmd")
            .args(["/d", "/c", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .stdout(std::process::Stdio::null())
            .status()
            .unwrap();
        assert!(status.success(), "mklink /J failed");
    }

    #[test]
    fn owners() {
        let me = "S-1-5-21-1-2-3-1001";
        assert!(owner_acceptable(me, me));
        assert!(owner_acceptable("S-1-5-32-544", me));
        assert!(owner_acceptable("S-1-5-18", me));
        assert!(!owner_acceptable("S-1-5-21-1-2-3-1002", me));
        assert!(!owner_acceptable("S-1-1-0", me), "Everyone");
    }

    #[test]
    fn a_real_directory_of_the_user_is_accepted() {
        let dir = tempfile::tempdir().unwrap();
        let guard = RootGuard::open(dir.path()).unwrap();
        // Work below the pinned root still goes on.
        std::fs::create_dir(dir.path().join("child")).unwrap();
        std::fs::write(dir.path().join("child").join("f"), b"x").unwrap();
        // A second call opens it the same way.
        let second = RootGuard::open(dir.path()).unwrap();
        // The root cannot be renamed away while pinned.
        let moved = dir.path().with_extension("moved");
        assert!(std::fs::rename(dir.path(), &moved).is_err());
        drop((guard, second));
        // Files and missing paths are refused.
        let file = dir.path().join("child").join("f");
        assert_eq!(
            RootGuard::open(&file).unwrap_err(),
            RootRefused::NotADirectory
        );
        assert_eq!(
            RootGuard::open(&dir.path().join("missing")).unwrap_err(),
            RootRefused::Unreadable
        );
    }

    #[test]
    fn a_junction_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target");
        std::fs::create_dir(&target).unwrap();
        let link = dir.path().join("link");
        junction(&link, &target);
        assert_eq!(
            RootGuard::open(&link).unwrap_err(),
            RootRefused::ReparsePoint
        );
        std::fs::remove_dir(&link).unwrap();
        assert!(target.exists());
    }
}
