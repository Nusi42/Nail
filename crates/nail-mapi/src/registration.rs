//! The registry entries that make NailMapi.dll the system's Simple MAPI
//! client (`nail_api::mapi::registry`). The installers write them (WP112;
//! HKLM needs elevation); nothing in this crate writes to the registry.
//! [`client_registration`] lists the values, [`to_reg_file`] renders them
//! as a `.reg` file for the installers' tests and for manual setups.
//!
//! What Windows reads (the MAPI stub `mapi32.dll` / `mapistub.dll`; from
//! memory of the docs, confirmed end to end by the installer smoke test —
//! ADR 0088):
//!
//! * `HKLM\Software\Clients\Mail\Nail` (default) = the client's name,
//!   `DLLPathEx` (preferred) and `DLLPath` = the provider DLL. 64-bit
//!   programs read the 64-bit view, 32-bit programs the 32-bit view
//!   (`WOW6432Node`), so each view points at the DLL of its architecture.
//! * The default client: the default value of `Software\Clients\Mail`
//!   (`HKCU` for one user, `HKLM` for the machine). Windows lets only the
//!   user choose it (Settings → Default apps / the first-run screen), so
//!   [`default_client_entry`] is for managed deployments.

use nail_api::desktop::WindowsArch;
use nail_api::mapi::{mapi_dll_relative_path, registry};
use std::fmt::Write as _;

/// The registry root of an entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegistryHive {
    /// `HKEY_LOCAL_MACHINE`.
    LocalMachine,
    /// `HKEY_CURRENT_USER`.
    CurrentUser,
}

impl RegistryHive {
    /// The name in `.reg` files.
    pub fn reg_name(self) -> &'static str {
        match self {
            Self::LocalMachine => "HKEY_LOCAL_MACHINE",
            Self::CurrentUser => "HKEY_CURRENT_USER",
        }
    }
}

/// Which registry view a 64-bit installer writes to (`KEY_WOW64_64KEY` /
/// `KEY_WOW64_32KEY`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegistryView {
    /// The native (64-bit) view: what x64 and ARM64 programs read.
    Native,
    /// The 32-bit view (`Software\WOW6432Node\…` under HKLM): what 32-bit
    /// programs read.
    Wow32,
}

/// A value's data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegistryData {
    /// `REG_SZ`.
    String(String),
}

/// One registry value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistryEntry {
    /// The root.
    pub hive: RegistryHive,
    /// The view.
    pub view: RegistryView,
    /// The key below the root (without `WOW6432Node`).
    pub key: String,
    /// The value's name; `None` = the key's default value.
    pub name: Option<String>,
    /// The data.
    pub data: RegistryData,
}

impl RegistryEntry {
    /// The key as the view shows it under the root (`Software\WOW6432Node\…`
    /// for the 32-bit view of HKLM; HKCU's `Software` is shared).
    pub fn view_key(&self) -> String {
        match (self.view, self.hive) {
            (RegistryView::Wow32, RegistryHive::LocalMachine) => match self.key.split_once('\\') {
                Some((first, rest)) if first.eq_ignore_ascii_case("software") => {
                    format!(r"{first}\WOW6432Node\{rest}")
                }
                _ => self.key.clone(),
            },
            _ => self.key.clone(),
        }
    }
}

fn join(dir: &str, relative: &str) -> String {
    let dir = dir.trim_end_matches(['\\', '/']);
    format!(r"{dir}\{relative}")
}

/// The values registering Nail as a Simple MAPI client for an installation
/// in `install_dir` on a Windows of architecture `os` (x64 or ARM64; an x86
/// Windows only has one view): the native view gets the DLL of the
/// system's architecture, the 32-bit view the x86 DLL.
pub fn client_registration(install_dir: &str, os: WindowsArch) -> Vec<RegistryEntry> {
    let views: Vec<(RegistryView, WindowsArch)> = match os {
        WindowsArch::X86 => vec![(RegistryView::Native, WindowsArch::X86)],
        native => vec![
            (RegistryView::Native, native),
            (RegistryView::Wow32, WindowsArch::X86),
        ],
    };
    let mut entries = Vec::new();
    for (view, arch) in views {
        let dll = join(install_dir, &mapi_dll_relative_path(arch));
        let entry = |name: Option<&str>, data: &str| RegistryEntry {
            hive: RegistryHive::LocalMachine,
            view,
            key: registry::NAIL_MAIL_CLIENT_KEY.to_owned(),
            name: name.map(str::to_owned),
            data: RegistryData::String(data.to_owned()),
        };
        entries.push(entry(None, registry::CLIENT_NAME));
        entries.push(entry(Some(registry::DLL_PATH_VALUE), &dll));
        entries.push(entry(Some(registry::DLL_PATH_EX_VALUE), &dll));
    }
    entries
}

/// The keys an uninstaller removes (with their values): Nail's client key
/// in every view [`client_registration`] wrote.
pub fn client_unregistration(os: WindowsArch) -> Vec<(RegistryHive, RegistryView, String)> {
    let mut keys = vec![(
        RegistryHive::LocalMachine,
        RegistryView::Native,
        registry::NAIL_MAIL_CLIENT_KEY.to_owned(),
    )];
    if os != WindowsArch::X86 {
        keys.push((
            RegistryHive::LocalMachine,
            RegistryView::Wow32,
            registry::NAIL_MAIL_CLIENT_KEY.to_owned(),
        ));
    }
    keys
}

