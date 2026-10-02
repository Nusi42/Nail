//! "Start with Windows" (M4b WP113, ADR 0087): the per-user Run key value
//! `Nail` = `"<Nail.exe>" --autostart`. The Run key is the truth: the core
//! reads it back through [`nail_api::desktop::DesktopHost::start_with_windows`],
//! so a user who removes the entry in Task Manager sees the switch off.
//! Started with `--autostart`, the app honours "start hidden" (tray only).

use std::path::Path;

use nail_api::desktop::{AUTOSTART_ARG, registry as keys};

use crate::registry::{Hive, Registry};

/// The Run key value for an executable.
pub fn run_command(exe: &Path) -> String {
    format!("\"{}\" {AUTOSTART_ARG}", exe.display())
}

fn run_value(registry: &dyn Registry, hive: Hive) -> Result<bool, String> {
    registry
        .get(hive, keys::RUN_KEY, keys::RUN_VALUE)
        .map(|value| value.is_some_and(|v| !v.as_string().trim().is_empty()))
}

/// Whether the machine-wide Run key starts Nail for every user (the MSI's
/// `AUTOSTART=1`, WP112). Only an administrator can remove it.
pub fn machine_enabled(registry: &dyn Registry) -> bool {
    run_value(registry, Hive::LocalMachine).unwrap_or(false)
}

/// Whether Nail starts with Windows: the user's Run key or the machine's
/// (`None` when the user's key cannot be read).
pub fn is_enabled(registry: &dyn Registry) -> Option<bool> {
    match run_value(registry, Hive::CurrentUser) {
        Ok(user) => Some(user || machine_enabled(registry)),
        Err(err) => {
            tracing::warn!(%err, "cannot read the Run key");
            None
        }
    }
}

/// Writes or removes the user's Run key value. Writing again refreshes the
/// executable's path (after an update moved it). With the machine-wide
/// entry (MSI `AUTOSTART=1`) no user entry is written (Nail would start
/// twice); switching off then removes only the user's entry, and the
/// machine's keeps the switch on — it is the administrator's choice.
pub fn set_enabled(registry: &dyn Registry, exe: &Path, enabled: bool) -> Result<(), String> {
    if enabled && machine_enabled(registry) {
        registry.delete_value(Hive::CurrentUser, keys::RUN_KEY, keys::RUN_VALUE)
    } else if enabled {
        registry.set_string(
            Hive::CurrentUser,
            keys::RUN_KEY,
            keys::RUN_VALUE,
            &run_command(exe),
        )
    } else {
        registry.delete_value(Hive::CurrentUser, keys::RUN_KEY, keys::RUN_VALUE)
    }
}

/// Whether the window starts hidden: started by the Run key with "start
/// hidden" on, and a tray icon to bring it back.
pub fn starts_hidden(autostart: bool, start_hidden: bool, has_tray: bool) -> bool {
    autostart && start_hidden && has_tray
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::MemoryRegistry;
    use pretty_assertions::assert_eq;

    #[test]
    fn the_run_key_carries_the_quoted_exe_and_the_flag() {
        let exe = Path::new(r"C:\Users\a b\AppData\Local\Programs\Nail\Nail.exe");
        assert_eq!(
            run_command(exe),
            r#""C:\Users\a b\AppData\Local\Programs\Nail\Nail.exe" --autostart"#
        );
        let reg = MemoryRegistry::new();
        assert_eq!(is_enabled(&reg), Some(false));
        set_enabled(&reg, exe, true).unwrap();
        assert_eq!(is_enabled(&reg), Some(true));
        assert_eq!(
            reg.string(Hive::CurrentUser, keys::RUN_KEY, "Nail")
                .as_deref(),
            Some(&*run_command(exe))
        );
        set_enabled(&reg, exe, false).unwrap();
        assert_eq!(is_enabled(&reg), Some(false));
        // Removing twice is fine.
        set_enabled(&reg, exe, false).unwrap();
    }

    /// The MSI's `AUTOSTART=1` writes the machine's Run key (WP112): the
    /// switch is on, no second user entry is written, and switching off
    /// cannot remove the administrator's entry.
    #[test]
    fn the_machine_wide_run_key_counts_and_is_never_duplicated() {
        let exe = Path::new(r"C:\Program Files\Nail\Nail.exe");
        let reg = MemoryRegistry::new();
        reg.set_string(Hive::LocalMachine, keys::RUN_KEY, "Nail", &run_command(exe))
            .unwrap();
        assert!(machine_enabled(&reg));
        assert_eq!(is_enabled(&reg), Some(true));
        set_enabled(&reg, exe, true).unwrap();
        assert_eq!(reg.string(Hive::CurrentUser, keys::RUN_KEY, "Nail"), None);
        set_enabled(&reg, exe, false).unwrap();
        assert_eq!(is_enabled(&reg), Some(true), "managed by the administrator");
        // A user entry from before the MSI is removed when switching on.
        reg.set_string(Hive::CurrentUser, keys::RUN_KEY, "Nail", "old")
            .unwrap();
        set_enabled(&reg, exe, true).unwrap();
        assert_eq!(reg.string(Hive::CurrentUser, keys::RUN_KEY, "Nail"), None);
    }

    #[test]
    fn hidden_only_when_started_by_windows_with_a_tray() {
        assert!(starts_hidden(true, true, true));
        assert!(
            !starts_hidden(false, true, true),
            "a normal start shows the window"
        );
        assert!(!starts_hidden(true, false, true));
        assert!(
            !starts_hidden(true, true, false),
            "no tray: never invisible"
        );
    }

    #[cfg(windows)]
    #[test]
    fn the_real_run_key_below_a_test_root() {
        let root = format!(r"Software\NailTests\startup-{}", std::process::id());
        let reg = crate::registry::WindowsRegistry::below(&root);
        let exe = Path::new(r"C:\Nail\Nail.exe");
        set_enabled(&reg, exe, true).unwrap();
        assert_eq!(is_enabled(&reg), Some(true));
        set_enabled(&reg, exe, false).unwrap();
        assert_eq!(is_enabled(&reg), Some(false));
        reg.remove_root().unwrap();
    }
}
