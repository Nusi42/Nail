//! Backup → inspect → verify → plan → restore round trips over in-memory
//! sources and targets (the shapes the core implements).

use std::collections::BTreeMap;
use std::io::Write;

use chrono::{DateTime, Utc};
use nail_api::Flags;
use nail_api::backup::{BackupContents, BackupItem, RestoreEffect};
use nail_api::ids::{AccountId, ConfigBackupId, PluginId};
use nail_api::portability::{JobKind, JobReport, JobState, PortabilityIssueCode};
use nail_ical::vcard::{ParsedContact, ParsedGroup};
use nail_ical::{ParsedEvent, ParsedTask};
use pretty_assertions::assert_eq;

use super::*;
use crate::export::tests::{MemoryMail, message, sample};
use crate::sink::{
    DuplicateKey, ExportCollection, ImportedMessage, MailImportSink, PimImportSink,
    SourceCollection, SourceFolder,
};

const PASSPHRASE: &str = "correct horse battery staple";

fn now() -> DateTime<Utc> {
    DateTime::from_timestamp(1_790_848_800, 0).unwrap()
}

#[derive(Clone, Default)]
struct MemoryPim(Vec<(ExportCollection, Vec<String>)>);

impl PimExportSource for MemoryPim {
    fn collections(&mut self) -> std::result::Result<Vec<ExportCollection>, SinkError> {
        Ok(self.0.iter().map(|(c, _)| c.clone()).collect())
    }
    fn items(
        &mut self,
        collection: &ExportCollection,
        visit: &mut dyn FnMut(std::result::Result<String, SinkError>) -> Visit,
    ) -> std::result::Result<(), SinkError> {
        for item in &self
            .0
            .iter()
            .find(|(c, _)| c.key == collection.key)
            .unwrap()
            .1
        {
            visit(Ok(item.clone()));
        }
        Ok(())
    }
}

/// The core's data as a backup sees it.
#[derive(Clone, Default)]
struct Source {
    files: BTreeMap<(BackupItem, String), Vec<u8>>,
    mail: BTreeMap<AccountId, MemoryMail>,
    calendars: MemoryPim,
    books: MemoryPim,
    /// Makes `write_file` produce different bytes on every call.
    unstable: bool,
    calls: u32,
}

impl BackupSource for Source {
    fn info(&mut self) -> BackupInfo {
        BackupInfo {
            app_version: "1.0.0-rc.1".into(),
            schema_version: 15,
            accounts: vec![AccountId::new("work")],
            plugins: vec![PluginId::new("hello")],
        }
    }
    fn files(&mut self, item: BackupItem) -> std::result::Result<Vec<BackupFile>, SinkError> {
        Ok(self
            .files
            .keys()
            .filter(|(i, _)| *i == item)
            .map(|(_, name)| BackupFile { name: name.clone() })
            .collect())
    }
    fn write_file(
        &mut self,
        item: BackupItem,
        name: &str,
        out: &mut dyn Write,
    ) -> std::result::Result<(), SinkError> {
        self.calls += 1;
        out.write_all(&self.files[&(item, name.to_owned())])
            .unwrap();
        if self.unstable {
            write!(out, "{}", self.calls).unwrap();
        }
        Ok(())
    }
    fn local_mail_accounts(&mut self) -> std::result::Result<Vec<AccountId>, SinkError> {
        Ok(self.mail.keys().cloned().collect())
    }
    fn local_mail(
        &mut self,
        account: &AccountId,
    ) -> std::result::Result<Box<dyn MailExportSource + '_>, SinkError> {
        Ok(Box::new(self.mail.get_mut(account).unwrap()))
    }
    fn local_calendars(&mut self) -> std::result::Result<Box<dyn PimExportSource + '_>, SinkError> {
        Ok(Box::new(&mut self.calendars))
    }
    fn local_address_books(
        &mut self,
    ) -> std::result::Result<Box<dyn PimExportSource + '_>, SinkError> {
        Ok(Box::new(&mut self.books))
    }
}

impl MailExportSource for &mut MemoryMail {
    fn folders(&mut self) -> std::result::Result<Vec<ExportFolder>, SinkError> {
        (**self).folders()
    }
    fn messages(
        &mut self,
        folder: &ExportFolder,
        visit: &mut dyn FnMut(std::result::Result<crate::sink::ExportMessage, SinkError>) -> Visit,
    ) -> std::result::Result<(), SinkError> {
        (**self).messages(folder, visit)
    }
}

