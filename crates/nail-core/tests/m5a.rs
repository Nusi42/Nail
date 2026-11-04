//! M5a integration (WP155): the appearance requests through
//! `Service::handle`, Follow Windows from the service's OS source,
//! `ReportSystemAppearance`, density, the resync events, and attachments of
//! received mail (`GetMessageAttachment`, `SaveAttachment`,
//! `OpenAttachment`) and "Show
//! source" (`GetMessageSource`, ADR 0194).

#![cfg(test)]

#[expect(dead_code, reason = "each test crate uses part of the shared helpers")]
mod common;
use common::next_event;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use base64::Engine as _;
use nail_api::appearance::{
    Appearance, Density, ManualSystemAppearance, SystemAppearance, ThemePreferences,
};
use nail_api::config::theme::ThemeMode;
use nail_api::desktop::{DesktopHost, DesktopNotifier};
use nail_api::{AccountId, ApiError, ErrorCode, Event, MessageId, Request, Response, SourceChunk};
use nail_core::demo::{DEMO_ACCOUNT_ID, seed_demo};
use nail_core::{Service, ServiceOptions};
use pretty_assertions::assert_eq;
use tempfile::TempDir;

struct Fixture {
    service: Service,
    os: Arc<ManualSystemAppearance>,
    config_dir: TempDir,
    _data_dir: TempDir,
}

/// A seeded service whose Follow Windows source is set by hand (light).
fn open() -> Fixture {
    let data_dir = tempfile::tempdir().unwrap();
    let config_dir = tempfile::tempdir().unwrap();
    let os = Arc::new(ManualSystemAppearance::new(Some(SystemAppearance::LIGHT)));
    let mut options = ServiceOptions::new(data_dir.path(), config_dir.path());
    options.system_appearance = Some(os.clone());
    let options = nail_core::demo::demo_options(options);
    let service = Service::open(options).unwrap();
    seed_demo(service.store(), service.blobs()).unwrap();
    Fixture {
        service,
        os,
        config_dir,
        _data_dir: data_dir,
    }
}

async fn themes(service: &Service, request: Request) -> Appearance {
    match service.handle(request).await {
        Response::Themes { appearance } => *appearance,
        other => panic!("expected themes, got {other:?}"),
    }
}

async fn resolved_mode(service: &Service) -> (ThemeMode, Density) {
    match service.handle(Request::GetConfig).await {
        Response::Config { theme, .. } => (theme.mode, theme.density),
        other => panic!("expected config, got {other:?}"),
    }
}

fn preferences(appearance: &Appearance) -> ThemePreferences {
    ThemePreferences::from(&appearance.settings)
}

#[tokio::test]
async fn first_run_follows_windows_with_frost_and_graphite() {
    let f = open();
    let appearance = themes(&f.service, Request::ListThemes).await;
    let ids: Vec<&str> = appearance.themes.iter().map(|t| t.id.as_str()).collect();
    assert_eq!(
        ids,
        ["frost", "sage", "graphite", "nocturne", "high-contrast"]
    );
    assert!(appearance.settings.follow_system);
    assert_eq!(
        (
            appearance.settings.light.as_str(),
            appearance.settings.dark.as_str()
        ),
        ("frost", "graphite")
    );
    assert_eq!(appearance.active, "frost");
    assert_eq!(appearance.system, Some(SystemAppearance::LIGHT));
    assert_eq!(
        resolved_mode(&f.service).await,
        (ThemeMode::Light, Density::Comfortable)
    );
    // Nothing was written: no theme.kdl yet.
    assert!(!f.config_dir.path().join("theme.kdl").exists());
}

