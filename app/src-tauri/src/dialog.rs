//! The system's file dialogs for `PickPath` (M4c, ADRs 0094, 0099): open a
//! file, open a folder, save a file — the Windows common item dialog
//! (`IFileOpenDialog` / `IFileSaveDialog`) on a thread of its own with COM
//! in single-threaded apartment mode, owned by the main window.

#![allow(unsafe_code)] // COM calls, each with its SAFETY note.

use nail_api::desktop::{PathPick, PathPickKind};

/// What the dialog shows, already localized.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DialogSpec {
    /// Open file, open folder or save file.
    pub kind: PathPickKind,
    /// The title.
    pub title: String,
    /// Filters: name and extensions (`*.mbox;*.mbx`; empty = all files).
    pub filters: Vec<(String, String)>,
    /// The start folder, if any.
    pub folder: Option<String>,
    /// The suggested file name (save), if any.
    pub file_name: Option<String>,
}

/// Builds the dialog's texts from a request (`localize` turns Fluent ids
/// into text).
pub fn spec(pick: &PathPick, localize: impl Fn(&str) -> String) -> DialogSpec {
    let filters = pick
        .filters
        .iter()
        .map(|f| {
            let pattern = if f.extensions.is_empty() {
                "*.*".to_owned()
            } else {
                f.extensions
                    .iter()
                    .map(|e| format!("*.{}", e.trim_start_matches('.')))
                    .collect::<Vec<_>>()
                    .join(";")
            };
            (localize(&f.label_id), pattern)
        })
        .collect();
    let (folder, file_name) = match pick.default_path.as_deref().map(std::path::Path::new) {
        None => (None, None),
        Some(path) if pick.kind == PathPickKind::SaveFile => (
            path.parent()
                .filter(|p| p.is_dir())
                .map(|p| p.display().to_string()),
            path.file_name().map(|n| n.to_string_lossy().into_owned()),
        ),
        Some(path) if path.is_dir() => (Some(path.display().to_string()), None),
        Some(path) => (
            path.parent()
                .filter(|p| p.is_dir())
                .map(|p| p.display().to_string()),
            None,
        ),
    };
    DialogSpec {
        kind: pick.kind,
        title: localize(&pick.title_id),
        filters,
        folder,
        file_name,
    }
}

