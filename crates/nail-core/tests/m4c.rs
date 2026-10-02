//! Wave M4c contract (ADR 0094): what the core serves at the contract
//! stage — the palette's recent actions (stored, capped, validated), print
//! jobs (made, read back, ready signal, PDF through the desktop host), the
//! desktop file dialog, smartcard settings (validated, stored) — and that
//! everything else answers `unsupported` (`m4c-not-ready`) until WP125.

use std::sync::{Arc, Mutex};

use nail_api::backup::BackupContents;
use nail_api::desktop::{
    DefaultAppsStatus, DesktopHost, DesktopNotifier, DesktopSettings, HostUpdateState, InstallKind,
    InstallationInfo, PathPick, PathPickKind, UpdateSettings, WindowsArch,
};
use nail_api::engine::BoxFuture;
use nail_api::palette::MAX_RECENT_ACTIONS;
use nail_api::portability::{ImportFormat, ImportSource};
use nail_api::print::{AgendaLayout, PrintJob, PrintOptions, PrintTarget};
use nail_api::smartcard::{Pkcs11Module, SmartcardSettings};
use nail_api::{ApiError, ErrorCode, PrintJobId, Request, Response};
use nail_core::{Service, ServiceOptions};
use pretty_assertions::assert_eq;

#[derive(Debug, Default)]
struct FakeHost {
    ready: Mutex<Vec<PrintJobId>>,
    pdfs: Mutex<Vec<(PrintJobId, String)>>,
}

impl DesktopHost for FakeHost {
    fn attach(&self, _notify: DesktopNotifier) {}
    fn installation(&self) -> InstallationInfo {
        InstallationInfo {
            version: "1.0.0".into(),
            kind: InstallKind::Development,
            scope: None,
            architecture: WindowsArch::X64,
            install_dir: None,
            updates_disabled: None,
            forced_channel: None,
            mapi_installed: false,
            webview_version: None,
        }
    }
    fn update_state(&self) -> HostUpdateState {
        HostUpdateState::default()
    }
    fn apply_update_settings(&self, _settings: &UpdateSettings) {}
    fn check_for_updates(&self) -> Result<(), ApiError> {
        Ok(())
    }
    fn download_update(&self) -> Result<(), ApiError> {
        Ok(())
    }
    fn install_update(&self) -> Result<(), ApiError> {
        Ok(())
    }
    fn start_with_windows(&self) -> Option<bool> {
        None
    }
    fn apply_desktop_settings(&self, _settings: &DesktopSettings) -> Result<(), ApiError> {
        Ok(())
    }
    fn dev_endpoint_url(&self) -> Option<String> {
        None
    }
    fn default_apps(&self) -> DefaultAppsStatus {
        DefaultAppsStatus::unregistered()
    }
    fn open_default_apps_settings(&self) -> Result<(), ApiError> {
        Ok(())
    }
    fn pick_path<'a>(
        &'a self,
        pick: &'a PathPick,
    ) -> BoxFuture<'a, Result<Option<String>, ApiError>> {
        let answer = match pick.kind {
            PathPickKind::OpenFolder => Some(r"C:\Users\a\Maildir".to_owned()),
            _ => None,
        };
        Box::pin(async move { Ok(answer) })
    }
    fn print_to_pdf<'a>(
        &'a self,
        job: &'a PrintJob,
        path: &'a str,
    ) -> BoxFuture<'a, Result<u64, ApiError>> {
        self.pdfs
            .lock()
            .unwrap()
            .push((job.job_id.clone(), path.to_owned()));
        Box::pin(async { Ok(4096) })
    }
    fn print_view_ready(&self, job_id: &PrintJobId) {
        self.ready.lock().unwrap().push(job_id.clone());
    }
}

fn open(host: Option<Arc<FakeHost>>) -> (Service, tempfile::TempDir, tempfile::TempDir) {
    let data = tempfile::tempdir().unwrap();
    let config = tempfile::tempdir().unwrap();
    let mut options = ServiceOptions::new(data.path(), config.path());
    options.watch_config = false;
    options.start_engines = false;
    options.desktop = host.map(|h| h as Arc<dyn DesktopHost>);
    let service = Service::open(nail_core::demo::demo_options(options)).unwrap();
    (service, data, config)
}

fn error_of(response: Response) -> (ErrorCode, Option<String>) {
    match response {
        Response::Error {
            code, localized, ..
        } => (code, localized.map(|l| l.id)),
        other => panic!("error expected, got {other:?}"),
    }
}

