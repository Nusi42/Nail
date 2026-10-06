//! WP125 (ADRs 0094, 0096, 0099): imports, exports, backups, restores and
//! the diagnostics bundle through the core — jobs with progress and
//! reports, Local Folders, an account's queue, PIM data, and a backup →
//! new data directory → restore round trip.

#![cfg(test)]

use std::path::Path;
use std::time::Duration;

use nail_api::backup::{BackupContents, BackupItem, RestoreEffect};
use nail_api::diagnostics::{DiagnosticsItem, DiagnosticsOptions};
use nail_api::portability::{
    ExportFormat, ExportOptions, ExportSource, ImportFormat, ImportOptions, ImportSource, JobKind,
    JobReport, JobState, JobStatus, LOCAL_FOLDERS_ACCOUNT_ID, MailImportTarget,
    PortabilityIssueCode,
};
use nail_api::{AccountId, ErrorCode, Event, Folder, FolderId, FolderRole, Request, Response};
use nail_core::{Service, ServiceOptions};
use pretty_assertions::assert_eq;
use tokio::sync::broadcast;

const PASSPHRASE: &str = "correct horse battery staple";

fn open_at(data: &Path, config: &Path) -> Service {
    let mut options = ServiceOptions::new(data, config);
    options.watch_config = false;
    options.start_engines = false;
    Service::open(nail_core::demo::demo_options(options)).unwrap()
}

fn open() -> (Service, tempfile::TempDir, tempfile::TempDir) {
    let data = tempfile::tempdir().unwrap();
    let config = tempfile::tempdir().unwrap();
    (open_at(data.path(), config.path()), data, config)
}

async fn ok(service: &Service, request: Request) -> Response {
    let response = service.handle(request).await;
    if let Response::Error { message, .. } = &response {
        panic!("request failed: {message}");
    }
    response
}

fn error_of(response: Response) -> (ErrorCode, Option<String>) {
    match response {
        Response::Error {
            code, localized, ..
        } => (code, localized.map(|l| l.id)),
        other => panic!("error expected, got {other:?}"),
    }
}

/// Starts a job and waits for its report.
async fn run_job(service: &Service, request: Request) -> JobReport {
    let mut events = service.subscribe();
    let Response::PortabilityJob { job } = ok(service, request).await else {
        panic!("job expected");
    };
    assert_eq!(job.state, JobState::Running);
    wait_report(&mut events, &job).await
}

async fn wait_report(events: &mut broadcast::Receiver<Event>, job: &JobStatus) -> JobReport {
    let deadline = tokio::time::Instant::now() + Duration::from_mins(2);
    loop {
        let event = tokio::time::timeout_at(deadline, events.recv())
            .await
            .expect("the job finishes")
            .expect("events");
        if let Event::PortabilityJobFinished { report } = event
            && report.job_id == job.job_id
        {
            return *report;
        }
    }
}

const MBOX: &str = "From alice@example.org Mon Sep 28 10:00:00 2026
From: Alice <alice@example.org>
To: Ada <ada@example.net>
Subject: First
Message-ID: <first@example.org>
Date: Mon, 28 Sep 2026 10:00:00 +0000
Status: RO
X-Status: F

Hello.
>From the archive.

From bob@example.org Tue Sep 29 11:00:00 2026
From: Bob <bob@example.org>
To: Ada <ada@example.net>
Subject: Second
Message-ID: <second@example.org>
Date: Tue, 29 Sep 2026 11:00:00 +0000
Status: O

Unread.
";

fn write_mbox(dir: &Path) -> String {
    let path = dir.join("old.mbox");
    std::fs::write(&path, MBOX).unwrap();
    path.display().to_string()
}