impl PimExportSource for &mut MemoryPim {
    fn collections(&mut self) -> std::result::Result<Vec<ExportCollection>, SinkError> {
        (**self).collections()
    }
    fn items(
        &mut self,
        collection: &ExportCollection,
        visit: &mut dyn FnMut(std::result::Result<String, SinkError>) -> Visit,
    ) -> std::result::Result<(), SinkError> {
        (**self).items(collection, visit)
    }
}

const EVENT: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:e1\r\nDTSTAMP:20260101T000000Z\r\nDTSTART:20261001T100000Z\r\nSUMMARY:Dinner\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
const CARD: &str = "BEGIN:VCARD\r\nVERSION:4.0\r\nUID:c1\r\nFN:Ada Lovelace\r\nEMAIL:ada@example.org\r\nEND:VCARD\r\n";

fn source() -> Source {
    let mut files = BTreeMap::new();
    files.insert(
        (BackupItem::Settings, "settings.json".into()),
        br#"{"palette.recent":[]}"#.to_vec(),
    );
    files.insert(
        (BackupItem::Theme, "theme.kdl".into()),
        b"theme \"mine\"\n".to_vec(),
    );
    files.insert(
        (BackupItem::Layout, "layout.kdl".into()),
        b"layout\n".to_vec(),
    );
    files.insert(
        (BackupItem::Rules, "rules.kdl".into()),
        b"rule \"x\"\n".to_vec(),
    );
    files.insert(
        (BackupItem::Rules, "saved-searches.json".into()),
        b"[]".to_vec(),
    );
    files.insert(
        (BackupItem::Accounts, "accounts.kdl".into()),
        b"account \"work\" { password \"keychain:nail/work/password\" }\n".to_vec(),
    );
    files.insert(
        (BackupItem::Keys, "keyring.json".into()),
        b"{\"keys\":[1,2]}".to_vec(),
    );
    files.insert(
        (BackupItem::Plugins, "hello/manifest.json".into()),
        b"{}".to_vec(),
    );
    let mut mail = BTreeMap::new();
    let mut local = sample();
    local.folders[0].1[0].keywords = vec!["work".into()];
    mail.insert(AccountId::new("local-folders"), local);
    mail.insert(
        AccountId::new("opened-files"),
        MemoryMail {
            folders: vec![(
                "Opened".into(),
                vec![message("opened", "x\r\n", Flags::default())],
            )],
        },
    );
    Source {
        files,
        mail,
        calendars: MemoryPim(vec![(
            ExportCollection {
                key: "cal-1".into(),
                name: "Family".into(),
                items: None,
            },
            vec![EVENT.into()],
        )]),
        books: MemoryPim(vec![(
            ExportCollection {
                key: "book-1".into(),
                name: "Friends".into(),
                items: None,
            },
            vec![CARD.into()],
        )]),
        unstable: false,
        calls: 0,
    }
}

fn options(contents: BackupContents) -> BackupOptions {
    BackupOptions {
        contents,
        passphrase: PASSPHRASE.into(),
        kdf: KdfParams::insecure_for_tests(),
        overwrite: false,
        now: now(),
    }
}

fn ctx(kind: JobKind) -> JobContext<'static> {
    JobContext::silent(JobReport::started("j".into(), kind, "t"))
}

/// The core's state as a restore sees it.
#[derive(Default)]
struct Target {
    files: BTreeMap<(BackupItem, String), Vec<u8>>,
    merged: Vec<(BackupItem, String, bool)>,
    account: Option<AccountId>,
    accounts_begun: Vec<(AccountId, bool)>,
    folders: Vec<(AccountId, String)>,
    messages: Vec<(AccountId, String, ImportedMessage)>,
    pim: Vec<(String, String)>,
    backups: u32,
}

impl MailImportSink for Target {
    fn ensure_folder(&mut self, folder: &SourceFolder) -> std::result::Result<(), SinkError> {
        let entry = (self.account.clone().unwrap(), folder.path.clone());
        if !self.folders.contains(&entry) {
            self.folders.push(entry);
        }
        Ok(())
    }
    fn contains(
        &mut self,
        folder: &SourceFolder,
        key: &DuplicateKey,
    ) -> std::result::Result<bool, SinkError> {
        let account = self.account.clone().unwrap();
        Ok(self
            .messages
            .iter()
            .any(|(a, f, m)| *a == account && *f == folder.path && m.key == *key))
    }
    fn append(
        &mut self,
        folder: &SourceFolder,
        message: ImportedMessage,
    ) -> std::result::Result<(), SinkError> {
        self.messages
            .push((self.account.clone().unwrap(), folder.path.clone(), message));
        Ok(())
    }
}

