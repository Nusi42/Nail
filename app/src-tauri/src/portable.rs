//! Portable mode (design "Windows installer & distribution"): when a file
//! named [`MARKER`] sits next to the executable, Nail keeps its data,
//! config and WebView2 profile beside the executable instead of in the
//! user profile. The portable zip (`scripts/build-installer.mjs`) ships
//! with the marker; the installer never creates it.

use std::path::{Path, PathBuf};

/// Name of the marker file next to `Nail.exe`.
pub const MARKER: &str = "portable";

/// The directory holding `data/`, `config/` and `webview/` in portable
/// mode: the executable's directory when the marker is there.
pub fn root_beside(executable: &Path) -> Option<PathBuf> {
    let dir = executable.parent()?;
    dir.join(MARKER).is_file().then(|| dir.to_path_buf())
}

/// [`root_beside`] for the running executable.
pub fn root() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()
        .and_then(|exe| root_beside(&exe))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_marker_file_switches_to_portable_mode() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("Nail.exe");
        assert_eq!(root_beside(&exe), None);

        // A directory with the marker's name is not the marker.
        std::fs::create_dir(dir.path().join(MARKER)).unwrap();
        assert_eq!(root_beside(&exe), None);
        std::fs::remove_dir(dir.path().join(MARKER)).unwrap();

        std::fs::write(dir.path().join(MARKER), b"").unwrap();
        assert_eq!(root_beside(&exe).as_deref(), Some(dir.path()));
    }

    #[test]
    fn the_test_runner_is_not_portable() {
        assert_eq!(root(), None);
    }
}