fn write_maildir(dir: &Path) -> String {
    let root = dir.join("Maildir");
    for sub in [
        "cur",
        "new",
        "tmp",
        ".Lists/cur",
        ".Lists/new",
        ".Lists/tmp",
    ] {
        std::fs::create_dir_all(root.join(sub)).unwrap();
    }
    let message = |id: &str, subject: &str| {
        format!(
            "From: Carol <carol@example.org>\r\nSubject: {subject}\r\nMessage-ID: <{id}@example.org>\r\nDate: Wed, 30 Sep 2026 09:00:00 +0000\r\n\r\nBody of {subject}.\r\n"
        )
    };
    std::fs::write(
        root.join("cur/1790000000.1.host!2,FS"),
        message("m1", "Flagged and seen"),
    )
    .unwrap();
    std::fs::write(root.join("new/1790000001.2.host"), message("m2", "New one")).unwrap();
    std::fs::write(
        root.join(".Lists/cur/1790000002.3.host!2,RS"),
        message("m3", "Answered list mail"),
    )
    .unwrap();
    root.display().to_string()
}

async fn folders(service: &Service, account: &str) -> Vec<Folder> {
    let Response::Folders { folders } = ok(
        service,
        Request::ListFolders {
            account_id: AccountId::new(account),
        },
    )
    .await
    else {
        panic!("folders expected");
    };
    folders
}

async fn folder(service: &Service, account: &str, path: &str) -> Folder {
    folders(service, account)
        .await
        .into_iter()
        .find(|f| f.path == path)
        .unwrap_or_else(|| panic!("folder {path} expected"))
}

async fn messages(service: &Service, folder: &FolderId) -> Vec<nail_api::MessageSummary> {
    let Response::Messages { messages, .. } = ok(
        service,
        Request::ListMessages {
            folder_id: folder.clone(),
            offset: 0,
            limit: 100,
        },
    )
    .await
    else {
        panic!("messages expected");
    };
    messages
}

