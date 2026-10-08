//! Mail for imports, exports and backups (ADRs 0094, 0099): the core's
//! [`MailImportSink`] (Local Folders, or an account's existing folders
//! through its operation queue) and [`MailExportSource`] (folders or chosen
//! messages, read from the store and blob store).
//!
//! Both run on the blocking pool (the store's blocking API). Imported
//! messages are written in batches; a message gets an id from its folder
//! and content, so importing the same file again finds the same rows.
//! `AppendMessage` operations queued for real accounts are announced by the
//! caller afterwards ([`Service::announce_enqueued`]).

use std::collections::{BTreeMap, HashSet};

use nail_api::accounts::IncomingServer;
use nail_api::portability::{ItemRef, LOCAL_FOLDERS_ACCOUNT_ID, MailImportTarget};
use nail_api::{
    Account, AccountId, AccountKind, ApiError, Folder, FolderId, FolderRole, MessageId,
};
use nail_portability::SinkError;
use nail_portability::sink::{
    DuplicateKey, ExportFolder, ExportMessage, ImportedMessage, MailExportSource, MailImportSink,
    SourceFolder, Visit,
};
use nail_store::{Enqueued, Operation};

use crate::error::Result;
use crate::ingest::{IncomingMessage, ingest};
use crate::service::Service;

/// Messages written per store transaction.
const BATCH: usize = 100;

/// The folders of a target account that a source folder may be filed in.
#[derive(Debug, Clone)]
enum Placement {
    /// Local Folders / a local account: the source tree is created.
    Tree {
        /// Path prefix of the created folders (`""` = top level).
        prefix: String,
        /// The folder the prefix names (its parent for the first level).
        parent: Option<FolderId>,
    },
    /// A real account: messages go into existing folders only (ADR 0099):
    /// the same-named folder below `base` when there is one, else `base`.
    Existing {
        /// The chosen folder (or the account's Inbox).
        base: Folder,
        /// Whether messages are queued as `AppendMessage` (IMAP, Gmail,
        /// Exchange; POP3 accounts keep them local).
        queue: bool,
    },
}

/// Where imported mail goes.
pub(crate) struct ImportTarget {
    service: Service,
    account: AccountId,
    placement: Placement,
    /// Source path → target folder.
    folders: BTreeMap<String, Folder>,
    /// Source folders filed into another folder (reported once each).
    pub merged: Vec<(String, String)>,
    pending: Vec<(Folder, IncomingMessage, DuplicateKey)>,
    /// Folders that received messages (for `MailChanged`).
    pub changed: HashSet<FolderId>,
    /// Folders the import created.
    pub created: Vec<FolderId>,
    /// Operations queued for real accounts (to announce).
    pub queued: Vec<Enqueued>,
    /// The account row was created (Local Folders on first use).
    pub account_created: bool,
    /// Resolve only (restore plans): nothing is written.
    dry_run: bool,
}

/// The display name of Local Folders.
fn local_folders_account(service: &Service) -> Account {
    Account {
        id: AccountId::new(LOCAL_FOLDERS_ACCOUNT_ID),
        name: crate::strings::message(service.inner.locale.as_deref(), "local-folders-account"),
        email: String::new(),
        kind: AccountKind::Local,
    }
}

fn store_err(err: impl std::fmt::Display) -> SinkError {
    SinkError::Fatal(err.to_string())
}

impl ImportTarget {
    /// Resolves the options' target (blocking). Local Folders is made on
    /// first use; a real account must exist and gets messages only in
    /// existing folders (`folder_name` is refused there).
    pub(crate) fn resolve(service: &Service, target: &MailImportTarget) -> Result<Self> {
        Self::resolve_with(service, target, false)
    }

    /// A local account's folder tree for a restore (`dry_run` = only
    /// look: the account and folders are not created).
    pub(crate) fn local_account(
        service: &Service,
        account: &AccountId,
        dry_run: bool,
    ) -> Result<Self> {
        let store = service.store();
        let mut account_created = false;
        if !dry_run && store.get_account(account).is_err() {
            let mut row = local_folders_account(service);
            if account.as_str() == nail_api::launch::OPENED_FILES_ACCOUNT_ID {
                row.name = crate::strings::message(
                    service.inner.locale.as_deref(),
                    "opened-files-account",
                );
            } else if account.as_str() != LOCAL_FOLDERS_ACCOUNT_ID {
                row.name = account.to_string();
            }
            row.id = account.clone();
            store.upsert_account(&row)?;
            account_created = true;
        }
        Ok(Self::new(
            service,
            account.clone(),
            Placement::Tree {
                prefix: String::new(),
                parent: None,
            },
            account_created,
            dry_run,
        ))
    }