impl PimImportSink for Target {
    fn event(
        &mut self,
        c: &SourceCollection,
        e: ParsedEvent,
    ) -> std::result::Result<(), SinkError> {
        self.pim.push((c.key.clone(), e.uid));
        Ok(())
    }
    fn task(&mut self, c: &SourceCollection, t: ParsedTask) -> std::result::Result<(), SinkError> {
        self.pim.push((c.key.clone(), t.uid));
        Ok(())
    }
    fn contact(
        &mut self,
        c: &SourceCollection,
        p: ParsedContact,
    ) -> std::result::Result<(), SinkError> {
        self.pim.push((c.key.clone(), p.uid));
        Ok(())
    }
    fn group(
        &mut self,
        c: &SourceCollection,
        g: ParsedGroup,
    ) -> std::result::Result<(), SinkError> {
        self.pim.push((c.key.clone(), g.group.uid));
        Ok(())
    }
    fn exists(&mut self, c: &SourceCollection, uid: &str) -> bool {
        self.pim.iter().any(|(k, u)| *k == c.key && u == uid)
    }
}

impl RestoreTarget for Target {
    fn current_file(
        &mut self,
        item: BackupItem,
        name: &str,
    ) -> std::result::Result<Option<Vec<u8>>, SinkError> {
        Ok(self.files.get(&(item, name.to_owned())).cloned())
    }
    fn replace_file(
        &mut self,
        item: BackupItem,
        name: &str,
        content: &[u8],
    ) -> std::result::Result<Option<ConfigBackupId>, SinkError> {
        let previous = self.files.insert((item, name.to_owned()), content.to_vec());
        Ok(previous.map(|_| {
            self.backups += 1;
            ConfigBackupId::new(format!("backup-{}", self.backups))
        }))
    }
    fn merge_file(
        &mut self,
        item: BackupItem,
        name: &str,
        _content: &[u8],
        dry_run: bool,
    ) -> std::result::Result<MergeOutcome, SinkError> {
        self.merged.push((item, name.to_owned(), dry_run));
        Ok(MergeOutcome {
            added: 2,
            replaced: 0,
            unchanged: 0,
        })
    }
    fn begin_mail_account(
        &mut self,
        account: &AccountId,
        dry_run: bool,
    ) -> std::result::Result<(), SinkError> {
        self.account = Some(account.clone());
        self.accounts_begun.push((account.clone(), dry_run));
        Ok(())
    }
}

