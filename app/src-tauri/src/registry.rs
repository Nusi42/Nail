//! The Windows registry as the desktop shell sees it (M4b WP113, ADR 0087):
//! the installers' facts (`nail_api::desktop::registry`), group policy, the
//! per-user Run key, default-app registration and the Simple MAPI client.
//!
//! Everything goes through [`Registry`] so the rules above it are tested
//! without touching the real registry:
//!
//! * [`WindowsRegistry`] is the real one. With a `root` (the
//!   `NAIL_REGISTRY_ROOT` variable, a key below `HKCU`) both hives map below
//!   that key (`<root>\HKCU\…`, `<root>\HKLM\…`), so tests and e2e runs can
//!   stage installer facts and policies without admin rights or side
//!   effects.
//! * [`MemoryRegistry`] keeps values in memory. Seeded runs (`--seed`) use
//!   it unless `NAIL_REGISTRY_ROOT` is set, so a demo never writes the real
//!   Run key.

use std::collections::HashMap;
use std::sync::Mutex;

/// Environment variable naming a key below `HKCU` that replaces both hives
/// (tests, e2e runs).
pub const REGISTRY_ROOT_ENV: &str = "NAIL_REGISTRY_ROOT";

/// Which hive a key lives in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Hive {
    /// `HKEY_CURRENT_USER`.
    CurrentUser,
    /// `HKEY_LOCAL_MACHINE` (64-bit view).
    LocalMachine,
}

impl Hive {
    /// The short name used below a test root (`HKCU`, `HKLM`).
    pub fn short_name(self) -> &'static str {
        match self {
            Self::CurrentUser => "HKCU",
            Self::LocalMachine => "HKLM",
        }
    }
}

/// A registry value Nail reads: a string (`REG_SZ` / `REG_EXPAND_SZ`) or a
/// number (`REG_DWORD` / `REG_QWORD`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegValue {
    /// A string.
    Str(String),
    /// A number.
    Num(u64),
}

impl RegValue {
    /// The value as a string (numbers in decimal).
    pub fn as_string(&self) -> String {
        match self {
            Self::Str(text) => text.clone(),
            Self::Num(n) => n.to_string(),
        }
    }

    /// The value as a switch: a non-zero number, or a string `1` / `true` /
    /// `yes` (installers write both `REG_DWORD` and `REG_SZ`). `None` for
    /// anything else.
    pub fn as_flag(&self) -> Option<bool> {
        match self {
            Self::Num(n) => Some(*n != 0),
            Self::Str(text) => match text.trim().to_ascii_lowercase().as_str() {
                "1" | "true" | "yes" => Some(true),
                "0" | "false" | "no" | "" => Some(false),
                _ => None,
            },
        }
    }
}

/// Read / write access to the registry (the parts the shell needs).
pub trait Registry: Send + Sync + std::fmt::Debug {
    /// A value; `Ok(None)` when the key or value does not exist. `name` ""
    /// is the key's default value.
    fn get(&self, hive: Hive, key: &str, name: &str) -> Result<Option<RegValue>, String>;
    /// Whether the key exists.
    fn key_exists(&self, hive: Hive, key: &str) -> Result<bool, String>;
    /// Writes a string value, creating the key.
    fn set_string(&self, hive: Hive, key: &str, name: &str, value: &str) -> Result<(), String>;
    /// Removes a value (no error when it does not exist).
    fn delete_value(&self, hive: Hive, key: &str, name: &str) -> Result<(), String>;

    /// [`Self::get`] with errors logged and treated as missing.
    fn value(&self, hive: Hive, key: &str, name: &str) -> Option<RegValue> {
        match self.get(hive, key, name) {
            Ok(value) => value,
            Err(err) => {
                tracing::debug!(%err, key, name, "cannot read a registry value");
                None
            }
        }
    }

    /// A string value (numbers in decimal); `None` when missing or empty.
    fn string(&self, hive: Hive, key: &str, name: &str) -> Option<String> {
        self.value(hive, key, name)
            .map(|v| v.as_string())
            .filter(|s| !s.trim().is_empty())
    }

