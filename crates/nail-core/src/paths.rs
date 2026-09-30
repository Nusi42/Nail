//! Default locations of user data (design.md, "Windows installer"):
//! settings, `theme.kdl` and `layout.kdl` in the roaming config directory
//! (`%APPDATA%\Nail`), the store in the local data directory
//! (`%LOCALAPPDATA%\Nail`).

use std::path::PathBuf;

/// Name of Nail's directory inside the platform directories.
pub const APP_DIR_NAME: &str = "Nail";

/// The default config directory (`%APPDATA%\Nail` on Windows), `None` when
/// the platform has no such directory.
pub fn default_config_dir() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join(APP_DIR_NAME))
}

/// The default data directory (`%LOCALAPPDATA%\Nail` on Windows), `None`
/// when the platform has no such directory.
pub fn default_data_dir() -> Option<PathBuf> {
    dirs::data_local_dir().map(|d| d.join(APP_DIR_NAME))
}

/// Takes the single-instance lock of a data directory for `owner`
/// (`"desktop"`, `"serve"`, …). When another Nail process holds it, the
/// error is the user-facing text in `locale` (`cli-data-dir-in-use-by`).
pub fn lock_data_dir(
    dir: &std::path::Path,
    owner: &str,
    locale: Option<&str>,
) -> Result<nail_store::DataDirLock, String> {
    nail_store::DataDirLock::acquire(dir, owner).map_err(|err| {
        let message = match &err {
            nail_store::LockError::Held(Some(holder)) => {
                nail_api::LocalizedMessage::new("cli-data-dir-in-use-by")
                    .arg("owner", holder.owner.as_str())
                    .arg("pid", u64::from(holder.pid))
            }
            nail_store::LockError::Held(None) => {
                nail_api::LocalizedMessage::new("cli-data-dir-in-use")
            }
            nail_store::LockError::Io(_) => {
                return err.to_string();
            }
        };
        crate::strings::render(locale, &message)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_held_data_directory_is_refused_with_a_text() {
        let dir = tempfile::tempdir().unwrap();
        let held = lock_data_dir(dir.path(), "desktop", None).unwrap();
        let refused = lock_data_dir(dir.path(), "serve", None).unwrap_err();
        assert!(refused.contains("desktop"), "{refused}");
        assert!(!refused.contains("cli-data-dir"), "rendered: {refused}");
        drop(held);
        assert!(lock_data_dir(dir.path(), "serve", None).is_ok());
    }

    #[test]
    fn defaults_end_in_the_app_dir() {
        for dir in [default_config_dir(), default_data_dir()]
            .into_iter()
            .flatten()
        {
            assert!(dir.ends_with(APP_DIR_NAME), "{}", dir.display());
        }
    }
}