#[tokio::test]
async fn an_os_switch_changes_the_theme_and_broadcasts() {
    let f = open();
    let mut events = f.service.subscribe();
    f.os.set(SystemAppearance::DARK);
    match next_event(&mut events).await {
        Event::ThemeChanged { appearance } => {
            assert_eq!(appearance.active, "graphite");
            assert_eq!(appearance.system, Some(SystemAppearance::DARK));
        }
        other => panic!("expected ThemeChanged, got {other:?}"),
    }
    match next_event(&mut events).await {
        Event::ConfigChanged { theme, .. } => assert_eq!(theme.mode, ThemeMode::Dark),
        other => panic!("expected ConfigChanged, got {other:?}"),
    }
    // A Windows contrast theme picks High contrast.
    f.os.set(SystemAppearance {
        high_contrast: true,
        ..SystemAppearance::DARK
    });
    match next_event(&mut events).await {
        Event::ThemeChanged { appearance } => assert_eq!(appearance.active, "high-contrast"),
        other => panic!("expected ThemeChanged, got {other:?}"),
    }
    assert_eq!(resolved_mode(&f.service).await.0, ThemeMode::HighContrast);
}

#[tokio::test]
async fn a_fixed_theme_ignores_the_os_and_density_applies_to_it() {
    let f = open();
    let appearance = themes(
        &f.service,
        Request::SetTheme {
            theme_id: "nocturne".into(),
        },
    )
    .await;
    assert_eq!(appearance.active, "nocturne");
    assert!(!appearance.settings.follow_system);
    let file = std::fs::read_to_string(f.config_dir.path().join("theme.kdl")).unwrap();
    assert!(file.contains("extends=\"nocturne\""), "{file}");

    f.os.set(SystemAppearance::LIGHT);
    f.os.set(SystemAppearance::DARK);
    let listed = themes(&f.service, Request::ListThemes).await;
    assert_eq!(listed.active, "nocturne");

    let mut prefs = preferences(&listed);
    prefs.density = Density::Compact;
    let appearance = themes(
        &f.service,
        Request::SetThemePreferences { preferences: prefs },
    )
    .await;
    assert_eq!(appearance.settings.density, Density::Compact);
    assert_eq!(
        resolved_mode(&f.service).await,
        (ThemeMode::Dark, Density::Compact)
    );
    let file = std::fs::read_to_string(f.config_dir.path().join("theme.kdl")).unwrap();
    assert!(file.contains("density=\"compact\""), "{file}");
}

#[tokio::test]
async fn unknown_themes_and_bad_overrides_are_refused() {
    let f = open();
    match f
        .service
        .handle(Request::SetTheme {
            theme_id: "nord".into(),
        })
        .await
    {
        Response::Error { code, .. } => assert_eq!(code, ErrorCode::InvalidRequest),
        other => panic!("expected an error, got {other:?}"),
    }
    let mut prefs = preferences(&themes(&f.service, Request::ListThemes).await);
    prefs.overrides.font_size = Some(40);
    match f
        .service
        .handle(Request::SetThemePreferences { preferences: prefs })
        .await
    {
        Response::Error {
            code, localized, ..
        } => {
            assert_eq!(code, ErrorCode::InvalidRequest);
            assert_eq!(
                localized.map(|l| l.id),
                Some("theme-overrides-issue-font-size-range".to_owned())
            );
        }
        other => panic!("expected an error, got {other:?}"),
    }
}

#[tokio::test]
async fn browser_reports_are_ignored_while_the_core_watches_the_os() {
    let f = open();
    let appearance = themes(
        &f.service,
        Request::ReportSystemAppearance {
            system: SystemAppearance::DARK,
        },
    )
    .await;
    assert_eq!(appearance.system, Some(SystemAppearance::LIGHT));
    assert_eq!(appearance.active, "frost");
}

#[tokio::test]
async fn browser_reports_apply_without_an_os_source() {
    let dir = tempfile::tempdir().unwrap();
    let store = nail_store::Store::open_in_memory().unwrap();
    let blobs = nail_store::BlobStore::open(dir.path().join("blobs")).unwrap();
    let config = nail_config::ConfigManager::load(dir.path());
    let service = Service::from_parts(store, blobs, config, None).unwrap();
    let mut events = service.subscribe();
    let appearance = themes(
        &service,
        Request::ReportSystemAppearance {
            system: SystemAppearance::DARK,
        },
    )
    .await;
    assert_eq!(appearance.active, "graphite");
    assert_eq!(appearance.system, Some(SystemAppearance::DARK));
    assert!(matches!(
        next_event(&mut events).await,
        Event::ThemeChanged { .. }
    ));
    assert!(matches!(
        next_event(&mut events).await,
        Event::ConfigChanged { .. }
    ));
}

