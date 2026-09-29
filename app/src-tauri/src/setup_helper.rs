//! Work the NSIS installer delegates to `Nail.exe` (plan M2b WP44,
//! `installer/hooks.nsh`), because NSIS cannot do it safely:
//!
//! * `--setup-path-add <DIR>` / `--setup-path-remove <DIR>`: add or remove
//!   one entry of the per-user `PATH` (`HKCU\Environment\Path`). NSIS
//!   strings are limited to 1024 characters and would truncate a long
//!   `PATH`; here the value keeps its type (`REG_EXPAND_SZ` stays expandable)
//!   and every other entry unchanged.
//! * `--setup-purge-credentials`: delete Nail's entries in the Windows
//!   Credential Manager (only when the user ticks "Also delete my mail data
//!   and settings" at uninstall). Nail's secrets are `keychain:nail/<name>`
//!   references (`nail_api::accounts::SecretRef`), stored by `keyring` as
//!   generic credentials with the target name `<name>.nail`.
//!
//! These run before Tauri starts and exit with a status code; they print
//! nothing (the installer only checks the exit code).

use nail_api::accounts::SecretRef;

/// What the installer asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SetupAction {
    /// Add a directory to the per-user `PATH`.
    PathAdd(String),
    /// Remove a directory from the per-user `PATH`.
    PathRemove(String),
    /// Delete Nail's credentials.
    PurgeCredentials,
}

/// Registry key (under `HKEY_CURRENT_USER`) holding the per-user `PATH`.
pub const ENVIRONMENT_KEY: &str = "Environment";

/// Comparable form of a `PATH` entry: without surrounding quotes and
/// spaces, without trailing backslashes, case-folded (Windows paths are
/// case-insensitive).
fn normalize(entry: &str) -> String {
    entry
        .trim()
        .trim_matches('"')
        .trim()
        .trim_end_matches(['\\', '/'])
        .to_lowercase()
}

/// `path` with `dir` appended; `None` when it is already there.
pub fn path_with(path: &str, dir: &str) -> Option<String> {
    let wanted = normalize(dir);
    if wanted.is_empty() || path.split(';').any(|e| normalize(e) == wanted) {
        return None;
    }
    let trimmed = path.trim_end_matches(';');
    Some(if trimmed.is_empty() {
        dir.to_owned()
    } else {
        format!("{trimmed};{dir}")
    })
}

/// `path` without any entry equal to `dir`; `None` when there was none.
pub fn path_without(path: &str, dir: &str) -> Option<String> {
    let wanted = normalize(dir);
    let kept: Vec<&str> = path
        .split(';')
        .filter(|e| wanted.is_empty() || normalize(e) != wanted)
        .collect();
    let removed = kept.len() != path.split(';').count();
    removed.then(|| kept.join(";"))
}

/// Whether a Credential Manager target name is one of Nail's entries
/// (`<name>.nail`, including the `<name>#<n>.nail` chunks of long values).
pub fn is_nail_credential(target: &str) -> bool {
    target
        .strip_suffix(SecretRef::SERVICE)
        .and_then(|rest| rest.strip_suffix('.'))
        .is_some_and(|name| !name.is_empty())
}

/// Runs `action`; the process exit code (0 = done or nothing to do).
pub fn run(action: &SetupAction) -> i32 {
    let result = match action {
        SetupAction::PathAdd(dir) => os::update_user_path(ENVIRONMENT_KEY, |p| path_with(p, dir)),
        SetupAction::PathRemove(dir) => {
            os::update_user_path(ENVIRONMENT_KEY, |p| path_without(p, dir))
        }
        SetupAction::PurgeCredentials => os::purge_credentials(is_nail_credential).map(|n| {
            tracing::info!(deleted = n, "removed Nail's credentials");
        }),
    };
    match result {
        Ok(()) => 0,
        Err(err) => {
            tracing::error!(%err, ?action, "setup action failed");
            1
        }
    }
}