#[test]
fn backup_inspect_verify_plan_restore() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("all.nailbackup");
    let mut source = source();
    let mut job = ctx(JobKind::Backup);
    let manifest = write_backup(
        &path,
        &options(BackupContents::default()),
        &mut source,
        &mut job,
    )
    .unwrap();
    assert!(job.report.issues.is_empty(), "{:?}", job.report.issues);
    assert_eq!(job.report.backup.as_ref(), Some(&manifest));
    assert!(!dir.path().join("all.nailbackup.nail-part").exists());

    // The manifest.
    assert_eq!(manifest.schema_version, 15);
    assert_eq!(manifest.counts.messages, 4);
    assert_eq!(manifest.counts.folders, 3);
    assert_eq!(manifest.counts.events, 1);
    assert_eq!(manifest.counts.contacts, 1);
    assert_eq!(manifest.counts.accounts, 1);
    assert_eq!(manifest.accounts, vec![AccountId::new("work")]);
    assert!(!manifest.contents.includes(BackupItem::Keys));
    assert_eq!(
        manifest.entries_of(BackupItem::Keys).count(),
        0,
        "keys are off by default"
    );
    let names: Vec<&str> = manifest.entries.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(
        names,
        vec![
            "config/settings.json",
            "config/theme.kdl",
            "config/layout.kdl",
            "config/rules.kdl",
            "config/saved-searches.json",
            "config/accounts.kdl",
            "mail/local-folders/1.mbox",
            "mail/local-folders/2.mbox",
            "mail/local-folders/index.json",
            "mail/opened-files/1.mbox",
            "mail/opened-files/index.json",
            "pim/1.ics",
            "pim/2.vcf",
            "pim/index.json",
            "plugins/hello/manifest.json",
        ]
    );
    // The archive does not show its content in clear text.
    let bytes = fs::read(&path).unwrap();
    assert!(bytes.starts_with(b"NAILBAK1"));
    assert!(!bytes.windows(7).any(|w| w == b"layout\n"));
    assert!(!bytes.windows(13).any(|w| w == b"manifest.json"));

    // Inspect and verify.
    assert_eq!(restore::inspect(&path, PASSPHRASE).unwrap(), manifest);
    let mut archive = BackupArchive::open(&path, PASSPHRASE).unwrap();
    let mut verify = ctx(JobKind::VerifyBackup);
    archive.verify(&mut verify).unwrap();
    assert!(verify.report.issues.is_empty());
    assert_eq!(verify.report.counts.files, manifest.entries.len() as u64);

    // The dry run against an empty Nail: everything is added, nothing written.
    let mut target = Target::default();
    let plan = archive
        .plan(&BackupContents::default(), &mut target)
        .unwrap();
    assert!(target.messages.is_empty() && target.files.is_empty() && target.pim.is_empty());
    assert!(target.accounts_begun.iter().all(|(_, dry)| *dry));
    let effects: Vec<(BackupItem, RestoreEffect, u64)> = plan
        .actions
        .iter()
        .map(|a| (a.item, a.effect, a.added))
        .collect();
    assert_eq!(
        effects,
        vec![
            (BackupItem::Settings, RestoreEffect::Add, 1),
            (BackupItem::Theme, RestoreEffect::Add, 1),
            (BackupItem::Layout, RestoreEffect::Add, 1),
            (BackupItem::Rules, RestoreEffect::Add, 2),
            (BackupItem::Accounts, RestoreEffect::Add, 1),
            (BackupItem::LocalMail, RestoreEffect::Add, 4),
            (BackupItem::PimLocal, RestoreEffect::Add, 2),
            (BackupItem::Plugins, RestoreEffect::Add, 2),
        ]
    );
    assert_eq!(plan.accounts_needing_secrets, vec![AccountId::new("work")]);
    assert!(plan.restart_required);
    assert_eq!(
        plan.actions[4].note.as_ref().map(|n| n.id.as_str()),
        Some("restore-note-accounts-need-passwords")
    );

    // Restore.
    target.files.insert(
        (BackupItem::Theme, "theme.kdl".into()),
        b"theme \"old\"\n".to_vec(),
    );
    let mut job = ctx(JobKind::Restore);
    let report = archive
        .restore(&BackupContents::default(), &mut target, &mut job)
        .unwrap();
    assert!(job.report.issues.is_empty(), "{:?}", job.report.issues);
    assert_eq!(job.report.restore.as_ref(), Some(&report));
    assert_eq!(report.config_backups, vec![ConfigBackupId::new("backup-1")]);
    assert_eq!(report.actions[1].effect, RestoreEffect::Replace);
    assert_eq!(
        target.files[&(BackupItem::Layout, "layout.kdl".to_owned())],
        b"layout\n"
    );
    assert_eq!(
        target.files[&(BackupItem::Theme, "theme.kdl".to_owned())],
        b"theme \"mine\"\n"
    );
    assert_eq!(target.messages.len(), 4);
    let original = &source.mail[&AccountId::new("local-folders")].folders[0].1[0];
    let (account, folder, restored) = &target.messages[0];
    assert_eq!(account.as_str(), "local-folders");
    assert_eq!(folder, "Inbox");
    assert_eq!(restored.raw, original.raw);
    assert_eq!(restored.flags, original.flags);
    assert_eq!(restored.keywords, vec!["work"]);
    assert_eq!(restored.received_at, Some(1_790_848_800));
    let lists = target
        .messages
        .iter()
        .find(|(_, f, _)| f == "Inbox/Lists")
        .unwrap();
    assert!(lists.2.flags.flagged);
    assert!(
        target
            .folders
            .contains(&(AccountId::new("local-folders"), "Inbox".into()))
    );
    assert!(
        target
            .folders
            .contains(&(AccountId::new("opened-files"), "Opened".into()))
    );
    assert_eq!(
        target.pim,
        vec![
            ("cal-1".into(), "e1".into()),
            ("book-1".into(), "c1".into())
        ]
    );
    assert_eq!(job.report.counts.messages, 4);

    // Restoring again changes nothing: mail is merged, files are equal.
    let plan = archive
        .plan(&BackupContents::default(), &mut target)
        .unwrap();
    let mail = plan
        .actions
        .iter()
        .find(|a| a.item == BackupItem::LocalMail)
        .unwrap();
    assert_eq!(
        (mail.effect, mail.added, mail.unchanged),
        (RestoreEffect::Skip, 0, 4)
    );
    let theme = plan
        .actions
        .iter()
        .find(|a| a.item == BackupItem::Theme)
        .unwrap();
    assert_eq!((theme.effect, theme.unchanged), (RestoreEffect::Skip, 1));
    let pim = plan
        .actions
        .iter()
        .find(|a| a.item == BackupItem::PimLocal)
        .unwrap();
    assert_eq!((pim.effect, pim.replaced), (RestoreEffect::Merge, 2));
    let mut job = ctx(JobKind::Restore);
    archive
        .restore(&BackupContents::default(), &mut target, &mut job)
        .unwrap();
    assert_eq!(target.messages.len(), 4);
    assert_eq!(job.report.counts.duplicates, 4);
    assert_eq!(job.report.finished_state(), JobState::Completed);
}