    /// A switch value ([`RegValue::as_flag`]).
    fn flag(&self, hive: Hive, key: &str, name: &str) -> Option<bool> {
        self.value(hive, key, name).and_then(|v| v.as_flag())
    }
}

/// Registry values in memory (tests, seeded runs). Keys and names are
/// case-insensitive like Windows'.
#[derive(Debug, Default)]
pub struct MemoryRegistry {
    values: Mutex<HashMap<(Hive, String), HashMap<String, RegValue>>>,
}

impl MemoryRegistry {
    /// An empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets a value (any type; for tests).
    pub fn put(&self, hive: Hive, key: &str, name: &str, value: RegValue) {
        self.lock()
            .entry((hive, key.to_ascii_lowercase()))
            .or_default()
            .insert(name.to_ascii_lowercase(), value);
    }

    /// Creates an empty key (for tests).
    pub fn create_key(&self, hive: Hive, key: &str) {
        self.lock()
            .entry((hive, key.to_ascii_lowercase()))
            .or_default();
    }

    fn lock(
        &self,
    ) -> std::sync::MutexGuard<'_, HashMap<(Hive, String), HashMap<String, RegValue>>> {
        self.values
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl Registry for MemoryRegistry {
    fn get(&self, hive: Hive, key: &str, name: &str) -> Result<Option<RegValue>, String> {
        Ok(self
            .lock()
            .get(&(hive, key.to_ascii_lowercase()))
            .and_then(|values| values.get(&name.to_ascii_lowercase()))
            .cloned())
    }

    fn key_exists(&self, hive: Hive, key: &str) -> Result<bool, String> {
        let key = key.to_ascii_lowercase();
        let prefix = format!("{key}\\");
        Ok(self
            .lock()
            .keys()
            .any(|(h, k)| *h == hive && (*k == key || k.starts_with(&prefix))))
    }

    fn set_string(&self, hive: Hive, key: &str, name: &str, value: &str) -> Result<(), String> {
        self.put(hive, key, name, RegValue::Str(value.to_owned()));
        Ok(())
    }

    fn delete_value(&self, hive: Hive, key: &str, name: &str) -> Result<(), String> {
        if let Some(values) = self.lock().get_mut(&(hive, key.to_ascii_lowercase())) {
            values.remove(&name.to_ascii_lowercase());
        }
        Ok(())
    }
}

/// The real registry, optionally below a test root (see the module docs).
#[derive(Debug, Clone, Default)]
pub struct WindowsRegistry {
    root: Option<String>,
}

impl WindowsRegistry {
    /// The real hives.
    pub fn new() -> Self {
        Self { root: None }
    }

    /// Both hives below `HKCU\<root>` (tests).
    pub fn below(root: impl Into<String>) -> Self {
        Self {
            root: Some(root.into()),
        }
    }

    /// The test root, if any.
    pub fn root(&self) -> Option<&str> {
        self.root.as_deref()
    }

    /// The hive and path a key really lives at.
    pub fn resolve(&self, hive: Hive, key: &str) -> (Hive, String) {
        match &self.root {
            Some(root) => (
                Hive::CurrentUser,
                format!("{root}\\{}\\{key}", hive.short_name()),
            ),
            None => (hive, key.to_owned()),
        }
    }
}

#[cfg(windows)]
mod win {
    use super::{Hive, RegValue, WindowsRegistry};

    /// `ERROR_FILE_NOT_FOUND`: key or value missing.
    const NOT_FOUND: i32 = 0x8007_0002_u32 as i32;

    fn base(hive: Hive) -> &'static windows_registry::Key {
        match hive {
            Hive::CurrentUser => windows_registry::CURRENT_USER,
            Hive::LocalMachine => windows_registry::LOCAL_MACHINE,
        }
    }

    fn missing(err: &windows_registry::Result<()>) -> bool {
        matches!(err, Err(e) if e.code().0 == NOT_FOUND)
    }

