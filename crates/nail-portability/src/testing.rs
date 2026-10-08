//! Test doubles of the core's side (feature `testing`, on in the crate's
//! own tests): an in-memory import sink and an in-memory PIM export
//! source, one definition for every test that needs them.

use std::collections::BTreeMap;

use nail_ical::vcard::{ParsedContact, ParsedGroup};
use nail_ical::{ParsedEvent, ParsedTask};

use nail_api::portability::{ImportOptions, ImportSource, JobKind, JobReport, JobState};

use crate::error::SinkError;
use crate::job::JobContext;
use crate::sink::{
    DuplicateKey, ExportCollection, ImportedMessage, MailImportSink, NullSink, PimExportSource,
    PimImportSink, SourceCollection, SourceFolder, Visit,
};

/// The passphrase of the backups tests write.
pub const PASSPHRASE: &str = "correct horse battery staple";

/// A sink that keeps everything in memory, like the core's store: folders
/// as created, messages in arrival order, PIM items as `(collection,
/// title)`. A message containing `FAIL` is refused (an item error).
#[derive(Debug, Default)]
pub struct MemorySink {
    /// The folders, in creation order.
    pub folders: Vec<SourceFolder>,
    /// Every stored message with its folder path, in arrival order.
    pub appended: Vec<(String, ImportedMessage)>,
    /// Every stored PIM item: collection name and title (`group <name>`
    /// for a group).
    pub pim: Vec<(String, String)>,
}

impl MemorySink {
    /// Imports `source` into this sink with the default options (no PIM
    /// sink); returns the job's report.
    pub fn import(&mut self, source: &ImportSource) -> crate::Result<JobReport> {
        let mut ctx = JobContext::silent(JobReport::started("i".into(), JobKind::Import, "t"));
        crate::import::run(
            source,
            &ImportOptions::default(),
            self,
            &mut NullSink,
            &mut ctx,
        )?;
        Ok(ctx.into_report())
    }

    /// The messages of one folder, in arrival order.
    pub fn messages(&self, folder: &str) -> Vec<&ImportedMessage> {
        self.appended
            .iter()
            .filter(|(f, _)| f == folder)
            .map(|(_, m)| m)
            .collect()
    }
}

impl MailImportSink for MemorySink {
    fn ensure_folder(&mut self, folder: &SourceFolder) -> Result<(), SinkError> {
        if !self.folders.iter().any(|f| f.path == folder.path) {
            self.folders.push(folder.clone());
        }
        Ok(())
    }

    fn contains(&mut self, folder: &SourceFolder, key: &DuplicateKey) -> Result<bool, SinkError> {
        Ok(self
            .appended
            .iter()
            .any(|(f, m)| *f == folder.path && m.key == *key))
    }

    fn append(&mut self, folder: &SourceFolder, message: ImportedMessage) -> Result<(), SinkError> {
        if message.raw.windows(4).any(|w| w == b"FAIL") {
            return Err(SinkError::Item("refused".into()));
        }
        self.appended.push((folder.path.clone(), message));
        Ok(())
    }
}

impl PimImportSink for MemorySink {
    fn event(&mut self, c: &SourceCollection, e: ParsedEvent) -> Result<(), SinkError> {
        self.pim.push((c.name.clone(), e.fields.title));
        Ok(())
    }

    fn task(&mut self, c: &SourceCollection, t: ParsedTask) -> Result<(), SinkError> {
        self.pim.push((c.name.clone(), t.fields.title));
        Ok(())
    }

    fn contact(&mut self, c: &SourceCollection, p: ParsedContact) -> Result<(), SinkError> {
        self.pim.push((c.name.clone(), p.fields.display_name));
        Ok(())
    }

    fn group(&mut self, c: &SourceCollection, g: ParsedGroup) -> Result<(), SinkError> {
        self.pim
            .push((c.name.clone(), format!("group {}", g.group.name)));
        Ok(())
    }
}

/// PIM collections in memory, each with its items' texts.
#[derive(Debug, Clone, Default)]
pub struct MemoryPim {
    collections: Vec<ExportCollection>,
    items: BTreeMap<String, Vec<String>>,
}

impl MemoryPim {
    /// Collections with their items' texts.
    pub fn new(collections: Vec<(ExportCollection, Vec<String>)>) -> Self {
        let mut pim = Self::default();
        for (collection, items) in collections {
            pim.items.insert(collection.key.clone(), items);
            pim.collections.push(collection);
        }
        pim
    }
}

impl PimExportSource for MemoryPim {
    fn collections(&mut self) -> Result<Vec<ExportCollection>, SinkError> {
        Ok(self.collections.clone())
    }

    fn items(
        &mut self,
        collection: &ExportCollection,
        visit: &mut dyn FnMut(Result<String, SinkError>) -> Visit,
    ) -> Result<(), SinkError> {
        for item in self.items.get(&collection.key).into_iter().flatten() {
            visit(Ok(item.clone()));
        }
        Ok(())
    }
}

/// Checks a finished import of the Thunderbird fixture profile
/// (`tests/fixtures/thunderbird`): 4 messages, 4 contacts and 2 groups,
/// completed with issues (the fixture's deliberate problems).
///
/// # Panics
///
/// When the report says otherwise.
pub fn assert_thunderbird_fixture_imported(report: &JobReport) {
    assert_eq!(report.state, JobState::CompletedWithIssues);
    assert_eq!(report.counts.messages, 4);
    assert_eq!(report.counts.contacts, 4);
    assert_eq!(report.counts.groups, 2);
}