#[test]
fn only_chosen_items_are_restored_and_keys_when_included() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("keys.nailbackup");
    let contents = BackupContents {
        items: vec![BackupItem::Keys, BackupItem::Theme],
    };
    let manifest = write_backup(
        &path,
        &options(contents),
        &mut source(),
        &mut ctx(JobKind::Backup),
    )
    .unwrap();
    assert_eq!(manifest.entries.len(), 2);
    assert!(manifest.accounts.is_empty());
    let mut archive = BackupArchive::open(&path, PASSPHRASE).unwrap();
    let mut target = Target::default();
    let only_theme = BackupContents {
        items: vec![BackupItem::Theme],
    };
    let report = archive
        .restore(&only_theme, &mut target, &mut ctx(JobKind::Restore))
        .unwrap();
    let keys = report
        .actions
        .iter()
        .find(|a| a.item == BackupItem::Keys)
        .unwrap();
    assert_eq!(keys.effect, RestoreEffect::Skip);
    assert!(target.merged.is_empty());
    assert!(!report.restart_required);
    let all = BackupContents::all();
    let plan = archive.plan(&all, &mut target).unwrap();
    assert_eq!(plan.actions.len(), 2, "only items in the archive");
    assert_eq!(
        target.merged,
        vec![(BackupItem::Keys, "keyring.json".to_owned(), true)]
    );
}

#[test]
fn wrong_passphrases_and_other_files() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("x.nailbackup");
    let contents = BackupContents {
        items: vec![BackupItem::Theme],
    };
    write_backup(
        &path,
        &options(contents),
        &mut source(),
        &mut ctx(JobKind::Backup),
    )
    .unwrap();
    assert!(matches!(
        BackupArchive::open(&path, "not the passphrase"),
        Err(PortabilityError::WrongPassphrase)
    ));
    let other = dir.path().join("other.zip");
    fs::write(&other, b"PK\x03\x04whatever").unwrap();
    assert!(matches!(
        BackupArchive::open(&other, PASSPHRASE),
        Err(PortabilityError::NotABackup(_))
    ));
}

#[test]
fn options_are_checked() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("x.nailbackup");
    let mut short = options(BackupContents::default());
    short.passphrase = "short".into();
    assert!(matches!(
        write_backup(&path, &short, &mut source(), &mut ctx(JobKind::Backup)),
        Err(PortabilityError::InvalidOptions(_))
    ));
    let nothing = options(BackupContents { items: vec![] });
    assert!(matches!(
        write_backup(&path, &nothing, &mut source(), &mut ctx(JobKind::Backup)),
        Err(PortabilityError::InvalidOptions(_))
    ));
    fs::write(&path, b"existing").unwrap();
    assert!(matches!(
        write_backup(
            &path,
            &options(BackupContents::default()),
            &mut source(),
            &mut ctx(JobKind::Backup)
        ),
        Err(PortabilityError::TargetExists(_))
    ));
    let mut overwrite = options(BackupContents {
        items: vec![BackupItem::Theme],
    });
    overwrite.overwrite = true;
    write_backup(&path, &overwrite, &mut source(), &mut ctx(JobKind::Backup)).unwrap();
    assert!(BackupArchive::open(&path, PASSPHRASE).is_ok());
}

