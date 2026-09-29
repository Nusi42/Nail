//! ConfigManager: loading a config directory, keep-last-valid and watching.

use std::sync::mpsc;
use std::time::{Duration, Instant};

use nail_api::Event;
use nail_config::{ConfigKind, ConfigManager, ConfigUpdate, default_layout, default_theme};

const ACCENT_THEME: &str =
    r##"theme "Mine" extends="default" { tokens { color "accent" "#ff0000"; }; }"##;
const BROKEN_THEME: &str =
    "theme \"Mine\" extends=\"default\" {\n  tokens { color \"accent\" \"$color.nope\"; }\n}";
const NARROW_LAYOUT: &str =
    r#"layout "Mine" extends="default" { window "main" { message-list; }; }"#;

fn write(dir: &std::path::Path, name: &str, text: &str) {
    std::fs::write(dir.join(name), text).unwrap();
}

#[test]
fn an_empty_directory_uses_the_defaults() {
    let dir = tempfile::tempdir().unwrap();
    let manager = ConfigManager::load(dir.path());
    assert_eq!(*manager.theme(), default_theme());
    assert_eq!(*manager.layout(), default_layout());
    assert!(manager.errors().is_empty());
    assert_eq!(manager.dir(), dir.path());
    // A directory that does not exist is fine, too.
    let manager = ConfigManager::load(dir.path().join("missing"));
    assert_eq!(*manager.theme(), default_theme());
}

#[test]
fn user_files_are_loaded() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "theme.kdl", ACCENT_THEME);
    write(dir.path(), "layout.kdl", NARROW_LAYOUT);
    let manager = ConfigManager::load(dir.path());
    assert_eq!(manager.theme().name, "Mine");
    assert_eq!(
        manager.theme().tokens.get("color", "accent"),
        Some("#ff0000")
    );
    assert_eq!(manager.layout().windows["main"].len(), 1);
    assert!(manager.errors().is_empty());
}

#[test]
fn an_invalid_file_at_start_up_falls_back_to_the_default() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "theme.kdl", BROKEN_THEME);
    write(dir.path(), "layout.kdl", NARROW_LAYOUT);
    let manager = ConfigManager::load(dir.path());
    assert_eq!(*manager.theme(), default_theme());
    assert_eq!(manager.layout().name, "Mine");
    let errors = manager.errors();
    assert_eq!(errors.len(), 1);
    assert_eq!(
        (errors[0].file.as_str(), errors[0].line, errors[0].column),
        ("theme.kdl", 2, 27)
    );
}

#[test]
fn reload_keeps_the_last_valid_configuration() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "theme.kdl", ACCENT_THEME);
    let manager = ConfigManager::load(dir.path());
    assert_eq!(manager.reload(ConfigKind::Theme), None, "nothing changed");

    write(dir.path(), "theme.kdl", BROKEN_THEME);
    let Some(ConfigUpdate::Error(err)) = manager.reload(ConfigKind::Theme) else {
        panic!("expected an error");
    };
    assert!(err.message.contains("$color.nope"));
    assert_eq!(
        manager.theme().tokens.get("color", "accent"),
        Some("#ff0000"),
        "last valid theme stays"
    );
    assert_eq!(manager.errors(), std::slice::from_ref(&err));
    assert_eq!(
        ConfigUpdate::Error(err).to_event(),
        Event::ConfigError {
            file: "theme.kdl".into(),
            line: 2,
            column: 27,
            message: "unknown token `$color.nope`".into(),
        }
    );
    assert_eq!(
        manager.reload(ConfigKind::Theme),
        None,
        "the same broken text is not reported twice"
    );

    write(
        dir.path(),
        "theme.kdl",
        &ACCENT_THEME.replace("#ff0000", "#00ff00"),
    );
    let Some(update @ ConfigUpdate::Changed { .. }) = manager.reload(ConfigKind::Theme) else {
        panic!("expected a change");
    };
    assert!(manager.errors().is_empty());
    assert_eq!(
        manager.theme().tokens.get("color", "accent"),
        Some("#00ff00")
    );
    let Event::ConfigChanged { theme, layout } = update.to_event() else {
        panic!("expected ConfigChanged");
    };
    assert_eq!(theme.tokens.get("color", "accent"), Some("#00ff00"));
    assert_eq!(*layout, default_layout());

    std::fs::remove_file(dir.path().join("theme.kdl")).unwrap();
    let updates = manager.reload_all();
    assert!(
        matches!(updates.as_slice(), [ConfigUpdate::Changed { .. }]),
        "{updates:?}"
    );
    assert_eq!(*manager.theme(), default_theme());
}

/// Waits for an update matching `pred`, skipping others.
fn wait_for(
    rx: &mpsc::Receiver<ConfigUpdate>,
    pred: impl Fn(&ConfigUpdate) -> bool,
) -> ConfigUpdate {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        match rx.recv_timeout(left) {
            Ok(update) if pred(&update) => return update,
            Ok(_) => {}
            Err(e) => panic!("no matching config update: {e}"),
        }
    }
}

#[test]
fn the_watcher_reports_changes_and_errors() {
    let dir = tempfile::tempdir().unwrap();
    let config_dir = dir.path().join("config");
    let manager = ConfigManager::load(&config_dir);
    let (tx, rx) = mpsc::channel();
    let watcher = manager
        .watch(Duration::from_millis(50), move |update| {
            let _ = tx.send(update);
        })
        .unwrap();
    assert!(config_dir.is_dir(), "watch creates the directory");

    write(&config_dir, "layout.kdl", NARROW_LAYOUT);
    let update = wait_for(
        &rx,
        |u| matches!(u, ConfigUpdate::Changed { layout, .. } if layout.name == "Mine"),
    );
    let ConfigUpdate::Changed { theme, .. } = update else {
        unreachable!()
    };
    assert_eq!(*theme, default_theme());
    assert_eq!(manager.layout().name, "Mine");

    write(&config_dir, "theme.kdl", BROKEN_THEME);
    let ConfigUpdate::Error(err) = wait_for(&rx, |u| matches!(u, ConfigUpdate::Error(_))) else {
        unreachable!()
    };
    assert_eq!((err.line, err.column), (2, 27));
    assert_eq!(*manager.theme(), default_theme(), "last valid theme stays");

    write(&config_dir, "theme.kdl", ACCENT_THEME);
    wait_for(
        &rx,
        |u| matches!(u, ConfigUpdate::Changed { theme, .. } if theme.name == "Mine"),
    );
    assert!(manager.errors().is_empty());

    // Other files in the directory are ignored.
    write(&config_dir, "notes.txt", "hello");
    drop(watcher);
}