#[cfg(windows)]
#[allow(unsafe_code)] // Credential Manager and window-message FFI only.
mod os {
    use windows::Win32::Foundation::{LPARAM, WPARAM};
    use windows::Win32::Security::Credentials::{
        CRED_TYPE_GENERIC, CREDENTIALW, CredDeleteW, CredEnumerateW, CredFree,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        HWND_BROADCAST, SMTO_ABORTIFHUNG, SendMessageTimeoutW, WM_SETTINGCHANGE,
    };
    use windows::core::{HSTRING, PCWSTR};
    use windows_registry::{CURRENT_USER, Type};

    pub(super) type Error = Box<dyn std::error::Error>;

    /// Rewrites the `Path` value of `HKCU\<key>` with `change` (which
    /// returns `None` for "nothing to do"), keeping the value's type, then
    /// tells running programs (Explorer) that the environment changed.
    pub(super) fn update_user_path(
        key: &str,
        change: impl FnOnce(&str) -> Option<String>,
    ) -> Result<(), Error> {
        let key = CURRENT_USER.create(key)?;
        let (current, expand) = match key.get_type("Path") {
            Ok(Type::ExpandString) => (key.get_string("Path")?, true),
            Ok(Type::String) => (key.get_string("Path")?, false),
            Ok(other) => return Err(format!("Path has the unexpected type {other:?}").into()),
            // No per-user PATH yet.
            Err(_) => (String::new(), true),
        };
        let Some(updated) = change(&current) else {
            return Ok(());
        };
        if expand {
            key.set_expand_string("Path", &updated)?;
        } else {
            key.set_string("Path", &updated)?;
        }
        broadcast_environment_change();
        Ok(())
    }

    fn broadcast_environment_change() {
        let area = HSTRING::from("Environment");
        // SAFETY: `area` outlives the call; the message only reads it.
        unsafe {
            SendMessageTimeoutW(
                HWND_BROADCAST,
                WM_SETTINGCHANGE,
                WPARAM(0),
                LPARAM(area.as_ptr() as isize),
                SMTO_ABORTIFHUNG,
                5000,
                None,
            );
        }
    }

    /// Deletes every generic credential whose target name matches;
    /// returns how many were deleted.
    pub(super) fn purge_credentials(matches: impl Fn(&str) -> bool) -> Result<usize, Error> {
        let mut targets = Vec::new();
        let mut count = 0u32;
        let mut list: *mut *mut CREDENTIALW = std::ptr::null_mut();
        // SAFETY: CredEnumerateW fills `count` / `list`; the list is read
        // only within its `count` bounds and freed with CredFree.
        unsafe {
            if CredEnumerateW(PCWSTR::null(), None, &mut count, &mut list).is_err() {
                // ERROR_NOT_FOUND: no credentials at all.
                return Ok(0);
            }
            for i in 0..count as usize {
                let credential = &**list.add(i);
                if credential.Type != CRED_TYPE_GENERIC || credential.TargetName.is_null() {
                    continue;
                }
                if let Ok(target) = credential.TargetName.to_string()
                    && matches(&target)
                {
                    targets.push(target);
                }
            }
            CredFree(list as *const core::ffi::c_void);
        }
        let mut deleted = 0;
        for target in targets {
            // SAFETY: the target name is a valid, NUL-terminated HSTRING.
            if unsafe { CredDeleteW(&HSTRING::from(target.as_str()), CRED_TYPE_GENERIC, None) }
                .is_ok()
            {
                deleted += 1;
            }
        }
        Ok(deleted)
    }
}

#[cfg(not(windows))]
mod os {
    pub(super) type Error = Box<dyn std::error::Error>;

    pub(super) fn update_user_path(
        _key: &str,
        _change: impl FnOnce(&str) -> Option<String>,
    ) -> Result<(), Error> {
        Err("the per-user PATH is a Windows setting".into())
    }