#[tokio::test]
async fn a_lagging_client_gets_the_appearance_back() {
    let f = open();
    let events = f.service.resync_events().await;
    match &events[0] {
        Event::ThemeChanged { appearance } => assert_eq!(appearance.active, "frost"),
        other => panic!("expected ThemeChanged first, got {other:?}"),
    }
    assert!(matches!(events[1], Event::ConfigChanged { .. }));
}

/// The demo's receipt with one text attachment.
async fn receipt(service: &Service) -> MessageId {
    let folders = match service
        .handle(Request::ListFolders {
            account_id: AccountId::new(DEMO_ACCOUNT_ID),
        })
        .await
    {
        Response::Folders { folders } => folders,
        other => panic!("expected folders, got {other:?}"),
    };
    for folder in folders {
        let Response::Messages { messages, .. } = service
            .handle(Request::ListMessages {
                folder_id: folder.id,
                offset: 0,
                limit: 200,
            })
            .await
        else {
            panic!("expected messages");
        };
        if let Some(m) = messages.iter().find(|m| m.subject == "Receipt #88213") {
            return m.id.clone();
        }
    }
    panic!("the demo receipt is missing");
}

const RECEIPT_TEXT: &str = "Receipt 88213 - total 42.00 EUR - paid by card";

#[tokio::test]
async fn attachments_download_as_base64() {
    let f = open();
    let id = receipt(&f.service).await;
    match f
        .service
        .handle(Request::GetMessageAttachment {
            message_id: id.clone(),
            index: 0,
        })
        .await
    {
        Response::AttachmentData { mime, data } => {
            assert_eq!(mime, "text/plain");
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(data)
                .unwrap();
            assert_eq!(String::from_utf8(bytes).unwrap().trim(), RECEIPT_TEXT);
        }
        other => panic!("expected attachment data, got {other:?}"),
    }
    match f
        .service
        .handle(Request::GetMessageAttachment {
            message_id: id,
            index: 7,
        })
        .await
    {
        Response::Error { code, .. } => assert_eq!(code, ErrorCode::NotFound),
        other => panic!("expected not_found, got {other:?}"),
    }
}

#[tokio::test]
async fn the_source_is_the_message_as_received_and_decoded() {
    let f = open();
    let id = receipt(&f.service).await;
    let Response::MessageSource { source } = f
        .service
        .handle(Request::GetMessageSource {
            message_id: id.clone(),
        })
        .await
    else {
        panic!("expected the source");
    };
    assert!(
        source
            .original
            .contains("Message-ID: <receipt-88213@corner-books.example>")
    );
    assert!(source.original.contains("--r-b--"));
    assert_eq!(source.size, source.original.len() as u64);
    assert!(!source.original_truncated && !source.decoded_truncated && !source.encrypted);
    let decoded: String = source
        .decoded
        .iter()
        .map(|chunk| match chunk {
            SourceChunk::Text { text } => text.as_str(),
            SourceChunk::Omitted { .. } => panic!("the receipt has only text parts"),
        })
        .collect();
    assert!(decoded.starts_with(
        "From: Corner Bookshop <shop@corner-books.example>
"
    ));
    assert!(decoded.contains("Thanks for your purchase!"));
    assert!(decoded.contains(RECEIPT_TEXT));

    match f
        .service
        .handle(Request::GetMessageSource {
            message_id: MessageId::new("no-such-message"),
        })
        .await
    {
        Response::Error { code, .. } => assert_eq!(code, ErrorCode::NotFound),
        other => panic!("expected not_found, got {other:?}"),
    }
}

/// A desktop host that records the files it was asked to open (with their
/// content at that moment), the links, and the threads it was called on;
/// it can refuse.
#[derive(Debug, Default)]
struct OpeningHost {
    opened: Mutex<Vec<(PathBuf, Vec<u8>)>>,
    links: Mutex<Vec<String>>,
    threads: Mutex<Vec<std::thread::ThreadId>>,
    refuse: bool,
}