#[test]
fn a_source_that_changes_fails_without_leaving_a_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("x.nailbackup");
    let mut unstable = source();
    unstable.unstable = true;
    let result = write_backup(
        &path,
        &options(BackupContents::default()),
        &mut unstable,
        &mut ctx(JobKind::Backup),
    );
    assert!(matches!(result, Err(PortabilityError::SourceChanged(_))));
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
}

#[test]
fn cancelled_backups_leave_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("x.nailbackup");
    let mut job = ctx(JobKind::Backup);
    job.cancel_flag().cancel();
    let result = write_backup(
        &path,
        &options(BackupContents::default()),
        &mut source(),
        &mut job,
    );
    assert!(matches!(result, Err(PortabilityError::Cancelled)));
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
}

#[test]
fn verification_finds_damaged_and_missing_entries() {
    // An archive whose manifest lists a wrong hash and an entry that is
    // not there (written by hand with the same layers).
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("damaged.nailbackup");
    let manifest = BackupManifest {
        format_version: 1,
        app_version: "x".into(),
        schema_version: 15,
        created_at: "2026-10-02T00:00:00Z".into(),
        contents: BackupContents {
            items: vec![BackupItem::Theme, BackupItem::Layout],
        },
        entries: vec![
            BackupEntry {
                item: BackupItem::Theme,
                name: "config/theme.kdl".into(),
                size_bytes: 5,
                sha256: "00".repeat(32),
            },
            BackupEntry {
                item: BackupItem::Layout,
                name: "config/layout.kdl".into(),
                size_bytes: 3,
                sha256: "11".repeat(32),
            },
        ],
        counts: JobCounts::default(),
        accounts: vec![],
        plugins: vec![],
    };
    {
        let file = File::create(&path).unwrap();
        let encrypting =
            crypto::EncryptingWriter::new(file, PASSPHRASE, KdfParams::insecure_for_tests())
                .unwrap();
        let mut zip = ZipWriter::new_stream(encrypting);
        let options = SimpleFileOptions::default();
        zip.start_file(BACKUP_MANIFEST_ENTRY, options).unwrap();
        serde_json::to_writer(&mut zip, &manifest).unwrap();
        zip.start_file("config/theme.kdl", options).unwrap();
        zip.write_all(b"hello").unwrap();
        zip.finish().unwrap().into_inner().finish().unwrap();
    }
    let mut archive = BackupArchive::open(&path, PASSPHRASE).unwrap();
    let mut job = ctx(JobKind::VerifyBackup);
    archive.verify(&mut job).unwrap();
    let codes: Vec<_> = job.report.issues.iter().map(|i| i.code).collect();
    assert_eq!(
        codes,
        vec![
            PortabilityIssueCode::ChecksumMismatch,
            PortabilityIssueCode::EntryMissing
        ]
    );
    assert_eq!(job.report.counts.files, 0);
    assert_eq!(job.report.finished_state(), JobState::CompletedWithIssues);
    // A restore leaves damaged entries out and says so.
    let mut target = Target::default();
    let plan = archive.plan(&manifest.contents, &mut target).unwrap();
    assert_eq!(plan.issues.len(), 2);
    assert!(plan.actions.iter().all(|a| a.effect == RestoreEffect::Skip));
}

#[test]
fn the_standard_cost_round_trips() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("real.nailbackup");
    let mut options = options(BackupContents {
        items: vec![BackupItem::Layout],
    });
    options.kdf = KdfParams::STANDARD;
    write_backup(&path, &options, &mut source(), &mut ctx(JobKind::Backup)).unwrap();
    let manifest = restore::inspect(&path, PASSPHRASE).unwrap();
    assert_eq!(manifest.entries.len(), 1);
}

#[test]
fn entry_names() {
    assert!(valid_entry_name("config/theme.kdl"));
    assert!(valid_entry_name("plugins/a b/x.json"));
    for bad in ["", "/abs", "a/../b", "a//b", "a\\b", "c:x", "./a", "a\u{0}"] {
        assert!(!valid_entry_name(bad), "{bad:?}");
    }
    assert_eq!(item_prefix(BackupItem::Accounts), "config/");
}