    pub(super) fn purge_credentials(_matches: impl Fn(&str) -> bool) -> Result<usize, Error> {
        Err("the Credential Manager is a Windows feature".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    const BIN: &str = r"C:\Users\ada\AppData\Local\Programs\Nail\bin";

    #[test]
    fn adding_appends_once() {
        assert_eq!(path_with("", BIN).as_deref(), Some(BIN));
        assert_eq!(
            path_with(r"%USERPROFILE%\bin;C:\tools;", BIN),
            Some(format!(r"%USERPROFILE%\bin;C:\tools;{BIN}"))
        );
        // Already there, in another spelling.
        let upper = format!(r"C:\tools;{}\", BIN.to_uppercase());
        assert_eq!(path_with(&upper, BIN), None);
        assert_eq!(path_with(&format!("\"{BIN}\""), BIN), None);
        assert_eq!(path_with("C:\\tools", "  "), None);
    }

    #[test]
    fn removing_keeps_every_other_entry() {
        let path = format!(r"C:\a;{BIN}\;%LOCALAPPDATA%\x;;{BIN}");
        assert_eq!(
            path_without(&path, BIN).as_deref(),
            Some(r"C:\a;%LOCALAPPDATA%\x;")
        );
        assert_eq!(path_without(r"C:\a;C:\b", BIN), None);
        assert_eq!(path_without("", BIN), None);
        // Removing what adding produced restores the original.
        let original = r"C:\a;C:\b";
        let added = path_with(original, BIN).unwrap();
        assert_eq!(path_without(&added, BIN).as_deref(), Some(original));
    }

    #[test]
    fn a_long_path_survives_intact() {
        // Longer than NSIS's 1024-character strings.
        let long: Vec<String> = (0..200).map(|i| format!(r"C:\tools\t{i}")).collect();
        let long = long.join(";");
        let added = path_with(&long, BIN).unwrap();
        assert!(added.starts_with(&long));
        assert_eq!(path_without(&added, BIN).unwrap(), long);
    }

    #[test]
    fn only_nails_credentials_match() {
        assert!(is_nail_credential("work/password.nail"));
        assert!(is_nail_credential("work/refresh-token#2.nail"));
        assert!(is_nail_credential("vault-0123456789abcdef/data-key.nail"));
        for other in [
            ".nail",
            "nail",
            "work/password.nail2",
            "work/password.other",
            "git:https://github.com",
            "Microsoft_OC1:uri=ada@example.org",
        ] {
            assert!(!is_nail_credential(other), "{other}");
        }
    }

    /// The registry round trip on a scratch key (never the real
    /// `HKCU\Environment`): types are kept, repeated calls are no-ops.
    #[cfg(windows)]
    #[test]
    fn user_path_round_trip_on_a_scratch_key() {
        use windows_registry::{CURRENT_USER, Type};
        let key_path = format!(r"Software\Nail-test-{}", std::process::id());
        let key = CURRENT_USER.create(&key_path).unwrap();
        key.set_expand_string("Path", r"%USERPROFILE%\bin").unwrap();

        os::update_user_path(&key_path, |p| path_with(p, BIN)).unwrap();
        assert_eq!(key.get_type("Path").unwrap(), Type::ExpandString);
        assert_eq!(
            key.get_string("Path").unwrap(),
            format!(r"%USERPROFILE%\bin;{BIN}")
        );
        os::update_user_path(&key_path, |p| path_with(p, BIN)).unwrap();
        os::update_user_path(&key_path, |p| path_without(p, BIN)).unwrap();
        assert_eq!(key.get_string("Path").unwrap(), r"%USERPROFILE%\bin");

        // A plain REG_SZ stays REG_SZ.
        key.set_string("Path", r"C:\a").unwrap();
        os::update_user_path(&key_path, |p| path_with(p, BIN)).unwrap();
        assert_eq!(key.get_type("Path").unwrap(), Type::String);

        // No value yet: created as REG_EXPAND_SZ.
        key.remove_value("Path").unwrap();
        os::update_user_path(&key_path, |p| path_with(p, BIN)).unwrap();
        assert_eq!(key.get_type("Path").unwrap(), Type::ExpandString);
        assert_eq!(key.get_string("Path").unwrap(), BIN);

        drop(key);
        CURRENT_USER.remove_tree(&key_path).unwrap();
    }

    /// Enumeration runs without deleting anything (the predicate matches
    /// nothing); the real Credential Manager is never modified by tests.
    #[cfg(windows)]
    #[test]
    fn credential_enumeration_works() {
        assert_eq!(os::purge_credentials(|_| false).unwrap(), 0);
    }
}