#[tokio::test]
async fn recent_actions_are_stored_capped_and_validated() {
    let (service, data, config) = open(None);
    let Response::RecentActions { recent } = service.handle(Request::GetRecentActions).await else {
        panic!("recent actions expected");
    };
    assert!(recent.actions.is_empty());
    for id in ["mail.reply", "mail.archive", "mail.reply"] {
        service
            .handle(Request::RecordActionUsed {
                action_id: id.into(),
            })
            .await;
    }
    let Response::RecentActions { recent } = service.handle(Request::GetRecentActions).await else {
        panic!("recent actions expected");
    };
    let ids: Vec<&str> = recent
        .actions
        .iter()
        .map(|a| a.action_id.as_str())
        .collect();
    assert_eq!(ids, ["mail.reply", "mail.archive"]);
    assert_eq!(recent.actions[0].count, 2);
    assert_eq!(
        error_of(
            service
                .handle(Request::RecordActionUsed {
                    action_id: "not an id".into()
                })
                .await
        ),
        (
            ErrorCode::InvalidRequest,
            Some("palette-invalid-action".into())
        )
    );
    for n in 0..30 {
        service
            .handle(Request::RecordActionUsed {
                action_id: format!("a.{n}"),
            })
            .await;
    }
    // They survive a restart.
    drop(service);
    let mut options = ServiceOptions::new(data.path(), config.path());
    options.watch_config = false;
    options.start_engines = false;
    let service = Service::open(nail_core::demo::demo_options(options)).unwrap();
    let Response::RecentActions { recent } = service.handle(Request::GetRecentActions).await else {
        panic!("recent actions expected");
    };
    assert_eq!(recent.actions.len(), MAX_RECENT_ACTIONS);
    assert_eq!(recent.actions[0].action_id, "a.29");
    let Response::RecentActions { recent } = service.handle(Request::ClearRecentActions).await
    else {
        panic!("recent actions expected");
    };
    assert!(recent.actions.is_empty());
}

#[tokio::test]
async fn print_jobs_round_trip_and_pdfs_go_through_the_host() {
    let host = Arc::new(FakeHost::default());
    let (service, _d, _c) = open(Some(host.clone()));
    let Response::PrintJob { job } = service
        .handle(Request::CreatePrintJob {
            target: PrintTarget::Agenda {
                start: "2026-10-05".into(),
                end: "2026-10-11".into(),
                calendar_ids: vec![],
                layout: AgendaLayout::Week,
            },
            options: PrintOptions::default(),
        })
        .await
    else {
        panic!("print job expected");
    };
    assert_eq!(job.view, format!("#/print/{}", job.job_id));
    assert!(!job.ready);
    let Response::PrintJob { job: read } = service
        .handle(Request::GetPrintJob {
            job_id: job.job_id.clone(),
        })
        .await
    else {
        panic!("print job expected");
    };
    assert_eq!(read.target, job.target);
    assert_eq!(
        service
            .handle(Request::PrintViewReady {
                job_id: job.job_id.clone()
            })
            .await,
        Response::Done
    );
    assert_eq!(*host.ready.lock().unwrap(), vec![job.job_id.clone()]);
    let Response::PdfSaved { file } = service
        .handle(Request::SavePrintPdf {
            job_id: job.job_id.clone(),
            path: r"C:\Users\a\week.pdf".into(),
        })
        .await
    else {
        panic!("pdf expected");
    };
    assert_eq!(file.size_bytes, 4096);
    assert_eq!(host.pdfs.lock().unwrap()[0].1, r"C:\Users\a\week.pdf");
    // Unknown jobs and bad agendas.
    assert_eq!(
        error_of(
            service
                .handle(Request::GetPrintJob {
                    job_id: "nope".into()
                })
                .await
        ),
        (ErrorCode::NotFound, Some("print-job-expired".into()))
    );
    assert_eq!(
        error_of(
            service
                .handle(Request::CreatePrintJob {
                    target: PrintTarget::Agenda {
                        start: "2026-10-11".into(),
                        end: "2026-10-05".into(),
                        calendar_ids: vec![],
                        layout: AgendaLayout::List,
                    },
                    options: PrintOptions::default(),
                })
                .await
        ),
        (
            ErrorCode::InvalidRequest,
            Some("print-target-issue-end-before-start".into())
        )
    );
}