fn local_target(name: &str) -> ImportOptions {
    ImportOptions {
        mail: MailImportTarget {
            account_id: None,
            parent_folder_id: None,
            folder_name: Some(name.to_owned()),
        },
        ..ImportOptions::default()
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn an_mbox_and_a_maildir_import_into_local_folders_with_flags() {
    let (service, _data, _config) = open();
    let files = tempfile::tempdir().unwrap();
    let mbox = ImportSource {
        format: ImportFormat::Mbox,
        path: write_mbox(files.path()),
    };
    let Response::ImportPreview { preview } = ok(
        &service,
        Request::PreviewImport {
            source: mbox.clone(),
        },
    )
    .await
    else {
        panic!("preview expected");
    };
    assert_eq!(preview.counts.messages, 2);
    let Response::Accounts { accounts } = ok(&service, Request::ListAccounts).await else {
        panic!("accounts expected");
    };
    assert!(
        !accounts
            .iter()
            .any(|a| a.id.as_str() == LOCAL_FOLDERS_ACCOUNT_ID),
        "a preview writes nothing"
    );

    let mut events = service.subscribe();
    let Response::PortabilityJob { job } = ok(
        &service,
        Request::StartImport {
            source: mbox.clone(),
            options: local_target("Old mail"),
        },
    )
    .await
    else {
        panic!("job expected");
    };
    assert_eq!(job.kind, JobKind::Import);
    let report = wait_report(&mut events, &job).await;
    assert_eq!(report.state, JobState::Completed, "{:?}", report.issues);
    assert_eq!(report.counts.messages, 2);
    assert!(
        !report.created_folders.is_empty(),
        "{:?}",
        report.created_folders
    );
    let Response::Accounts { accounts } = ok(&service, Request::ListAccounts).await else {
        panic!("accounts expected");
    };
    assert!(
        accounts
            .iter()
            .any(|a| a.id.as_str() == LOCAL_FOLDERS_ACCOUNT_ID)
    );
    let imported = folder(&service, LOCAL_FOLDERS_ACCOUNT_ID, "Old mail/old").await;
    let list = messages(&service, &imported.id).await;
    assert_eq!(list.len(), 2);
    let first = list.iter().find(|m| m.subject == "First").unwrap();
    assert!(first.flags.seen && first.flags.flagged);
    let second = list.iter().find(|m| m.subject == "Second").unwrap();
    assert!(!second.flags.seen);
    // The finished job is listed with its report.
    let Response::PortabilityJobs { jobs } = ok(&service, Request::ListPortabilityJobs).await
    else {
        panic!("jobs expected");
    };
    assert_eq!(jobs[0].job_id, job.job_id);
    assert_eq!(jobs[0].report.as_ref().unwrap().counts.messages, 2);

    // Again: duplicates are skipped.
    let again = run_job(
        &service,
        Request::StartImport {
            source: mbox,
            options: local_target("Old mail"),
        },
    )
    .await;
    assert_eq!(again.counts.messages, 0);
    assert_eq!(again.counts.duplicates, 2);
    assert_eq!(messages(&service, &imported.id).await.len(), 2);

    // A Maildir with a subfolder and flags.
    let maildir = run_job(
        &service,
        Request::StartImport {
            source: ImportSource {
                format: ImportFormat::Maildir,
                path: write_maildir(files.path()),
            },
            options: local_target("Server copy"),
        },
    )
    .await;
    assert_eq!(maildir.state, JobState::Completed, "{:?}", maildir.issues);
    assert_eq!(maildir.counts.messages, 3);
    let all = folders(&service, LOCAL_FOLDERS_ACCOUNT_ID).await;
    let lists = all
        .iter()
        .find(|f| f.path.starts_with("Server copy/") && f.name == "Lists")
        .expect("the Lists subfolder");
    let list_mail = messages(&service, &lists.id).await;
    assert_eq!(list_mail.len(), 1);
    assert!(list_mail[0].flags.answered && list_mail[0].flags.seen);
}

#[tokio::test(flavor = "multi_thread")]
async fn requests_with_bad_paths_or_targets_are_refused() {
    let (service, _data, _config) = open();
    let relative = ImportSource {
        format: ImportFormat::Mbox,
        path: "relative.mbox".into(),
    };
    assert_eq!(
        error_of(
            service
                .handle(Request::PreviewImport { source: relative })
                .await
        ),
        (
            ErrorCode::InvalidRequest,
            Some("portability-error-path-not-absolute".into())
        )
    );
    let missing = tempfile::tempdir().unwrap().path().join("nope.mbox");
    assert_eq!(
        error_of(
            service
                .handle(Request::StartImport {
                    source: ImportSource {
                        format: ImportFormat::Mbox,
                        path: missing.display().to_string(),
                    },
                    options: ImportOptions::default(),
                })
                .await
        ),
        (
            ErrorCode::NotFound,
            Some("portability-error-missing-path".into())
        )
    );
    // A calendar cannot be exported as mbox.
    let out = tempfile::tempdir().unwrap();
    assert_eq!(
        error_of(
            service
                .handle(Request::StartExport {
                    source: ExportSource::Calendars {
                        calendar_ids: vec!["cal".into()],
                    },
                    format: ExportFormat::Mbox,
                    path: out.path().join("x.mbox").display().to_string(),
                    options: ExportOptions::default(),
                })
                .await
        ),
        (
            ErrorCode::InvalidRequest,
            Some("portability-error-format-mismatch".into())
        )
    );
    // Short passphrases and unknown jobs.
    assert_eq!(
        error_of(
            service
                .handle(Request::CreateBackup {
                    path: out.path().join("b.nailbackup").display().to_string(),
                    contents: BackupContents::default(),
                    passphrase: "short".into(),
                    overwrite: false,
                })
                .await
        ),
        (
            ErrorCode::InvalidRequest,
            Some("backup-issue-passphrase-too-short".into())
        )
    );
    assert_eq!(
        error_of(
            service
                .handle(Request::GetPortabilityJob {
                    job_id: "job-42".into()
                })
                .await
        ),
        (ErrorCode::NotFound, Some("portability-job-unknown".into()))
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_folder_exported_to_mbox_imports_again_identically() {
    let (service, _data, _config) = open();
    let files = tempfile::tempdir().unwrap();
    run_job(
        &service,
        Request::StartImport {
            source: ImportSource {
                format: ImportFormat::Mbox,
                path: write_mbox(files.path()),
            },
            options: local_target("Source"),
        },
    )
    .await;
    let source = folder(&service, LOCAL_FOLDERS_ACCOUNT_ID, "Source/old").await;
    let target = files.path().join("export.mbox");
    let export = run_job(
        &service,
        Request::StartExport {
            source: ExportSource::Folders {
                folder_ids: vec![source.id.clone()],
                include_subfolders: false,
            },
            format: ExportFormat::Mbox,
            path: target.display().to_string(),
            options: ExportOptions::default(),
        },
    )
    .await;
    assert_eq!(export.state, JobState::Completed, "{:?}", export.issues);
    assert_eq!(export.counts.messages, 2);
    // The target exists now: a second export is a conflict.
    assert_eq!(
        error_of(
            service
                .handle(Request::StartExport {
                    source: ExportSource::Folders {
                        folder_ids: vec![source.id.clone()],
                        include_subfolders: false,
                    },
                    format: ExportFormat::Mbox,
                    path: target.display().to_string(),
                    options: ExportOptions::default(),
                })
                .await
        )
        .0,
        ErrorCode::Conflict
    );
    run_job(
        &service,
        Request::StartImport {
            source: ImportSource {
                format: ImportFormat::Mbox,
                path: target.display().to_string(),
            },
            options: local_target("Again"),
        },
    )
    .await;
    let again = folder(&service, LOCAL_FOLDERS_ACCOUNT_ID, "Again/export").await;
    let raw_of = |list: Vec<nail_api::MessageSummary>| {
        let mut raws: Vec<Vec<u8>> = list
            .iter()
            .map(|m| {
                let stored = service.store().get_message(&m.id).unwrap();
                service.blobs().get(&stored.blob_hash.unwrap()).unwrap()
            })
            .collect();
        raws.sort();
        raws
    };
    let before = raw_of(messages(&service, &source.id).await);
    let after = raw_of(messages(&service, &again.id).await);
    assert_eq!(before, after, "the messages are byte-identical");
    let flags = |list: &[nail_api::MessageSummary]| {
        let mut f: Vec<(String, bool, bool)> = list
            .iter()
            .map(|m| (m.subject.clone(), m.flags.seen, m.flags.flagged))
            .collect();
        f.sort();
        f
    };
    assert_eq!(
        flags(&messages(&service, &source.id).await),
        flags(&messages(&service, &again.id).await)
    );
}

const ACCOUNTS: &str = "account \"work\" name=\"Work\" email=\"ada@work.example\" {
    imap host=\"127.0.0.1\" port=1 tls=\"plaintext\" {
        auth \"none\"
    }
    smtp host=\"127.0.0.1\" port=1 tls=\"plaintext\" {
        auth \"none\"
    }
}
";

#[tokio::test(flavor = "multi_thread")]
async fn importing_into_an_account_files_into_existing_folders_through_the_queue() {
    let data = tempfile::tempdir().unwrap();
    let config = tempfile::tempdir().unwrap();
    std::fs::write(config.path().join("accounts.kdl"), ACCOUNTS).unwrap();
    let service = open_at(data.path(), config.path());
    let account = AccountId::new("work");
    let inbox = Folder {
        id: FolderId::new("f-work-inbox"),
        account_id: account.clone(),
        parent_id: None,
        name: "INBOX".into(),
        path: "INBOX".into(),
        role: FolderRole::Inbox,
        unread: 0,
        total: 0,
    };
    service.store().upsert_folder(&inbox).unwrap();
    let files = tempfile::tempdir().unwrap();
    let source = ImportSource {
        format: ImportFormat::Mbox,
        path: write_mbox(files.path()),
    };
    // New folders only in Local Folders.
    assert_eq!(
        error_of(
            service
                .handle(Request::StartImport {
                    source: source.clone(),
                    options: ImportOptions {
                        mail: MailImportTarget {
                            account_id: Some(account.clone()),
                            parent_folder_id: Some(inbox.id.clone()),
                            folder_name: Some("Imported".into()),
                        },
                        ..ImportOptions::default()
                    },
                })
                .await
        ),
        (
            ErrorCode::InvalidRequest,
            Some("portability-error-account-folders".into())
        )
    );
    let mut events = service.subscribe();
    let report = run_job(
        &service,
        Request::StartImport {
            source,
            options: ImportOptions {
                mail: MailImportTarget {
                    account_id: Some(account.clone()),
                    parent_folder_id: Some(inbox.id.clone()),
                    folder_name: None,
                },
                ..ImportOptions::default()
            },
        },
    )
    .await;
    assert_eq!(report.state, JobState::CompletedWithIssues);
    assert_eq!(report.counts.messages, 2);
    assert!(
        report
            .issues
            .iter()
            .any(|i| i.code == PortabilityIssueCode::FolderMerged)
    );
    assert_eq!(messages(&service, &inbox.id).await.len(), 2);
    // Every message is an AppendMessage in the account's queue.
    let ops = service.store().list_ops(None).unwrap();
    assert_eq!(ops.len(), 2);
    assert!(ops.iter().all(|op| op.operation.kind() == "append_message"));
    let mut queued = 0;
    while let Ok(event) = events.try_recv() {
        if matches!(event, Event::OpStatus { .. }) {
            queued += 1;
        }
    }
    assert_eq!(queued, 2, "the queued operations were announced");
}

fn thunderbird_profile() -> tempfile::TempDir {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../nail-portability/tests/fixtures/thunderbird/Profiles/x7k2m9qa.default-release");
    let dir = tempfile::tempdir().unwrap();
    copy_dir(&fixture, dir.path());
    dir
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_thunderbird_profile_proposes_accounts_and_imports_mail_and_contacts() {
    let (service, _data, _config) = open();
    let profile = thunderbird_profile();
    let source = ImportSource {
        format: ImportFormat::ThunderbirdProfile,
        path: profile.path().display().to_string(),
    };
    let Response::ImportPreview { preview } = ok(
        &service,
        Request::PreviewImport {
            source: source.clone(),
        },
    )
    .await
    else {
        panic!("preview expected");
    };
    assert_eq!(preview.accounts.len(), 2, "Work IMAP and the POP3 account");
    assert_eq!(preview.rules.len(), 4);
    assert!(preview.rules.iter().any(|r| !r.complete));
    let report = run_job(
        &service,
        Request::StartImport {
            source,
            options: local_target("Thunderbird"),
        },
    )
    .await;
    assert_eq!(report.state, JobState::CompletedWithIssues);
    assert_eq!(report.counts.messages, 4);
    assert_eq!(report.counts.contacts, 4);
    assert_eq!(report.counts.groups, 2);
    assert_eq!(report.accounts.len(), 2);
    assert!(
        report
            .issues
            .iter()
            .any(|i| i.code == PortabilityIssueCode::DeletedInSource)
    );
    // The contacts are in the default address book.
    let Response::Contacts { contacts, .. } = ok(
        &service,
        Request::ListContacts {
            address_book_ids: None,
            group_id: None,
            offset: 0,
            limit: 100,
        },
    )
    .await
    else {
        panic!("contacts expected");
    };
    assert!(contacts.len() >= 4, "{contacts:?}");
}

async fn write_local_pim(service: &Service) {
    let ics = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//t//t//EN\r\nBEGIN:VEVENT\r\nUID:ev-1@example.org\r\nDTSTAMP:20260901T000000Z\r\nDTSTART:20261001T090000Z\r\nDTEND:20261001T100000Z\r\nSUMMARY:Planning\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
    let vcf = "BEGIN:VCARD\r\nVERSION:4.0\r\nUID:urn:uuid:c-1\r\nFN:Grace Hopper\r\nEMAIL:grace@example.org\r\nEND:VCARD\r\n";
    for (text, name) in [(ics, "e.ics"), (vcf, "c.vcf")] {
        let response = ok(
            service,
            Request::ImportPim {
                source: nail_api::pim::PimImportSource::Data {
                    content: text.into(),
                    file_name: Some(name.into()),
                },
                calendar_id: None,
                address_book_id: None,
            },
        )
        .await;
        assert!(
            matches!(response, Response::PimImported { .. }),
            "{response:?}"
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn backup_new_data_directory_restore_round_trip() {
    let (service, _data, config) = open();
    let files = tempfile::tempdir().unwrap();
    run_job(
        &service,
        Request::StartImport {
            source: ImportSource {
                format: ImportFormat::Mbox,
                path: write_mbox(files.path()),
            },
            options: local_target("Archive"),
        },
    )
    .await;
    write_local_pim(&service).await;
    let theme = "theme \"Mine\" extends=\"default\" {\n    tokens {\n        color \"accent\" \"#ff0000\"\n    }\n}\n";
    std::fs::write(config.path().join("theme.kdl"), theme).unwrap();
    ok(
        &service,
        Request::RecordActionUsed {
            action_id: "mail.reply".into(),
        },
    )
    .await;

    let archive = files.path().join("nail.nailbackup");
    let backup = run_job(
        &service,
        Request::CreateBackup {
            path: archive.display().to_string(),
            contents: BackupContents::default(),
            passphrase: PASSPHRASE.into(),
            overwrite: false,
        },
    )
    .await;
    assert_eq!(backup.state, JobState::Completed, "{:?}", backup.issues);
    let manifest = backup.backup.clone().expect("the manifest");
    assert!(
        manifest
            .entries
            .iter()
            .any(|e| e.name == "config/theme.kdl")
    );
    assert!(
        manifest
            .entries
            .iter()
            .any(|e| e.name == "config/settings.json")
    );

    // Inspect and verify.
    let Response::BackupManifest {
        manifest: inspected,
    } = ok(
        &service,
        Request::InspectBackup {
            path: archive.display().to_string(),
            passphrase: PASSPHRASE.into(),
        },
    )
    .await
    else {
        panic!("manifest expected");
    };
    assert_eq!(inspected.entries, manifest.entries);
    assert_eq!(
        error_of(
            service
                .handle(Request::InspectBackup {
                    path: archive.display().to_string(),
                    passphrase: "a wrong passphrase!".into(),
                })
                .await
        ),
        (
            ErrorCode::AuthFailed,
            Some("backup-wrong-passphrase".into())
        )
    );
    let verify = run_job(
        &service,
        Request::VerifyBackup {
            path: archive.display().to_string(),
            passphrase: PASSPHRASE.into(),
        },
    )
    .await;
    assert_eq!(verify.state, JobState::Completed, "{:?}", verify.issues);

    // A wiped installation: new data and config directories.
    let (fresh, _fresh_data, fresh_config) = open();
    let Response::RestorePlan { plan } = ok(
        &fresh,
        Request::PreviewRestore {
            path: archive.display().to_string(),
            passphrase: PASSPHRASE.into(),
            contents: None,
        },
    )
    .await
    else {
        panic!("plan expected");
    };
    let effect = |item: BackupItem| {
        plan.actions
            .iter()
            .find(|a| a.item == item)
            .map(|a| (a.effect, a.added))
            .unwrap()
    };
    assert_eq!(effect(BackupItem::Theme), (RestoreEffect::Add, 1));
    assert_eq!(effect(BackupItem::LocalMail), (RestoreEffect::Add, 2));
    assert!(
        !fresh_config.path().join("theme.kdl").exists(),
        "a dry run writes nothing"
    );
    assert!(
        fresh
            .store()
            .get_account(&AccountId::new(LOCAL_FOLDERS_ACCOUNT_ID))
            .is_err(),
        "a dry run creates no account"
    );

    let restore = run_job(
        &fresh,
        Request::StartRestore {
            path: archive.display().to_string(),
            passphrase: PASSPHRASE.into(),
            contents: None,
        },
    )
    .await;
    assert_eq!(restore.state, JobState::Completed, "{:?}", restore.issues);
    assert_eq!(
        std::fs::read_to_string(fresh_config.path().join("theme.kdl")).unwrap(),
        theme
    );
    assert_eq!(
        fresh.config().theme().tokens.get("color", "accent"),
        Some("#ff0000")
    );
    let restored = folder(&fresh, LOCAL_FOLDERS_ACCOUNT_ID, "Archive/old").await;
    let mail = messages(&fresh, &restored.id).await;
    assert_eq!(mail.len(), 2);
    assert!(mail.iter().any(|m| m.subject == "First" && m.flags.flagged));
    let Response::RecentActions { recent } = ok(&fresh, Request::GetRecentActions).await else {
        panic!("recent actions expected");
    };
    assert_eq!(recent.actions[0].action_id, "mail.reply");
    let Response::Contacts { contacts, .. } = ok(
        &fresh,
        Request::ListContacts {
            address_book_ids: None,
            group_id: None,
            offset: 0,
            limit: 100,
        },
    )
    .await
    else {
        panic!("contacts expected");
    };
    assert!(contacts.iter().any(|c| c.display_name == "Grace Hopper"));

    // A second restore adds nothing.
    let second = run_job(
        &fresh,
        Request::StartRestore {
            path: archive.display().to_string(),
            passphrase: PASSPHRASE.into(),
            contents: None,
        },
    )
    .await;
    assert_ne!(second.state, JobState::Failed, "{:?}", second.message);
    assert_eq!(messages(&fresh, &restored.id).await.len(), 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_diagnostics_bundle_holds_exactly_the_preview() {
    let (service, _data, config) = open();
    std::fs::write(config.path().join("accounts.kdl"), ACCOUNTS).unwrap();
    let options = DiagnosticsOptions {
        items: vec![
            DiagnosticsItem::SystemInfo,
            DiagnosticsItem::Config,
            DiagnosticsItem::StoreStats,
            DiagnosticsItem::OperationQueue,
        ],
        message_ids: Vec::new(),
        redact_addresses: true,
    };
    let Response::DiagnosticsPreview { manifest } = ok(
        &service,
        Request::PreviewDiagnostics {
            options: options.clone(),
        },
    )
    .await
    else {
        panic!("preview expected");
    };
    let names: Vec<&str> = manifest.entries.iter().map(|e| e.name.as_str()).collect();
    assert!(names.contains(&"system/info.json"));
    assert!(names.contains(&"config/accounts.kdl"));
    let accounts = manifest
        .entries
        .iter()
        .find(|e| e.name == "config/accounts.kdl")
        .unwrap();
    let preview = accounts.preview.as_deref().unwrap();
    assert!(!preview.contains("ada@work.example"), "{preview}");
    let out = tempfile::tempdir().unwrap();
    let path = out.path().join("diagnostics.zip");
    let bundle_request = |overwrite: bool| Request::CreateDiagnosticsBundle {
        options: options.clone(),
        path: path.display().to_string(),
        overwrite,
    };
    let Response::DiagnosticsBundle { bundle } = ok(&service, bundle_request(false)).await else {
        panic!("bundle expected");
    };
    assert_eq!(bundle.manifest.entries, manifest.entries);
    assert!(path.exists());
    assert_eq!(bundle.size_bytes, std::fs::metadata(&path).unwrap().len());
    // Post-M4c: the existing bundle is replaced only when the user
    // confirmed it in the Save dialog (`overwrite`).
    std::fs::write(&path, b"old").unwrap();
    assert_eq!(
        error_of(service.handle(bundle_request(false)).await),
        (
            ErrorCode::Conflict,
            Some("portability-error-target-exists".into())
        )
    );
    assert_eq!(std::fs::read(&path).unwrap(), b"old");
    let Response::DiagnosticsBundle { bundle } = ok(&service, bundle_request(true)).await else {
        panic!("bundle expected");
    };
    assert_eq!(bundle.size_bytes, std::fs::metadata(&path).unwrap().len());
    assert_eq!(std::fs::read_dir(out.path()).unwrap().count(), 1);
}