/// Shows the dialog (blocking; call on a thread of its own). `owner` is the
/// main window's handle. `Ok(None)` = cancelled.
#[cfg(windows)]
pub fn show(spec: &DialogSpec, owner: Option<isize>) -> Result<Option<String>, String> {
    use windows::Win32::Foundation::{ERROR_CANCELLED, HWND};
    use windows::Win32::System::Com::{
        CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
        CoTaskMemFree, CoUninitialize,
    };
    use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
    use windows::Win32::UI::Shell::{
        FOS_FORCEFILESYSTEM, FOS_NOCHANGEDIR, FOS_OVERWRITEPROMPT, FOS_PATHMUSTEXIST,
        FOS_PICKFOLDERS, FileOpenDialog, FileSaveDialog, IFileDialog, IShellItem,
        SHCreateItemFromParsingName, SIGDN_FILESYSPATH,
    };
    use windows::core::{HSTRING, Interface, PCWSTR};

    // SAFETY: COM is initialized for this thread for the dialog's lifetime
    // and uninitialized after; every pointer passed lives across the call
    // (the HSTRINGs and filter vectors outlive `Show`); the returned path
    // is freed with CoTaskMemFree.
    unsafe {
        CoInitializeEx(None, COINIT_APARTMENTTHREADED)
            .ok()
            .map_err(|e| e.to_string())?;
        let result = (|| -> windows::core::Result<Option<String>> {
            let dialog: IFileDialog = if spec.kind == PathPickKind::SaveFile {
                CoCreateInstance::<_, windows::Win32::UI::Shell::IFileSaveDialog>(
                    &FileSaveDialog,
                    None,
                    CLSCTX_INPROC_SERVER,
                )?
                .cast()?
            } else {
                CoCreateInstance::<_, windows::Win32::UI::Shell::IFileOpenDialog>(
                    &FileOpenDialog,
                    None,
                    CLSCTX_INPROC_SERVER,
                )?
                .cast()?
            };
            let mut options = dialog.GetOptions()? | FOS_FORCEFILESYSTEM | FOS_NOCHANGEDIR;
            match spec.kind {
                PathPickKind::OpenFolder => options |= FOS_PICKFOLDERS | FOS_PATHMUSTEXIST,
                PathPickKind::OpenFile => options |= FOS_PATHMUSTEXIST,
                PathPickKind::SaveFile => options |= FOS_OVERWRITEPROMPT,
            }
            dialog.SetOptions(options)?;
            let title = HSTRING::from(spec.title.as_str());
            dialog.SetTitle(&title)?;
            let names: Vec<HSTRING> = spec
                .filters
                .iter()
                .map(|(n, _)| HSTRING::from(n.as_str()))
                .collect();
            let patterns: Vec<HSTRING> = spec
                .filters
                .iter()
                .map(|(_, p)| HSTRING::from(p.as_str()))
                .collect();
            if spec.kind != PathPickKind::OpenFolder && !names.is_empty() {
                let filters: Vec<COMDLG_FILTERSPEC> = names
                    .iter()
                    .zip(&patterns)
                    .map(|(n, p)| COMDLG_FILTERSPEC {
                        pszName: PCWSTR(n.as_ptr()),
                        pszSpec: PCWSTR(p.as_ptr()),
                    })
                    .collect();
                dialog.SetFileTypes(&filters)?;
            }
            if let Some(folder) = &spec.folder {
                let item: IShellItem =
                    SHCreateItemFromParsingName(&HSTRING::from(folder.as_str()), None)?;
                dialog.SetFolder(&item)?;
            }
            if let Some(name) = &spec.file_name {
                dialog.SetFileName(&HSTRING::from(name.as_str()))?;
            }
            let owner = owner.map(|h| HWND(h as *mut core::ffi::c_void));
            if let Err(err) = dialog.Show(owner) {
                if err.code() == ERROR_CANCELLED.to_hresult() {
                    return Ok(None);
                }
                return Err(err);
            }
            let item = dialog.GetResult()?;
            let raw = item.GetDisplayName(SIGDN_FILESYSPATH)?;
            let path = raw
                .to_string()
                .map_err(|_| windows::core::Error::from_thread());
            CoTaskMemFree(Some(raw.0 as *const core::ffi::c_void));
            Ok(Some(path?))
        })();
        CoUninitialize();
        result.map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nail_api::desktop::PathFilter;

    #[test]
    fn specs_carry_localized_texts_filters_and_start_folders() {
        let dir = tempfile::tempdir().unwrap();
        let pick = PathPick {
            kind: PathPickKind::SaveFile,
            title_id: "portability-export-title".into(),
            filters: vec![
                PathFilter {
                    label_id: "portability-filter-mbox".into(),
                    extensions: vec!["mbox".into(), ".mbx".into()],
                },
                PathFilter {
                    label_id: "portability-filter-all".into(),
                    extensions: Vec::new(),
                },
            ],
            default_path: Some(dir.path().join("Inbox.mbox").display().to_string()),
        };
        let spec = spec(&pick, |id| format!("<{id}>"));
        assert_eq!(spec.title, "<portability-export-title>");
        assert_eq!(
            spec.filters,
            vec![
                (
                    "<portability-filter-mbox>".to_owned(),
                    "*.mbox;*.mbx".to_owned()
                ),
                ("<portability-filter-all>".to_owned(), "*.*".to_owned()),
            ]
        );
        assert_eq!(spec.folder, Some(dir.path().display().to_string()));
        assert_eq!(spec.file_name.as_deref(), Some("Inbox.mbox"));
        let folder = PathPick {
            kind: PathPickKind::OpenFolder,
            title_id: "t".into(),
            filters: Vec::new(),
            default_path: Some(dir.path().display().to_string()),
        };
        let spec2 = super::spec(&folder, str::to_owned);
        assert_eq!(spec2.folder, Some(dir.path().display().to_string()));
        assert_eq!(spec2.file_name, None);
    }
}