    impl WindowsRegistry {
        pub(super) fn open(
            &self,
            hive: Hive,
            key: &str,
        ) -> Result<Option<windows_registry::Key>, String> {
            let (hive, path) = self.resolve(hive, key);
            match base(hive).open(&path) {
                Ok(key) => Ok(Some(key)),
                Err(e) if e.code().0 == NOT_FOUND => Ok(None),
                Err(e) => Err(format!("{path}: {e}")),
            }
        }

        pub(super) fn read(
            &self,
            hive: Hive,
            key: &str,
            name: &str,
        ) -> Result<Option<RegValue>, String> {
            let Some(opened) = self.open(hive, key)? else {
                return Ok(None);
            };
            let value = match opened.get_value(name) {
                Ok(value) => value,
                Err(e) if e.code().0 == NOT_FOUND => return Ok(None),
                Err(e) => return Err(format!("{key}\\{name}: {e}")),
            };
            Ok(match value.ty() {
                windows_registry::Type::String | windows_registry::Type::ExpandString => {
                    String::try_from(value).ok().map(RegValue::Str)
                }
                windows_registry::Type::U32 | windows_registry::Type::U64 => {
                    u64::try_from(value).ok().map(RegValue::Num)
                }
                _ => None,
            })
        }

        pub(super) fn write(
            &self,
            hive: Hive,
            key: &str,
            name: &str,
            value: &str,
        ) -> Result<(), String> {
            let (hive, path) = self.resolve(hive, key);
            let opened = base(hive)
                .create(&path)
                .map_err(|e| format!("{path}: {e}"))?;
            opened
                .set_string(name, value)
                .map_err(|e| format!("{path}\\{name}: {e}"))
        }

        pub(super) fn remove(&self, hive: Hive, key: &str, name: &str) -> Result<(), String> {
            let (real_hive, path) = self.resolve(hive, key);
            let opened = match base(real_hive).options().read().write().open(&path) {
                Ok(key) => key,
                Err(e) if e.code().0 == NOT_FOUND => return Ok(()),
                Err(e) => return Err(format!("{path}: {e}")),
            };
            let result = opened.remove_value(name);
            if missing(&result) {
                return Ok(());
            }
            result.map_err(|e| format!("{path}\\{name}: {e}"))
        }

        /// Deletes the whole test root (tests only).
        pub fn remove_root(&self) -> Result<(), String> {
            let Some(root) = &self.root else {
                return Err("not a test registry".into());
            };
            let result = windows_registry::CURRENT_USER.remove_tree(root);
            if missing(&result) {
                return Ok(());
            }
            result.map_err(|e| format!("{root}: {e}"))
        }
    }
}

impl Registry for WindowsRegistry {
    fn get(&self, hive: Hive, key: &str, name: &str) -> Result<Option<RegValue>, String> {
        #[cfg(windows)]
        {
            self.read(hive, key, name)
        }
        #[cfg(not(windows))]
        {
            let _ = (hive, key, name);
            Ok(None)
        }
    }

    fn key_exists(&self, hive: Hive, key: &str) -> Result<bool, String> {
        #[cfg(windows)]
        {
            self.open(hive, key).map(|k| k.is_some())
        }
        #[cfg(not(windows))]
        {
            let _ = (hive, key);
            Ok(false)
        }
    }

    fn set_string(&self, hive: Hive, key: &str, name: &str, value: &str) -> Result<(), String> {
        #[cfg(windows)]
        {
            self.write(hive, key, name, value)
        }
        #[cfg(not(windows))]
        {
            let _ = (hive, key, name, value);
            Err("no registry on this system".into())
        }
    }