    /// A target that has placed nothing yet.
    fn new(
        service: &Service,
        account: AccountId,
        placement: Placement,
        account_created: bool,
        dry_run: bool,
    ) -> Self {
        Self {
            service: service.clone(),
            account,
            placement,
            folders: BTreeMap::new(),
            merged: Vec::new(),
            pending: Vec::new(),
            changed: HashSet::new(),
            created: Vec::new(),
            queued: Vec::new(),
            account_created,
            dry_run,
        }
    }

    fn resolve_with(service: &Service, target: &MailImportTarget, dry_run: bool) -> Result<Self> {
        let store = service.store();
        let mut account_created = false;
        let (account, placement) = match &target.account_id {
            None => {
                let account = local_folders_account(service);
                if store.get_account(&account.id).is_err() {
                    store.upsert_account(&account)?;
                    account_created = true;
                }
                (
                    account.id,
                    Self::tree(service, target, &AccountId::new(LOCAL_FOLDERS_ACCOUNT_ID))?,
                )
            }
            Some(id) => {
                let stored = store
                    .get_account(id)
                    .map_err(|_| ApiError::not_found(format!("account {id}")))?;
                match stored.kind {
                    AccountKind::Local => (id.clone(), Self::tree(service, target, id)?),
                    AccountKind::Virtual => {
                        return Err(ApiError::invalid_request(
                            "mail cannot be imported into a virtual account",
                        )
                        .with_localized(nail_api::LocalizedMessage::new(
                            "portability-error-virtual-target",
                        ))
                        .into());
                    }
                    _ => {
                        if target
                            .folder_name
                            .as_deref()
                            .is_some_and(|n| !n.trim().is_empty())
                        {
                            return Err(ApiError::invalid_request(
                                "new folders are only created in Local Folders; choose an existing folder of the account",
                            )
                            .with_localized(nail_api::LocalizedMessage::new(
                                "portability-error-account-folders",
                            ))
                            .into());
                        }
                        let base = if let Some(folder) = &target.parent_folder_id {
                            let f = store.get_folder(folder)?;
                            if &f.account_id != id {
                                return Err(ApiError::invalid_request(format!(
                                    "folder {folder} is not in account {id}"
                                ))
                                .into());
                            }
                            f
                        } else {
                            let inbox = store.folder_with_role(id, FolderRole::Inbox)?.ok_or_else(
                                || ApiError::not_found(format!("the Inbox of account {id}")),
                            )?;
                            store.get_folder(&inbox)?
                        };
                        let queue = !matches!(
                            service
                                .inner
                                .accounts
                                .config()
                                .account(id)
                                .map(|a| &a.incoming),
                            Some(IncomingServer::Pop3 { .. }) | None
                        );
                        (id.clone(), Placement::Existing { base, queue })
                    }
                }
            }
        };
        Ok(Self::new(
            service,
            account,
            placement,
            account_created,
            dry_run,
        ))
    }

    fn tree(
        service: &Service,
        target: &MailImportTarget,
        account: &AccountId,
    ) -> Result<Placement> {
        let store = service.store();
        let (mut prefix, mut parent) = match &target.parent_folder_id {
            Some(id) => {
                let folder = store.get_folder(id)?;
                if &folder.account_id != account {
                    return Err(ApiError::invalid_request(format!(
                        "folder {id} is not in account {account}"
                    ))
                    .into());
                }
                (folder.path, Some(folder.id))
            }
            None => (String::new(), None),
        };
        if let Some(name) = target
            .folder_name
            .as_deref()
            .map(str::trim)
            .filter(|n| !n.is_empty())
        {
            let name = nail_portability::folder::sanitize_folder_name(name).0;
            let path = join(&prefix, &name);
            let folder = Folder {
                id: crate::ids::folder_id(account, &path),
                account_id: account.clone(),
                parent_id: parent.clone(),
                name,
                path: path.clone(),
                role: FolderRole::Other,
                unread: 0,
                total: 0,
            };
            if store.get_folder(&folder.id).is_err() {
                store.upsert_folder(&folder)?;
            }
            prefix = path;
            parent = Some(folder.id);
        }
        Ok(Placement::Tree { prefix, parent })
    }

