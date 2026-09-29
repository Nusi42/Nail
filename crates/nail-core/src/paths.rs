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

#[cfg(test)]
mod tests {
    use super::*;

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