    fn delete_value(&self, hive: Hive, key: &str, name: &str) -> Result<(), String> {
        #[cfg(windows)]
        {
            self.remove(hive, key, name)
        }
        #[cfg(not(windows))]
        {
            let _ = (hive, key, name);
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn values_read_as_strings_and_flags() {
        assert_eq!(RegValue::Num(1).as_flag(), Some(true));
        assert_eq!(RegValue::Num(0).as_flag(), Some(false));
        assert_eq!(RegValue::Str(" 1 ".into()).as_flag(), Some(true));
        assert_eq!(RegValue::Str("TRUE".into()).as_flag(), Some(true));
        assert_eq!(RegValue::Str("0".into()).as_flag(), Some(false));
        assert_eq!(RegValue::Str("maybe".into()).as_flag(), None);
        assert_eq!(RegValue::Num(7).as_string(), "7");
    }

    #[test]
    fn the_memory_registry_is_case_insensitive() {
        let reg = MemoryRegistry::new();
        reg.set_string(
            Hive::CurrentUser,
            r"Software\Nail\Install",
            "Scope",
            "per-user",
        )
        .unwrap();
        assert_eq!(
            reg.string(Hive::CurrentUser, r"software\nail\install", "SCOPE")
                .as_deref(),
            Some("per-user")
        );
        assert_eq!(
            reg.string(Hive::LocalMachine, r"Software\Nail\Install", "Scope"),
            None
        );
        assert!(reg.key_exists(Hive::CurrentUser, r"Software\Nail").unwrap());
        assert!(!reg.key_exists(Hive::CurrentUser, r"Software\Nai").unwrap());
        reg.delete_value(Hive::CurrentUser, r"Software\Nail\Install", "scope")
            .unwrap();
        assert_eq!(
            reg.value(Hive::CurrentUser, r"Software\Nail\Install", "Scope"),
            None
        );
        // Deleting what is not there is fine.
        reg.delete_value(Hive::LocalMachine, r"Software\X", "y")
            .unwrap();
    }

    #[test]
    fn a_test_root_maps_both_hives_below_hkcu() {
        let reg = WindowsRegistry::below(r"Software\NailTests\abc");
        assert_eq!(
            reg.resolve(Hive::LocalMachine, r"Software\Policies\Nail"),
            (
                Hive::CurrentUser,
                r"Software\NailTests\abc\HKLM\Software\Policies\Nail".to_owned()
            )
        );
        assert_eq!(
            WindowsRegistry::new().resolve(Hive::LocalMachine, "Software\\X"),
            (Hive::LocalMachine, "Software\\X".to_owned())
        );
    }

    /// The real registry, below a throw-away key under HKCU.
    #[cfg(windows)]
    #[test]
    fn the_windows_registry_reads_and_writes_below_a_test_root() {
        let root = format!(r"Software\NailTests\registry-{}", std::process::id());
        let reg = WindowsRegistry::below(&root);
        let key = r"Software\Nail\Install";
        assert_eq!(reg.get(Hive::LocalMachine, key, "Installer").unwrap(), None);
        assert!(!reg.key_exists(Hive::LocalMachine, key).unwrap());
        reg.set_string(Hive::LocalMachine, key, "Installer", "msi")
            .unwrap();
        assert_eq!(
            reg.get(Hive::LocalMachine, key, "Installer").unwrap(),
            Some(RegValue::Str("msi".into()))
        );
        // A DWORD written by an installer reads as a number.
        windows_registry::CURRENT_USER
            .create(format!(r"{root}\HKLM\{key}"))
            .unwrap()
            .set_u32("DisableUpdates", 1)
            .unwrap();
        assert_eq!(
            reg.flag(Hive::LocalMachine, key, "DisableUpdates"),
            Some(true)
        );
        assert!(reg.key_exists(Hive::LocalMachine, key).unwrap());
        reg.delete_value(Hive::LocalMachine, key, "Installer")
            .unwrap();
        reg.delete_value(Hive::LocalMachine, key, "Installer")
            .unwrap();
        assert_eq!(reg.get(Hive::LocalMachine, key, "Installer").unwrap(), None);
        reg.remove_root().unwrap();
        assert!(!reg.key_exists(Hive::LocalMachine, key).unwrap());
    }
}