impl OpeningHost {
    /// Records the calling thread; `Err(why)` when refusing.
    fn called(&self, why: &str) -> Result<(), ApiError> {
        self.threads
            .lock()
            .unwrap()
            .push(std::thread::current().id());
        if self.refuse {
            Err(ApiError::internal(why))
        } else {
            Ok(())
        }
    }
}

impl DesktopHost for OpeningHost {
    fn attach(&self, _notify: DesktopNotifier) {}
    nail_api::inert_desktop_host!();
    fn open_file(&self, path: &Path) -> Result<(), ApiError> {
        self.called("no handler")?;
        self.opened
            .lock()
            .unwrap()
            .push((path.to_path_buf(), std::fs::read(path).unwrap()));
        Ok(())
    }
    fn open_link(&self, url: &str) -> Result<(), ApiError> {
        self.called("no browser")?;
        self.links.lock().unwrap().push(url.to_owned());
        Ok(())
    }
}

/// The host waits for the system (`open` on macOS): the core calls it off
/// the runtime's thread (the test's single-threaded runtime here), and its
/// errors still arrive.
#[tokio::test]
async fn the_desktop_host_opens_files_and_links_off_the_runtime_thread() {
    let host = Arc::new(OpeningHost::default());
    let (service, _data, _config) = open_with_host(Some(host.clone()));
    let id = receipt(&service).await;
    let open = Request::OpenAttachment {
        message_id: id,
        index: 0,
    };
    assert_eq!(service.handle(open).await, Response::Done);
    let link = Request::OpenLink {
        url: "https://example.com/a".into(),
    };
    match service.handle(link.clone()).await {
        Response::LinkOpened { outcome } => {
            assert_eq!(outcome, nail_api::links::LinkOutcome::Opened);
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(
        host.links.lock().unwrap().as_slice(),
        ["https://example.com/a"]
    );
    let runtime = std::thread::current().id();
    let threads = host.threads.lock().unwrap().clone();
    assert_eq!(threads.len(), 2);
    assert!(threads.iter().all(|t| *t != runtime), "{threads:?}");

    let refusing = Arc::new(OpeningHost {
        refuse: true,
        ..OpeningHost::default()
    });
    let (service, _data, _config) = open_with_host(Some(refusing));
    match service.handle(link).await {
        Response::Error { localized, .. } => {
            assert_eq!(localized.map(|l| l.id), Some("link-open-failed".to_owned()));
        }
        other => panic!("{other:?}"),
    }
}

fn open_with_host(host: Option<Arc<OpeningHost>>) -> (Service, TempDir, TempDir) {
    let data_dir = tempfile::tempdir().unwrap();
    let config_dir = tempfile::tempdir().unwrap();
    let mut options = ServiceOptions::new(data_dir.path(), config_dir.path());
    options.desktop = host.map(|h| -> Arc<dyn DesktopHost> { h });
    let service = Service::open(nail_core::demo::demo_options(options)).unwrap();
    seed_demo(service.store(), service.blobs()).unwrap();
    (service, data_dir, config_dir)
}

#[tokio::test]
async fn attachments_open_through_a_private_folder_and_the_desktop_host() {
    let host = Arc::new(OpeningHost::default());
    let (service, data_dir, _config) = open_with_host(Some(host.clone()));
    let id = receipt(&service).await;
    let open = || Request::OpenAttachment {
        message_id: id.clone(),
        index: 0,
    };
    assert_eq!(service.handle(open()).await, Response::Done);
    assert_eq!(service.handle(open()).await, Response::Done);
    let opened = host.opened.lock().unwrap().clone();
    assert_eq!(opened.len(), 2);
    let root = data_dir.path().join("opened-attachments");
    for (path, content) in &opened {
        assert!(path.starts_with(&root), "{}", path.display());
        assert_eq!(
            String::from_utf8(content.clone()).unwrap().trim(),
            RECEIPT_TEXT
        );
    }
    // One subfolder per open, the same file name in each.
    assert_ne!(opened[0].0.parent(), opened[1].0.parent());
    assert_eq!(opened[0].0.file_name(), opened[1].0.file_name());
    // Unknown message and index.
    match service
        .handle(Request::OpenAttachment {
            message_id: id.clone(),
            index: 99,
        })
        .await
    {
        Response::Error { code, .. } => assert_eq!(code, ErrorCode::NotFound),
        other => panic!("expected not_found, got {other:?}"),
    }
    // The files go at shutdown.
    service.shutdown().await;
    assert!(!root.exists());
}

#[tokio::test]
async fn opened_attachment_folders_start_empty_and_old_ones_are_pruned() {
    let data_dir = tempfile::tempdir().unwrap();
    let stale = data_dir.path().join("opened-attachments").join("left-over");
    std::fs::create_dir_all(&stale).unwrap();
    std::fs::write(stale.join("old.pdf"), "x").unwrap();
    let config_dir = tempfile::tempdir().unwrap();
    let mut options = ServiceOptions::new(data_dir.path(), config_dir.path());
    options.desktop = Some(Arc::new(OpeningHost::default()));
    let _service = Service::open(nail_core::demo::demo_options(options)).unwrap();
    // A previous run's files are removed when the service starts.
    assert!(!stale.exists());
}

#[tokio::test]
async fn attachments_are_not_opened_without_a_desktop_host_or_when_it_fails() {
    let (service, _data, _config) = open_with_host(None);
    let id = receipt(&service).await;
    let open = Request::OpenAttachment {
        message_id: id.clone(),
        index: 0,
    };
    match service.handle(open.clone()).await {
        Response::Error { code, .. } => assert_eq!(code, ErrorCode::Unsupported),
        other => panic!("expected unsupported, got {other:?}"),
    }
    let refusing = Arc::new(OpeningHost {
        refuse: true,
        ..OpeningHost::default()
    });
    let (service, _data, _config) = open_with_host(Some(refusing));
    let id = receipt(&service).await;
    match service
        .handle(Request::OpenAttachment {
            message_id: id,
            index: 0,
        })
        .await
    {
        Response::Error {
            code, localized, ..
        } => {
            assert_eq!(code, ErrorCode::Internal);
            assert_eq!(
                localized.map(|l| l.id),
                Some("attachment-open-failed".to_owned())
            );
        }
        other => panic!("expected an error, got {other:?}"),
    }
}

#[tokio::test]
async fn attachments_save_to_a_file_and_replace_only_when_confirmed() {
    let f = open();
    let id = receipt(&f.service).await;
    let out = tempfile::tempdir().unwrap();
    let target = out.path().join("receipt.txt");
    let save = |overwrite| Request::SaveAttachment {
        message_id: id.clone(),
        index: 0,
        path: target.display().to_string(),
        overwrite,
    };
    assert_eq!(f.service.handle(save(false)).await, Response::Done);
    let written = std::fs::read_to_string(&target).unwrap();
    assert_eq!(written.trim(), RECEIPT_TEXT);

    match f.service.handle(save(false)).await {
        Response::Error { code, .. } => assert_eq!(code, ErrorCode::Conflict),
        other => panic!("expected conflict, got {other:?}"),
    }
    std::fs::write(&target, "old").unwrap();
    assert_eq!(f.service.handle(save(true)).await, Response::Done);
    assert_eq!(
        std::fs::read_to_string(&target).unwrap().trim(),
        RECEIPT_TEXT
    );

    match f
        .service
        .handle(Request::SaveAttachment {
            message_id: id,
            index: 0,
            path: "relative.txt".into(),
            overwrite: false,
        })
        .await
    {
        Response::Error {
            code, localized, ..
        } => {
            assert_eq!(code, ErrorCode::InvalidRequest);
            assert_eq!(
                localized.map(|l| l.id),
                Some("attachment-path-not-absolute".to_owned())
            );
        }
        other => panic!("expected invalid_request, got {other:?}"),
    }
}