/// Makes Nail the default Simple MAPI client (for managed deployments;
/// Windows leaves this choice to the user otherwise).
pub fn default_client_entry(hive: RegistryHive) -> RegistryEntry {
    RegistryEntry {
        hive,
        view: RegistryView::Native,
        key: registry::MAIL_CLIENTS_KEY.to_owned(),
        name: None,
        data: RegistryData::String(registry::CLIENT_NAME.to_owned()),
    }
}

fn reg_escape(text: &str) -> String {
    text.replace('\\', r"\\").replace('"', "\\\"")
}

/// Renders entries as a `.reg` file (Registry Editor 5.00, CRLF line
/// ends), grouped by key in the given order.
pub fn to_reg_file(entries: &[RegistryEntry]) -> String {
    let mut out = String::from("Windows Registry Editor Version 5.00\r\n");
    let mut current: Option<String> = None;
    for entry in entries {
        let key = format!(r"{}\{}", entry.hive.reg_name(), entry.view_key());
        if current.as_deref() != Some(key.as_str()) {
            // Writing to a String cannot fail.
            let _ = write!(out, "\r\n[{key}]\r\n");
            current = Some(key);
        }
        let name = entry
            .name
            .as_deref()
            .map_or_else(|| "@".to_owned(), |n| format!("\"{}\"", reg_escape(n)));
        let RegistryData::String(data) = &entry.data;
        let _ = write!(out, "{name}=\"{}\"\r\n", reg_escape(data));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn x64_registers_both_views() {
        let entries = client_registration(r"C:\Program Files\Nail\", WindowsArch::X64);
        assert_eq!(entries.len(), 6);
        let native: Vec<_> = entries
            .iter()
            .filter(|e| e.view == RegistryView::Native)
            .collect();
        assert!(native.iter().all(
            |e| e.hive == RegistryHive::LocalMachine && e.key == r"Software\Clients\Mail\Nail"
        ));
        assert_eq!(native[0].name, None);
        assert_eq!(native[0].data, RegistryData::String("Nail".into()));
        assert_eq!(native[1].name.as_deref(), Some("DLLPath"));
        assert_eq!(
            native[1].data,
            RegistryData::String(r"C:\Program Files\Nail\mapi\x64\NailMapi.dll".into())
        );
        assert_eq!(native[2].name.as_deref(), Some("DLLPathEx"));
        let wow: Vec<_> = entries
            .iter()
            .filter(|e| e.view == RegistryView::Wow32)
            .collect();
        assert_eq!(
            wow[1].data,
            RegistryData::String(r"C:\Program Files\Nail\mapi\x86\NailMapi.dll".into())
        );
        assert_eq!(wow[1].view_key(), r"Software\WOW6432Node\Clients\Mail\Nail");
    }

    #[test]
    fn arm64_and_x86_systems() {
        let entries = client_registration(r"C:\Nail", WindowsArch::Arm64);
        assert_eq!(
            entries[1].data,
            RegistryData::String(r"C:\Nail\mapi\arm64\NailMapi.dll".into())
        );
        assert_eq!(
            entries[4].data,
            RegistryData::String(r"C:\Nail\mapi\x86\NailMapi.dll".into())
        );
        let entries = client_registration(r"C:\Nail", WindowsArch::X86);
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].view, RegistryView::Native);
        assert_eq!(client_unregistration(WindowsArch::X86).len(), 1);
        assert_eq!(client_unregistration(WindowsArch::X64).len(), 2);
    }

    #[test]
    fn default_client_and_hkcu_views() {
        let entry = default_client_entry(RegistryHive::CurrentUser);
        assert_eq!(entry.key, r"Software\Clients\Mail");
        assert_eq!(entry.name, None);
        let mut wow = entry.clone();
        wow.view = RegistryView::Wow32;
        // HKCU\Software is shared between the views.
        assert_eq!(wow.view_key(), r"Software\Clients\Mail");
    }

    #[test]
    fn reg_file_rendering() {
        let mut entries = client_registration(r"C:\Program Files\Nail", WindowsArch::X64);
        entries.push(default_client_entry(RegistryHive::LocalMachine));
        let text = to_reg_file(&entries);
        let expected = concat!(
            "Windows Registry Editor Version 5.00\r\n",
            "\r\n[HKEY_LOCAL_MACHINE\\Software\\Clients\\Mail\\Nail]\r\n",
            "@=\"Nail\"\r\n",
            "\"DLLPath\"=\"C:\\\\Program Files\\\\Nail\\\\mapi\\\\x64\\\\NailMapi.dll\"\r\n",
            "\"DLLPathEx\"=\"C:\\\\Program Files\\\\Nail\\\\mapi\\\\x64\\\\NailMapi.dll\"\r\n",
            "\r\n[HKEY_LOCAL_MACHINE\\Software\\WOW6432Node\\Clients\\Mail\\Nail]\r\n",
            "@=\"Nail\"\r\n",
            "\"DLLPath\"=\"C:\\\\Program Files\\\\Nail\\\\mapi\\\\x86\\\\NailMapi.dll\"\r\n",
            "\"DLLPathEx\"=\"C:\\\\Program Files\\\\Nail\\\\mapi\\\\x86\\\\NailMapi.dll\"\r\n",
            "\r\n[HKEY_LOCAL_MACHINE\\Software\\Clients\\Mail]\r\n",
            "@=\"Nail\"\r\n",
        );
        assert_eq!(text, expected);
        assert_eq!(reg_escape(r#"a"b\c"#), r#"a\"b\\c"#);
    }
}