    fn folder_for(&mut self, source: &SourceFolder) -> Result<Folder, SinkError> {
        if let Some(folder) = self.folders.get(&source.path) {
            return Ok(folder.clone());
        }
        let store = self.service.store();
        let folder = match &self.placement {
            Placement::Tree { prefix, parent } => {
                let path = join(prefix, &source.path);
                let parent_id = match &source.parent {
                    Some(p) => match self.folders.get(p) {
                        Some(f) => Some(f.id.clone()),
                        None => Some(crate::ids::folder_id(&self.account, &join(prefix, p))),
                    },
                    None => parent.clone(),
                };
                let folder = Folder {
                    id: crate::ids::folder_id(&self.account, &path),
                    account_id: self.account.clone(),
                    parent_id,
                    name: source.name.clone(),
                    path,
                    role: FolderRole::Other,
                    unread: 0,
                    total: 0,
                };
                match store.get_folder(&folder.id) {
                    Ok(existing) => existing,
                    Err(_) if self.dry_run => folder,
                    Err(_) => {
                        store.upsert_folder(&folder).map_err(store_err)?;
                        self.created.push(folder.id.clone());
                        folder
                    }
                }
            }
            Placement::Existing { base, .. } => {
                let wanted = format!("{}/{}", base.path, source.path);
                let found = store
                    .list_folders(&self.account)
                    .map_err(store_err)?
                    .into_iter()
                    .find(|f| {
                        f.path.eq_ignore_ascii_case(&wanted)
                            || f.path
                                .replace(['.', '\\'], "/")
                                .eq_ignore_ascii_case(&wanted)
                            || (f.parent_id.as_ref() == Some(&base.id)
                                && f.name.eq_ignore_ascii_case(&source.name)
                                && source.parent.is_none())
                    });
                if let Some(folder) = found {
                    folder
                } else {
                    self.merged.push((source.path.clone(), base.path.clone()));
                    base.clone()
                }
            }
        };
        self.folders.insert(source.path.clone(), folder.clone());
        Ok(folder)
    }

    /// Writes the pending batch.
    pub(crate) fn flush(&mut self) -> Result<(), SinkError> {
        if self.pending.is_empty() || self.dry_run {
            return Ok(());
        }
        let batch = std::mem::take(&mut self.pending);
        let store = self.service.store();
        let blobs = self.service.blobs();
        let index = self.service.inner.search.get();
        let queue = matches!(self.placement, Placement::Existing { queue: true, .. });
        let mut by_folder: BTreeMap<FolderId, (Folder, Vec<IncomingMessage>)> = BTreeMap::new();
        for (folder, message, _) in batch {
            by_folder
                .entry(folder.id.clone())
                .or_insert_with(|| (folder, Vec::new()))
                .1
                .push(message);
        }
        for (_, (folder, messages)) in by_folder {
            let report =
                ingest(store, blobs, Some(&index), &folder, 0, &messages).map_err(store_err)?;
            self.changed.insert(folder.id.clone());
            if queue && !report.new.is_empty() {
                let ops: Vec<Operation> = report
                    .new
                    .iter()
                    .map(|id| Operation::AppendMessage {
                        message_id: id.clone(),
                        folder_id: folder.id.clone(),
                    })
                    .collect();
                self.queued
                    .extend(store.enqueue_all(&ops).map_err(store_err)?);
            }
        }
        Ok(())
    }
}

fn join(prefix: &str, path: &str) -> String {
    if prefix.is_empty() {
        path.to_owned()
    } else {
        format!("{prefix}/{path}")
    }
}

/// Unix seconds of an RFC 3339 date.
fn unix_of(date: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(date)
        .ok()
        .map(|d| d.timestamp())
}

impl MailImportSink for ImportTarget {
    fn ensure_folder(&mut self, folder: &SourceFolder) -> Result<(), SinkError> {
        self.folder_for(folder).map(|_| ())
    }

