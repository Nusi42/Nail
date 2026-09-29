//! ConfigManager, M1b WP24: every error per file, write-back, merge with
//! backup and revert, "open file at line".

use std::sync::mpsc;
use std::time::{Duration, Instant};

use nail_api::customize::{MergeChoice, MergeTake};
use nail_api::{ConfigFile, ErrorCode, Event, LayoutNodeRef, LayoutPatch};
use nail_config::{
    BACKUP_DIR, ConfigKind, ConfigManager, ConfigUpdate, default_layout, default_theme,
};

const TWO_ERRORS: &str =
    "theme \"x\" extends=\"default\" {\n    bogus\n    tokens { colour \"a\" \"#fff\"; }\n}\n";
const ACCENT: &str = "theme \"Mine\" extends=\"default\" {\n    tokens {\n        color \"accent\" \"#ff0000\"\n    }\n}\n";

fn write(dir: &std::path::Path, name: &str, text: &str) {
    std::fs::write(dir.join(name), text).unwrap();
}

#[test]
fn all_errors_of_a_file_are_kept_and_reported() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "theme.kdl", TWO_ERRORS);
    let manager = ConfigManager::load(dir.path());
    assert_eq!(manager.file_errors(ConfigKind::Theme).len(), 2);
    assert!(manager.file_errors(ConfigKind::Layout).is_empty());
    // The M1a view: the first error per file.
    assert_eq!(manager.errors().len(), 1);
    assert_eq!(manager.errors()[0].line, 2);

    let events = manager.error_events();
    assert_eq!(events.len(), 2);
    let Event::ConfigErrors { file, errors } = &events[0] else {
        panic!("{events:?}")
    };
    assert_eq!((file.as_str(), errors.len()), ("theme.kdl", 2));
    assert_eq!(
        events[1],
        Event::ConfigErrors {
            file: "layout.kdl".into(),
            errors: vec![]
        }
    );
}

#[test]
fn reload_updates_report_first_error_then_all_then_the_fix() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "theme.kdl", ACCENT);
    let manager = ConfigManager::load(dir.path());

    write(dir.path(), "theme.kdl", TWO_ERRORS);
    let updates = manager.reload_updates(ConfigKind::Theme);
    let [
        ConfigUpdate::Error(first),
        ConfigUpdate::Errors { kind, errors },
    ] = updates.as_slice()
    else {
        panic!("{updates:?}")
    };
    assert_eq!(*kind, ConfigKind::Theme);
    assert_eq!(errors.len(), 2);
    assert_eq!(first, &errors[0]);
    assert!(matches!(
        updates[1].to_event(),
        Event::ConfigErrors { ref file, ref errors } if file == "theme.kdl" && errors.len() == 2
    ));
    assert_eq!(
        manager.theme().tokens.get("color", "accent"),
        Some("#ff0000"),
        "keep last valid"
    );

    write(dir.path(), "theme.kdl", ACCENT);
    let updates = manager.reload_updates(ConfigKind::Theme);
    assert!(
        matches!(
            updates.as_slice(),
            [ConfigUpdate::Changed { .. }, ConfigUpdate::Errors { kind: ConfigKind::Theme, errors }] if errors.is_empty()
        ),
        "{updates:?}"
    );
    // A valid file that was valid before gets no empty list.
    write(dir.path(), "theme.kdl", &ACCENT.replace("ff0000", "00ff00"));
    let updates = manager.reload_updates(ConfigKind::Theme);
    assert!(
        matches!(updates.as_slice(), [ConfigUpdate::Changed { .. }]),
        "{updates:?}"
    );
    // reload_all: errors first, then Changed, then the cleared lists.
    write(dir.path(), "theme.kdl", TWO_ERRORS);
    write(dir.path(), "layout.kdl", "layout \"x\" {");
    let updates = manager.reload_all();
    assert_eq!(updates.len(), 4, "{updates:?}");
    write(dir.path(), "theme.kdl", ACCENT);
    std::fs::remove_file(dir.path().join("layout.kdl")).unwrap();
    let updates = manager.reload_all();
    assert!(
        matches!(
            updates.as_slice(),
            [
                ConfigUpdate::Changed { .. },
                ConfigUpdate::Errors {
                    kind: ConfigKind::Theme,
                    ..
                },
                ConfigUpdate::Errors {
                    kind: ConfigKind::Layout,
                    ..
                }
            ]
        ),
        "{updates:?}"
    );
}

#[test]
fn the_watcher_sends_all_errors_after_the_first() {
    let dir = tempfile::tempdir().unwrap();
    let manager = ConfigManager::load(dir.path());
    let (tx, rx) = mpsc::channel();
    let _watcher = manager
        .watch(Duration::from_millis(50), move |update| {
            let _ = tx.send(update);
        })
        .unwrap();
    write(dir.path(), "theme.kdl", TWO_ERRORS);
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut seen = Vec::new();
    while seen.len() < 2 {
        let left = deadline.saturating_duration_since(Instant::now());
        seen.push(rx.recv_timeout(left).expect("updates"));
    }
    assert!(matches!(seen[0], ConfigUpdate::Error(_)), "{seen:?}");
    assert!(
        matches!(&seen[1], ConfigUpdate::Errors { errors, .. } if errors.len() == 2),
        "{seen:?}"
    );
    write(dir.path(), "theme.kdl", ACCENT);
    let mut after = Vec::new();
    while after.len() < 2 {
        let left = deadline.saturating_duration_since(Instant::now());
        after.push(rx.recv_timeout(left).expect("updates"));
    }
    assert!(
        matches!(after[0], ConfigUpdate::Changed { .. }),
        "{after:?}"
    );
    assert!(
        matches!(&after[1], ConfigUpdate::Errors { errors, .. } if errors.is_empty()),
        "{after:?}"
    );
}

