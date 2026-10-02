//! Dev mode: watching a plug-in folder for changes (`nail plugin dev`).
//!
//! Changes are debounced (a bundler writes several files at once); changes
//! only under `node_modules`, `.git` or other dot folders are ignored. The
//! callback runs on the watcher's thread and should hand the reload to the
//! host's runtime (see [`crate::PluginHost::load_dev`]).

use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use notify_debouncer_mini::notify::{RecommendedWatcher, RecursiveMode};
use notify_debouncer_mini::{DebounceEventResult, Debouncer, new_debouncer};

/// How long the watcher waits for a burst of changes to end.
pub const DEBOUNCE: Duration = Duration::from_millis(300);

/// A finished (or skipped) reload of a plug-in, as broadcast by
/// [`crate::PluginHost::subscribe_reloads`]: after a change in a dev folder
/// or a call of [`crate::PluginHost::reload`]. Reloads of one host run one
/// at a time, so a notice describes the state the reload left behind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginReload {
    /// The plug-in.
    pub plugin_id: nail_api::PluginId,
    /// What the reload did.
    pub outcome: ReloadOutcome,
    /// The plug-in's start generation afterwards (`None` when the host no
    /// longer knows it).
    pub generation: Option<u64>,
}

/// What a reload did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReloadOutcome {
    /// Started again with what is in the folder (or the stored spec).
    Started,
    /// Started again, but its start failed (the error is in its log).
    Failed,
    /// The folder's manifest is not valid; the plug-in stays stopped.
    Invalid,
    /// A change arrived while the dev plug-in was disabled (also one
    /// queued behind another reload); nothing ran.
    SkippedDisabled,
}

/// Watches a folder while it lives.
pub struct DevWatcher {
    _debouncer: Debouncer<RecommendedWatcher>,
}

impl std::fmt::Debug for DevWatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("DevWatcher")
    }
}

/// Whether a change at `path` should reload the plug-in: not below
/// `node_modules` or a dot folder of the plug-in folder (one of `roots`,
/// the folder as given and canonical). Paths outside the roots count.
pub fn is_relevant(roots: &[PathBuf], path: &Path) -> bool {
    let Some(relative) = roots.iter().find_map(|r| path.strip_prefix(r).ok()) else {
        return true;
    };
    !relative.components().any(|c| match c {
        Component::Normal(name) => {
            let name = name.to_string_lossy();
            name == "node_modules" || name.starts_with('.')
        }
        _ => false,
    })
}

impl DevWatcher {
    /// Calls `on_change` after relevant changes below `folder`.
    pub fn watch(
        folder: &Path,
        on_change: impl Fn() + Send + 'static,
    ) -> Result<Self, notify_debouncer_mini::notify::Error> {
        let root: PathBuf = folder
            .canonicalize()
            .unwrap_or_else(|_| folder.to_path_buf());
        let mut roots = vec![folder.to_path_buf(), root.clone()];
        // `canonicalize` on Windows adds the `\\?\` prefix; events may
        // come without it.
        if let Some(plain) = root.to_str().and_then(|r| r.strip_prefix(r"\\?\")) {
            roots.push(PathBuf::from(plain));
        }
        let mut debouncer = new_debouncer(DEBOUNCE, move |result: DebounceEventResult| {
            if let Ok(events) = result
                && events.iter().any(|e| is_relevant(&roots, &e.path))
            {
                on_change();
            }
        })?;
        debouncer.watcher().watch(&root, RecursiveMode::Recursive)?;
        Ok(Self {
            _debouncer: debouncer,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use super::*;

    #[test]
    fn ignores_dependencies_and_dot_folders() {
        let roots = [PathBuf::from("/tmp/.tmp1/p")];
        let root = roots.as_slice();
        assert!(is_relevant(root, Path::new("/tmp/.tmp1/p/dist/index.js")));
        assert!(is_relevant(root, Path::new("/tmp/.tmp1/p/manifest.json")));
        assert!(!is_relevant(
            root,
            Path::new("/tmp/.tmp1/p/node_modules/x/index.js")
        ));
        assert!(!is_relevant(root, Path::new("/tmp/.tmp1/p/.git/HEAD")));
        assert!(!is_relevant(root, Path::new("/tmp/.tmp1/p/.cache/x")));
        assert!(is_relevant(root, Path::new("/elsewhere/.x")));
    }

    #[test]
    fn reports_changes() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("index.js"), "1").unwrap();
        let (tx, rx) = mpsc::channel();
        let _watcher = DevWatcher::watch(dir.path(), move || {
            let _ = tx.send(());
        })
        .unwrap();
        // The OS watcher is armed when `watch` returns; the bound is only
        // there so a broken watcher fails instead of hanging (generous for
        // loaded machines).
        std::fs::write(dir.path().join("index.js"), "2").unwrap();
        rx.recv_timeout(Duration::from_secs(120))
            .expect("a change is reported");
    }
}