#[tokio::test]
async fn without_the_desktop_pdfs_and_file_dialogs_are_unsupported() {
    let (service, _d, _c) = open(None);
    let Response::PrintJob { job } = service
        .handle(Request::CreatePrintJob {
            target: PrintTarget::Message {
                message_id: "m1".into(),
            },
            options: PrintOptions::default(),
        })
        .await
    else {
        panic!("print job expected");
    };
    assert_eq!(
        error_of(
            service
                .handle(Request::SavePrintPdf {
                    job_id: job.job_id,
                    path: "C:/x.pdf".into()
                })
                .await
        ),
        (ErrorCode::Unsupported, Some("desktop-unavailable".into()))
    );
    let pick = PathPick {
        kind: PathPickKind::OpenFolder,
        title_id: "portability-pick-maildir".into(),
        filters: vec![],
        default_path: None,
    };
    assert_eq!(
        error_of(service.handle(Request::PickPath { pick }).await),
        (ErrorCode::Unsupported, Some("desktop-unavailable".into()))
    );
}

#[tokio::test]
async fn file_dialogs_go_through_the_host() {
    let (service, _d, _c) = open(Some(Arc::new(FakeHost::default())));
    let folder = PathPick {
        kind: PathPickKind::OpenFolder,
        title_id: "portability-pick-maildir".into(),
        filters: vec![],
        default_path: None,
    };
    assert_eq!(
        service.handle(Request::PickPath { pick: folder }).await,
        Response::PathPicked {
            path: Some(r"C:\Users\a\Maildir".into())
        }
    );
    let save = PathPick {
        kind: PathPickKind::SaveFile,
        title_id: "portability-pick-backup".into(),
        filters: vec![],
        default_path: Some("nail.nailbackup".into()),
    };
    assert_eq!(
        service.handle(Request::PickPath { pick: save }).await,
        Response::PathPicked { path: None }
    );
}

#[tokio::test]
async fn smartcard_settings_are_validated_and_stored() {
    let (service, _d, _c) = open(None);
    assert_eq!(
        service.handle(Request::GetSmartcardSettings).await,
        Response::SmartcardSettings {
            settings: SmartcardSettings::default()
        }
    );
    let settings = SmartcardSettings {
        modules: vec![Pkcs11Module {
            path: r"C:\Windows\System32\opensc-pkcs11.dll".into(),
            name: Some("OpenSC".into()),
            enabled: true,
        }],
        pin_cache_minutes: 5,
    };
    service
        .handle(Request::SetSmartcardSettings {
            settings: settings.clone(),
        })
        .await;
    assert_eq!(
        service.handle(Request::GetSmartcardSettings).await,
        Response::SmartcardSettings { settings }
    );
    let bad = SmartcardSettings {
        modules: vec![Pkcs11Module {
            path: "opensc.dll".into(),
            name: None,
            enabled: true,
        }],
        pin_cache_minutes: 0,
    };
    assert_eq!(
        error_of(
            service
                .handle(Request::SetSmartcardSettings { settings: bad })
                .await
        ),
        (
            ErrorCode::InvalidRequest,
            Some("smartcard-settings-issue-relative-path".into())
        )
    );
}

#[tokio::test]
async fn the_rest_waits_for_the_integration() {
    let (service, _d, _c) = open(None);
    let source = ImportSource {
        format: ImportFormat::Mbox,
        path: "C:/mail.mbox".into(),
    };
    for request in [
        Request::FindImportSources,
        Request::PreviewImport {
            source: source.clone(),
        },
        Request::StartImport {
            source,
            options: Default::default(),
        },
        Request::CreateBackup {
            path: "C:/b.nailbackup".into(),
            contents: BackupContents::default(),
            passphrase: "a long passphrase".into(),
        },
        Request::ListPortabilityJobs,
        Request::PreviewDiagnostics {
            options: Default::default(),
        },
        Request::CreateRecoveryKey { save_to: None },
        Request::ListSmartcards,
    ] {
        assert_eq!(
            error_of(service.handle(request).await),
            (ErrorCode::Unsupported, Some("m4c-not-ready".into()))
        );
    }
    // The vault status carries the recovery-key fields.
    let Response::VaultStatus { status } = service.handle(Request::GetVaultStatus).await else {
        panic!("vault status expected");
    };
    assert!(!status.needs_recovery);
    assert!(!status.recovery.configured);
}