#[test]
fn update_layout_writes_the_file_and_reloads() {
    let dir = tempfile::tempdir().unwrap();
    let manager = ConfigManager::load(dir.path());
    let patch = LayoutPatch::AddKeyBinding {
        context: "list".into(),
        key: "e".into(),
        action: "mail.archive".into(),
        args: vec![],
    };
    let updates = manager.update_layout(std::slice::from_ref(&patch)).unwrap();
    assert!(matches!(updates.as_slice(), [ConfigUpdate::Changed { .. }]));
    let text = std::fs::read_to_string(dir.path().join("layout.kdl")).unwrap();
    assert!(text.contains("bind \"e\" \"mail.archive\""), "{text}");
    assert!(
        manager
            .layout()
            .keys
            .iter()
            .any(|k| k.context == "list" && k.key == "e")
    );
    // The same edit again changes nothing.
    assert!(manager.update_layout(&[patch]).unwrap().is_empty());
    // A patch that does not fit.
    let err = manager
        .update_layout(&[LayoutPatch::SplitSizes {
            node: LayoutNodeRef {
                window: "main".into(),
                breakpoint: 9,
                path: vec![],
            },
            sizes: vec![],
        }])
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidRequest);
    // An invalid file on disk is not touched.
    write(dir.path(), "layout.kdl", "layout {");
    let err = manager
        .update_layout(&[LayoutPatch::RemoveKeyBinding {
            context: "list".into(),
            key: "e".into(),
        }])
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::Config);
    assert_eq!(
        std::fs::read_to_string(dir.path().join("layout.kdl")).unwrap(),
        "layout {"
    );
}

#[test]
fn merge_apply_backup_and_revert() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "theme.kdl", ACCENT);
    let manager = ConfigManager::load(dir.path());
    let incoming = "theme \"Theirs\" extends=\"default\" {\n    tokens {\n        color \"accent\" \"#0000ff\"\n    }\n}\n";

    let preview = manager.preview_merge(ConfigFile::Theme, incoming).unwrap();
    assert!(!preview.sections.is_empty());
    let resolved = manager
        .resolve_merge(ConfigFile::Theme, incoming, &[], MergeTake::Theirs)
        .unwrap();
    assert_eq!(
        resolved.theme.unwrap().tokens.get("color", "accent"),
        Some("#0000ff")
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("theme.kdl")).unwrap(),
        ACCENT,
        "resolving writes nothing"
    );

    let (backup, updates) = manager
        .apply_merge(
            ConfigFile::Theme,
            incoming,
            &[MergeChoice {
                item_id: "tokens:color.accent".into(),
                take: MergeTake::Theirs,
            }],
            MergeTake::Mine,
        )
        .unwrap();
    assert!(matches!(updates.as_slice(), [ConfigUpdate::Changed { .. }]));
    assert_eq!(
        manager.theme().tokens.get("color", "accent"),
        Some("#0000ff")
    );
    assert_eq!(manager.theme().name, "Mine");
    assert_eq!(backup.file, ConfigFile::Theme);
    assert!(backup.created_at.ends_with('Z'));
    assert!(dir.path().join(BACKUP_DIR).is_dir());
    assert_eq!(
        manager.list_backups(ConfigFile::Theme).unwrap(),
        std::slice::from_ref(&backup)
    );

    let updates = manager.revert(ConfigFile::Theme, Some(&backup.id)).unwrap();
    assert!(matches!(updates.as_slice(), [ConfigUpdate::Changed { .. }]));
    assert_eq!(
        std::fs::read_to_string(dir.path().join("theme.kdl")).unwrap(),
        ACCENT
    );
    assert_eq!(
        manager.theme().tokens.get("color", "accent"),
        Some("#ff0000")
    );
    // The merged version was backed up by the revert.
    assert_eq!(manager.list_backups(ConfigFile::Theme).unwrap().len(), 2);

    let err = manager.revert(ConfigFile::Layout, None).unwrap_err();
    assert_eq!(err.code, ErrorCode::NotFound);
    let err = manager.preview_merge(ConfigFile::Accounts, "").unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidRequest);
}

#[test]
fn reverting_to_no_file_goes_back_to_the_defaults() {
    let dir = tempfile::tempdir().unwrap();
    let manager = ConfigManager::load(dir.path());
    let (backup, _) = manager
        .apply_merge(
            ConfigFile::Layout,
            "layout \"Theirs\" extends=\"default\" {\n    keys {\n        bind \"z\" \"mail.archive\"\n    }\n}\n",
            &[],
            MergeTake::Theirs,
        )
        .unwrap();
    assert_eq!(manager.layout().name, "Theirs");
    manager
        .revert(ConfigFile::Layout, Some(&backup.id))
        .unwrap();
    assert!(!dir.path().join("layout.kdl").exists());
    assert_eq!(*manager.layout(), default_layout());
    assert_eq!(*manager.theme(), default_theme());
}

#[test]
fn opening_a_file_creates_a_sparse_one_first() {
    let dir = tempfile::tempdir().unwrap();
    let manager = ConfigManager::load(dir.path());
    let command = manager.open_file_command(ConfigFile::Layout, 3, 0).unwrap();
    assert!(dir.path().join("layout.kdl").is_file());
    let path = dir.path().join("layout.kdl").display().to_string();
    assert!(
        command
            .get_args()
            .any(|a| a.to_string_lossy().contains(&path)),
        "{command:?}"
    );
    assert_eq!(
        manager.file_path(ConfigFile::Rules),
        dir.path().join("rules.kdl")
    );
}