    fn contains(&mut self, folder: &SourceFolder, key: &DuplicateKey) -> Result<bool, SinkError> {
        let target = self.folder_for(folder)?;
        if self
            .pending
            .iter()
            .any(|(f, _, k)| f.id == target.id && k == key)
        {
            return Ok(true);
        }
        let Some(message_id) = &key.message_id else {
            return Ok(false);
        };
        let store = self.service.store();
        let candidates = store
            .find_messages_by_message_id(message_id)
            .map_err(store_err)?;
        for found in candidates.iter().filter(|m| m.folder_id == target.id) {
            let size = store.message_meta(&found.id).ok().and_then(|m| m.size);
            let same_size = size.is_none_or(|s| s == key.size);
            let same_date = match (key.date, unix_of(&found.date)) {
                (Some(a), Some(b)) => a == b,
                _ => true,
            };
            if same_size && same_date {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn append(&mut self, folder: &SourceFolder, message: ImportedMessage) -> Result<(), SinkError> {
        let target = self.folder_for(folder)?;
        let content = hex::encode(<sha2::Sha256 as sha2::Digest>::digest(&message.raw));
        let id: MessageId = crate::ids::local_message_id(&target.id, &content);
        let incoming = IncomingMessage {
            id: Some(id),
            flags: message.flags,
            keywords: message.keywords,
            internal_date: message.received_at.map(nail_util::time::rfc3339_utc),
            size: Some(message.raw.len() as u64),
            raw: message.raw,
            ..IncomingMessage::default()
        };
        self.pending.push((target, incoming, message.key));
        if self.pending.len() >= BATCH {
            self.flush()?;
        }
        Ok(())
    }
}

/// Item reference of a message for issues.
fn message_ref(folder: &str, id: &MessageId) -> ItemRef {
    ItemRef {
        folder: Some(folder.to_owned()),
        label: Some(id.to_string()),
        ..ItemRef::default()
    }
}

/// What an export or a backup reads: folders (by export path) with their
/// store folder, or one pseudo-folder of chosen messages.
pub(crate) struct StoreMailSource {
    service: Service,
    folders: Vec<(ExportFolder, SourceSet)>,
    /// Fetch bodies of headers-only messages before exporting them.
    fetch_bodies: bool,
}

#[derive(Debug, Clone)]
enum SourceSet {
    Folder(FolderId),
    Messages(Vec<MessageId>),
}

impl StoreMailSource {
    /// Folders by id, with their subfolders when asked (blocking); export
    /// paths are relative to the chosen folders.
    pub(crate) fn folders(
        service: &Service,
        folder_ids: &[FolderId],
        include_subfolders: bool,
        fetch_bodies: bool,
    ) -> Result<Self> {
        let store = service.store();
        let mut out: Vec<(ExportFolder, SourceSet)> = Vec::new();
        let mut used: HashSet<String> = HashSet::new();
        for id in folder_ids {
            let root = store.get_folder(id)?;
            let mut name = nail_portability::folder::sanitize_folder_name(&root.name).0;
            let base = name.clone();
            let mut n = 1;
            while !used.insert(name.clone()) {
                n += 1;
                name = format!("{base} ({n})");
            }
            out.push((
                ExportFolder {
                    path: name.clone(),
                    messages: Some(u64::from(root.total)),
                    size_bytes: None,
                },
                SourceSet::Folder(root.id.clone()),
            ));
            if include_subfolders {
                let all = store.list_folders(&root.account_id)?;
                let mut stack = vec![(root.id.clone(), name.clone())];
                while let Some((parent, path)) = stack.pop() {
                    let mut children: Vec<&Folder> = all
                        .iter()
                        .filter(|f| f.parent_id.as_ref() == Some(&parent))
                        .collect();
                    children.sort_by(|a, b| a.name.cmp(&b.name));
                    for child in children {
                        let child_path = format!(
                            "{path}/{}",
                            nail_portability::folder::sanitize_folder_name(&child.name).0
                        );
                        out.push((
                            ExportFolder {
                                path: child_path.clone(),
                                messages: Some(u64::from(child.total)),
                                size_bytes: None,
                            },
                            SourceSet::Folder(child.id.clone()),
                        ));
                        stack.push((child.id.clone(), child_path));
                    }
                }
            }
        }
        // Parents first.
        out.sort_by(|a, b| a.0.path.cmp(&b.0.path));
        Ok(Self {
            service: service.clone(),
            folders: out,
            fetch_bodies,
        })
    }

    /// Chosen messages as one folder named `name`.
    pub(crate) fn messages(
        service: &Service,
        ids: Vec<MessageId>,
        name: &str,
        fetch_bodies: bool,
    ) -> Self {
        Self {
            service: service.clone(),
            folders: vec![(
                ExportFolder {
                    path: name.to_owned(),
                    messages: Some(ids.len() as u64),
                    size_bytes: None,
                },
                SourceSet::Messages(ids),
            )],
            fetch_bodies,
        }
    }

    /// Every folder of a local account (backups), paths = folder paths.
    pub(crate) fn account(service: &Service, account: &AccountId) -> Result<Self> {
        let store = service.store();
        let mut folders = store.list_folders(account)?;
        folders.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(Self {
            service: service.clone(),
            folders: folders
                .into_iter()
                .map(|f| {
                    (
                        ExportFolder {
                            path: f.path.clone(),
                            messages: Some(u64::from(f.total)),
                            size_bytes: None,
                        },
                        SourceSet::Folder(f.id),
                    )
                })
                .collect(),
            fetch_bodies: false,
        })
    }

    /// How many folders the export writes.
    pub(crate) fn folder_count(&self) -> usize {
        self.folders.len()
    }

    fn ids_of(&self, set: &SourceSet) -> Result<Vec<MessageId>, SinkError> {
        match set {
            SourceSet::Messages(ids) => Ok(ids.clone()),
            SourceSet::Folder(folder) => {
                let store = self.service.store();
                let mut ids = Vec::new();
                let mut offset = 0;
                loop {
                    let page = store
                        .list_messages(folder, offset, 500)
                        .map_err(store_err)?;
                    let n = u32::try_from(page.messages.len()).unwrap_or(u32::MAX);
                    ids.extend(page.messages.into_iter().map(|m| m.id));
                    offset += n;
                    if n == 0 || offset >= page.total {
                        break;
                    }
                }
                // Oldest first (the store lists newest first).
                ids.reverse();
                Ok(ids)
            }
        }
    }
}

impl MailExportSource for StoreMailSource {
    fn folders(&mut self) -> Result<Vec<ExportFolder>, SinkError> {
        Ok(self.folders.iter().map(|(f, _)| f.clone()).collect())
    }

    fn messages(
        &mut self,
        folder: &ExportFolder,
        visit: &mut dyn FnMut(Result<ExportMessage, SinkError>) -> Visit,
    ) -> Result<(), SinkError> {
        let Some((_, set)) = self.folders.iter().find(|(f, _)| f.path == folder.path) else {
            return Ok(());
        };
        let set = set.clone();
        let ids = self.ids_of(&set)?;
        let store = self.service.store();
        if self.fetch_bodies {
            let missing: Vec<MessageId> = ids
                .iter()
                .filter(|id| {
                    store
                        .body_state(id)
                        .is_ok_and(|s| s == nail_api::sync::BodyState::HeadersOnly)
                })
                .cloned()
                .collect();
            if !missing.is_empty() {
                let service = self.service.clone();
                let fetched = tokio::runtime::Handle::current()
                    .block_on(async move { service.fetch_bodies(missing).await });
                if let Err(err) = fetched {
                    tracing::warn!(%err, "cannot fetch bodies for an export");
                }
            }
        }
        for id in ids {
            let item = (|| -> Result<ExportMessage, SinkError> {
                let stored = store
                    .get_message(&id)
                    .map_err(|e| SinkError::Item(e.to_string()))?;
                let hash = stored.blob_hash.ok_or_else(|| {
                    SinkError::Item(format!("message {id} has no stored content"))
                })?;
                let raw = self
                    .service
                    .blobs()
                    .get(&hash)
                    .map_err(|e| SinkError::Item(e.to_string()))?;
                let summary = stored.summary;
                Ok(ExportMessage {
                    raw,
                    flags: summary.flags,
                    keywords: summary.tags,
                    received_at: unix_of(&summary.date),
                    envelope_from: None,
                })
            })();
            let item = item.map_err(|e| match e {
                SinkError::Item(detail) => {
                    let r = message_ref(&folder.path, &id);
                    SinkError::Item(format!("{detail} ({})", r.label.unwrap_or_default()))
                }
                other @ SinkError::Fatal(_) => other,
            });
            if visit(item) == Visit::Stop {
                break;
            }
        }
        Ok(())
    }
}
