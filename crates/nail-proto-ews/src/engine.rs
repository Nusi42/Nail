//! [`EwsEngine`]: the EWS implementation of
//! [`ExchangeEngine`] for one account.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use nail_api::AccountId;
use nail_api::accounts::{AccountConfig, AuthConfig, IncomingServer, SendingMailbox};
use nail_api::engine::{
    AccessTokenProvider, BoxFuture, DeltaOutcome, EngineError, Envelope, FreeBusyProvider,
    OutgoingTransport, RemoteWrite, SecretStore, SendReceipt, SyncDelta, WriteOutcome,
};
use nail_api::exchange::engine::{
    ExchangeDirectoryEngine, ExchangeEngine, ExchangeFolderClass, ExchangeMailEngine,
    ExchangePimEngine, ExchangePimWrite, ExchangePimWriteOutcome, ExchangeServerInfo,
    ExchangeSettingsEngine, HierarchyDelta, HierarchyOutcome, MeetingSend, NegotiateAuth, PimDelta,
    PimDeltaOutcome, RemoteCategory,
};
use nail_api::exchange::pim::{ExchangeEvent, ExchangePimFolder, ExchangePimItem, PimFolderKind};
use nail_api::exchange::rules::ExchangeInboxRule;
use nail_api::exchange::{
    AutomaticRepliesSettings, ExchangeCategorySettings, ExchangeCollectionRole,
    ExchangeEngine as EngineKind, ExchangeWellKnownFolder, InferenceClassification, Room, RoomList,
};
use nail_api::scheduling::{AttendeeFreeBusy, FreeBusyQuery, FreeBusySource, InvitationResponse};

use crate::auth::{EwsAuth, Password};
use crate::calendar::{
    self, EVENT_FIELDS, create_send, delete_send, event_create_xml, event_updates,
    modified_occurrences, parse_event, update_send, write_zone,
};
use crate::client::{EwsClient, Operation};
use crate::contacts::{
    CONTACT_SHAPE, contact_create_xml, contact_tag, contact_updates, parse_contact,
};
use crate::error::EwsError;
use crate::mail::{
    self, DISTINGUISHED, MAX_SYNC_CHANGES, SyncOutcome, exchange_folder, message_shape,
    remote_change, remote_message, with_envelope_recipients,
};
use crate::notify::StreamingWatcher;
use crate::settings::{CategoryEdit, RuleOperation, new_guid, parse_category_list, same_rule};
use crate::soap;
use crate::tasks::{TASK_FIELDS, parse_task, task_create_xml, task_updates, task_zone};
use crate::types::{FolderRef, ItemRef, Update};
use crate::xml::Element;

/// Tunables of an [`EwsEngine`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EwsOptions {
    /// Push through streaming notifications (`false` = the core polls).
    pub streaming: bool,
    /// Read `InferenceClassification` (servers with Focused Inbox; on-
    /// premises Exchange has none, and older servers refuse the field).
    pub focused_inbox: bool,
    /// Changes per `SyncFolderItems` call (1–512).
    pub page_size: u32,
    /// Keep a copy of sent mail in Sent Items (`SendAndSaveCopy`).
    pub save_sent_copy: bool,
    /// Let inbox rule changes delete Outlook's client-only rules.
    pub remove_outlook_rules: bool,
}

impl Default for EwsOptions {
    fn default() -> Self {
        Self {
            streaming: true,
            focused_inbox: false,
            page_size: MAX_SYNC_CHANGES,
            save_sent_copy: true,
            remove_outlook_rules: false,
        }
    }
}

/// The well-known folders of a mailbox, by id.
#[derive(Debug, Clone, Default)]
struct WellKnown {
    root: Option<String>,
    by_id: HashMap<String, ExchangeWellKnownFolder>,
}

/// The EWS engine of one account.
#[derive(Debug)]
pub struct EwsEngine {
    client: EwsClient,
    account_id: AccountId,
    options: EwsOptions,
    watcher: StreamingWatcher,
    well_known: Mutex<HashMap<Option<String>, WellKnown>>,
    rooms: Mutex<HashSet<String>>,
}

fn key(mailbox: Option<&str>) -> Option<String> {
    mailbox.map(str::to_ascii_lowercase)
}

impl EwsEngine {
    /// An engine on `client` for `account_id` (named in free/busy
    /// answers) with default options.
    pub fn new(client: EwsClient, account_id: AccountId) -> Self {
        Self {
            client,
            account_id,
            options: EwsOptions::default(),
            watcher: StreamingWatcher::new(),
            well_known: Mutex::new(HashMap::new()),
            rooms: Mutex::new(HashSet::new()),
        }
    }

    /// The engine of a configured `exchange engine="ews"` account: the
    /// endpoint from its options, the account's address as own mailbox,
    /// and its login — Basic with the keychain password (`password`),
    /// OAuth2 bearer tokens (`oauth2`, scope `EWS.AccessAsUser.All`) or
    /// Windows sign-in through `negotiate` (`windows`; built by the core
    /// with the account's user / password or the signed-in user).
    pub fn for_account(
        account: &AccountConfig,
        secrets: Arc<dyn SecretStore>,
        tokens: Arc<dyn AccessTokenProvider>,
        negotiate: Option<Arc<dyn NegotiateAuth>>,
    ) -> Result<Self, EwsError> {
        let IncomingServer::Exchange { server, options } = &account.incoming else {
            return Err(EwsError::Config(account.id.to_string()));
        };
        if options.engine != EngineKind::Ews {
            return Err(EwsError::Config(format!("{} uses Graph", account.id)));
        }
        let auth = match &server.auth {
            AuthConfig::Password { username, .. } => EwsAuth::Basic {
                username: username.clone(),
                password: Password::Store {
                    store: secrets,
                    secret: server
                        .auth
                        .secret_ref(&account.id)
                        .ok_or_else(|| EwsError::Config(account.id.to_string()))?,
                },
            },
            AuthConfig::OAuth2 { provider, .. } => EwsAuth::OAuth2 {
                tokens,
                provider: provider.clone(),
                secret: server
                    .auth
                    .secret_ref(&account.id)
                    .ok_or_else(|| EwsError::Config(account.id.to_string()))?,
            },
            AuthConfig::Windows { .. } => EwsAuth::Negotiate(negotiate.ok_or_else(|| {
                EwsError::Config(format!("{}: no Windows sign-in available", account.id))
            })?),
            AuthConfig::ClientCertificate { .. } | AuthConfig::None => {
                return Err(EwsError::Config(format!(
                    "{}: EWS needs a password, OAuth2 or Windows sign-in",
                    account.id
                )));
            }
        };
        let client =
            EwsClient::new(&options.ews_url(server), auth)?.with_own_address(&account.email);
        Ok(
            Self::new(client, account.id.clone()).with_options(EwsOptions {
                streaming: options.streaming(),
                save_sent_copy: account.save_sent_copy.unwrap_or(true),
                ..EwsOptions::default()
            }),
        )
    }

    /// Replaces the options.
    #[must_use]
    pub fn with_options(mut self, options: EwsOptions) -> Self {
        self.options = options;
        self
    }

    /// The client.
    pub fn client(&self) -> &EwsClient {
        &self.client
    }

    /// The options.
    pub fn options(&self) -> EwsOptions {
        self.options
    }

    /// The streaming subscriptions.
    pub fn watcher(&self) -> &StreamingWatcher {
        &self.watcher
    }

    /// The well-known folders of a mailbox (looked up once).
    async fn well_known(&self, mailbox: Option<&str>) -> Result<WellKnown, EwsError> {
        if let Some(found) = self
            .well_known
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&key(mailbox))
        {
            return Ok(found.clone());
        }
        let refs: Vec<FolderRef<'_>> = DISTINGUISHED
            .iter()
            .filter_map(|f| f.ews_name())
            .map(|name| FolderRef::distinguished(name, mailbox))
            .collect();
        let answers = self.client.get_folders(mailbox, &refs).await?;
        let mut out = WellKnown::default();
        for (known, answer) in DISTINGUISHED.iter().zip(answers) {
            let folder = match answer {
                Ok(folder) => folder,
                // The root decides access; others may be missing or hidden.
                Err(e) if *known == ExchangeWellKnownFolder::Root => return Err(e),
                Err(_) => continue,
            };
            if let Some(id) = folder.child("FolderId").and_then(|f| f.attr("Id")) {
                if *known == ExchangeWellKnownFolder::Root {
                    out.root = Some(id.to_owned());
                } else {
                    out.by_id.insert(id.to_owned(), *known);
                }
            }
        }
        self.well_known
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(key(mailbox), out.clone());
        Ok(out)
    }

    async fn hierarchy(
        &self,
        mailbox: Option<&str>,
        cursor: Option<&str>,
    ) -> Result<HierarchyOutcome, EwsError> {
        let known = self.well_known(mailbox).await?;
        let SyncOutcome::Changes(changes) =
            self.client.sync_folder_hierarchy(mailbox, cursor).await?
        else {
            return Ok(HierarchyOutcome::CursorExpired);
        };
        let lookup = |id: &str| known.by_id.get(id).copied();
        Ok(HierarchyOutcome::Delta(HierarchyDelta {
            cursor: changes.sync_state,
            upserted: changes
                .upserted
                .iter()
                .filter_map(|f| exchange_folder(f, known.root.as_deref(), &lookup))
                .collect(),
            removed: changes.deleted,
            more: !changes.complete,
        }))
    }

    async fn items(
        &self,
        mailbox: Option<&str>,
        folder: &str,
        cursor: Option<&str>,
        since: Option<i64>,
    ) -> Result<DeltaOutcome, EwsError> {
        let shape = message_shape(self.options.focused_inbox);
        let SyncOutcome::Changes(changes) = self
            .client
            .sync_folder_items(
                mailbox,
                FolderRef::named(mailbox, folder),
                cursor,
                &shape,
                self.options.page_size,
            )
            .await?
        else {
            return Ok(DeltaOutcome::CursorExpired);
        };
        let first_pass = cursor.is_none();
        let mut delta = SyncDelta {
            cursor: changes.sync_state,
            removed: changes.deleted,
            more: !changes.complete,
            ..SyncDelta::default()
        };
        for item in &changes.created {
            if let Some(mut message) = remote_message(item, folder, mailbox) {
                message.folder = folder.to_owned();
                if first_pass
                    && let (Some(since), Some(date)) = (since, message.internal_date)
                    && date < since
                {
                    continue;
                }
                delta.added.push(message);
            }
        }
        delta.changed.extend(
            changes
                .updated
                .iter()
                .filter_map(|i| remote_change(i, mailbox)),
        );
        // A read-state change names only the item: read its properties.
        let read_ids: Vec<ItemRef> = changes
            .read_changes
            .iter()
            .filter(|(id, _)| !delta.changed.iter().any(|c| &c.remote_id == id))
            .map(|(id, _)| ItemRef::new(id.clone()))
            .collect();
        if !read_ids.is_empty() {
            for item in self.client.get_items(mailbox, &read_ids, &shape).await? {
                match item {
                    Ok(item) => delta.changed.extend(remote_change(&item, mailbox)),
                    Err(EwsError::NotFound(_)) => {}
                    Err(e) => return Err(e),
                }
            }
        }
        Ok(DeltaOutcome::Delta(delta))
    }

    async fn replay(
        &self,
        mailbox: Option<&str>,
        write: &RemoteWrite,
    ) -> Result<WriteOutcome, EwsError> {
        let gone = |result: Result<WriteOutcome, EwsError>| match result {
            Err(EwsError::NotFound(_)) => Ok(WriteOutcome::Gone),
            other => other,
        };
        match write {
            RemoteWrite::SetFlags { remote_id, change } => gone(
                self.client
                    .apply_flag_change(mailbox, remote_id, change)
                    .await
                    .map(|_| WriteOutcome::Done { remote_id: None }),
            ),
            RemoteWrite::Move {
                remote_id,
                to_folder,
                ..
            } => gone(
                self.client
                    .move_item(
                        mailbox,
                        remote_id,
                        FolderRef::named(mailbox, to_folder),
                        false,
                    )
                    .await
                    .map(|new| WriteOutcome::Done {
                        remote_id: Some(new.id),
                    }),
            ),
            RemoteWrite::Trash { remote_id } => gone(
                self.client
                    .move_item(
                        mailbox,
                        remote_id,
                        FolderRef::distinguished("deleteditems", mailbox),
                        false,
                    )
                    .await
                    .map(|new| WriteOutcome::Done {
                        remote_id: Some(new.id),
                    }),
            ),
            RemoteWrite::Delete { remote_id } => gone(
                self.client
                    .delete_item(mailbox, &ItemRef::new(remote_id.clone()), "HardDelete", "")
                    .await
                    .map(|()| WriteOutcome::Done { remote_id: None }),
            ),
            RemoteWrite::Append { folder, raw, flags } => {
                let target = FolderRef::named(mailbox, folder);
                // Idempotent: a replay after a crash finds the first copy.
                if let Some(id) = mail::message_id(raw)
                    && let Some(found) =
                        self.client.find_by_message_id(mailbox, target, &id).await?
                {
                    return Ok(WriteOutcome::Done {
                        remote_id: Some(found.id),
                    });
                }
                let created = self
                    .client
                    .create_mime_item(mailbox, target, raw, *flags)
                    .await?;
                Ok(WriteOutcome::Done {
                    remote_id: Some(created.id),
                })
            }
        }
    }

    async fn classify_items(
        &self,
        mailbox: Option<&str>,
        remote_ids: &[String],
        classification: InferenceClassification,
        always: bool,
    ) -> Result<(), EwsError> {
        if !self.options.focused_inbox {
            return Err(EwsError::Unsupported(
                "this Exchange server has no Focused Inbox".into(),
            ));
        }
        if always {
            return Err(EwsError::Unsupported(
                "EWS cannot add Focused Inbox overrides".into(),
            ));
        }
        let value = match classification {
            InferenceClassification::Focused => "Focused",
            InferenceClassification::Other => "Other",
        };
        for id in remote_ids {
            let update = Update::Set(
                crate::types::Field::Uri("item:InferenceClassification"),
                crate::xml::el("t:InferenceClassification", value),
            );
            match self
                .client
                .update_item(mailbox, &ItemRef::new(id.clone()), "Message", &[update], "")
                .await
            {
                Ok(_) | Err(EwsError::NotFound(_)) => {}
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }

    // -- settings -------------------------------------------------------

    async fn create_rule(&self, rule: &ExchangeInboxRule) -> Result<String, EwsError> {
        self.client
            .update_inbox_rules(
                &[RuleOperation::Create(rule.clone())],
                self.options.remove_outlook_rules,
            )
            .await?;
        let rules = self.client.get_inbox_rules().await?;
        rules
            .iter()
            .rev()
            .find(|(r, _)| same_rule(r, rule))
            .map(|(r, _)| r.id.clone())
            .ok_or_else(|| EwsError::Protocol("the created inbox rule is not listed".into()))
    }

    async fn update_rule(&self, rule: &ExchangeInboxRule) -> Result<(), EwsError> {
        let rules = self.client.get_inbox_rules().await?;
        let (current, raw) = rules
            .into_iter()
            .find(|(r, _)| r.id == rule.id)
            .ok_or_else(|| EwsError::NotFound(format!("inbox rule {}", rule.id)))?;
        let operation = if current.has_unsupported {
            let mut raw: Element = raw;
            match raw.child_mut("IsEnabled") {
                Some(enabled) => enabled.text = rule.enabled.to_string(),
                None => {
                    let mut enabled = Element::new("IsEnabled");
                    enabled.text = rule.enabled.to_string();
                    raw.children.push(enabled);
                }
            }
            RuleOperation::SetRaw(raw)
        } else {
            RuleOperation::Set(rule.clone())
        };
        self.client
            .update_inbox_rules(&[operation], self.options.remove_outlook_rules)
            .await
    }

    async fn delete_rule(&self, id: &str) -> Result<(), EwsError> {
        let rules = self.client.get_inbox_rules().await?;
        if !rules.iter().any(|(r, _)| r.id == id) {
            return Ok(());
        }
        match self
            .client
            .update_inbox_rules(
                &[RuleOperation::Delete(id.to_owned())],
                self.options.remove_outlook_rules,
            )
            .await
        {
            Err(EwsError::NotFound(_)) => Ok(()),
            other => other,
        }
    }

    // -- directory ------------------------------------------------------

    async fn all_rooms(&self, room_list: Option<&str>) -> Result<Vec<Room>, EwsError> {
        let rooms = match room_list {
            Some(list) => self.client.get_rooms(list).await?,
            None => {
                let mut out: Vec<Room> = Vec::new();
                for list in self.client.get_room_lists().await? {
                    for room in self.client.get_rooms(&list.address).await? {
                        if !out
                            .iter()
                            .any(|r| r.address.eq_ignore_ascii_case(&room.address))
                        {
                            out.push(room);
                        }
                    }
                }
                out
            }
        };
        self.rooms
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .extend(rooms.iter().map(|r| r.address.to_ascii_lowercase()));
        Ok(rooms)
    }

    async fn check(&self, address: &str) -> Result<Option<String>, EwsError> {
        let root = [FolderRef::distinguished("msgfolderroot", Some(address))];
        let mut answers = self.client.get_folders(Some(address), &root).await?;
        match answers.pop() {
            Some(Ok(_)) => {}
            Some(Err(e)) => return Err(e),
            None => return Err(EwsError::Protocol("GetFolder without an answer".into())),
        }
        Ok(self
            .client
            .resolve_name(address)
            .await
            .ok()
            .and_then(|(_, name)| name))
    }

    // -- PIM ------------------------------------------------------------

    async fn pim_folder_list(
        &self,
        mailbox: Option<&str>,
    ) -> Result<Vec<ExchangePimFolder>, EwsError> {
        let role_of = |known: Option<ExchangeWellKnownFolder>| match known {
            Some(ExchangeWellKnownFolder::Calendar) => {
                Some(ExchangeCollectionRole::DefaultCalendar)
            }
            Some(ExchangeWellKnownFolder::Contacts) => {
                Some(ExchangeCollectionRole::DefaultContacts)
            }
            Some(ExchangeWellKnownFolder::Tasks) => Some(ExchangeCollectionRole::DefaultTasks),
            _ => None,
        };
        let kind_of = |class| match class {
            ExchangeFolderClass::Calendar => Some(PimFolderKind::Calendar),
            ExchangeFolderClass::Contacts => Some(PimFolderKind::Contacts),
            ExchangeFolderClass::Tasks => Some(PimFolderKind::Tasks),
            _ => None,
        };
        let mut out = Vec::new();
        let full = async {
            let mut folders = Vec::new();
            let mut cursor: Option<String> = None;
            loop {
                match self.hierarchy(mailbox, cursor.as_deref()).await? {
                    HierarchyOutcome::Delta(delta) => {
                        folders.extend(delta.upserted);
                        if !delta.more {
                            return Ok(folders);
                        }
                        cursor = Some(delta.cursor);
                    }
                    HierarchyOutcome::CursorExpired => cursor = None,
                }
            }
        };
        match full.await {
            Ok(folders) => {
                for folder in folders {
                    if let Some(kind) = kind_of(folder.class) {
                        out.push(ExchangePimFolder {
                            remote_id: folder.remote_id,
                            change_key: folder.change_key,
                            name: folder.name,
                            kind,
                            role: role_of(folder.well_known),
                            todo: false,
                            read_only: false,
                            color: None,
                        });
                    }
                }
            }
            // A delegate may see only some folders of another mailbox.
            Err(denied @ EwsError::Forbidden(_)) if mailbox.is_some() => {
                let names = ["calendar", "contacts", "tasks"];
                let refs: Vec<FolderRef<'_>> = names
                    .iter()
                    .map(|n| FolderRef::distinguished(n, mailbox))
                    .collect();
                for (name, answer) in names
                    .iter()
                    .zip(self.client.get_folders(mailbox, &refs).await?)
                {
                    let Ok(element) = answer else { continue };
                    let known = ExchangeWellKnownFolder::parse(name);
                    let lookup = |_: &str| known;
                    if let Some(folder) = exchange_folder(&element, None, &lookup)
                        && let Some(kind) = kind_of(folder.class).or(match *name {
                            "calendar" => Some(PimFolderKind::Calendar),
                            "contacts" => Some(PimFolderKind::Contacts),
                            _ => Some(PimFolderKind::Tasks),
                        })
                    {
                        out.push(ExchangePimFolder {
                            remote_id: folder.remote_id,
                            change_key: folder.change_key,
                            name: folder.name,
                            kind,
                            role: role_of(known),
                            todo: false,
                            read_only: false,
                            color: None,
                        });
                    }
                }
                if out.is_empty() {
                    return Err(denied);
                }
            }
            Err(e) => return Err(e),
        }
        Ok(out)
    }

    async fn read_events(
        &self,
        mailbox: Option<&str>,
        ids: &[ItemRef],
    ) -> Result<Vec<ExchangePimItem>, EwsError> {
        let shape = calendar::shape(&EVENT_FIELDS);
        let mut out = Vec::new();
        for item in self.client.get_items(mailbox, ids, &shape).await? {
            let item = match item {
                Ok(item) => item,
                Err(EwsError::NotFound(_)) => continue,
                Err(e) => return Err(e),
            };
            let Some(mut event) = parse_event(&item) else {
                continue;
            };
            let occurrences = modified_occurrences(&item);
            if !occurrences.is_empty() {
                for exception in self.client.get_items(mailbox, &occurrences, &shape).await? {
                    let Ok(exception) = exception else { continue };
                    if let Some(mut parsed) = parse_event(&exception) {
                        parsed.series_master_id = Some(event.remote_id.clone());
                        event.exceptions.push(parsed);
                    }
                }
            }
            out.push(ExchangePimItem::Event(Box::new(event)));
        }
        Ok(out)
    }

    async fn pim_delta(
        &self,
        mailbox: Option<&str>,
        folder: &ExchangePimFolder,
        cursor: Option<&str>,
    ) -> Result<PimDeltaOutcome, EwsError> {
        let id_shape = "<m:ItemShape><t:BaseShape>IdOnly</t:BaseShape></m:ItemShape>";
        let SyncOutcome::Changes(changes) = self
            .client
            .sync_folder_items(
                mailbox,
                FolderRef::named(mailbox, &folder.remote_id),
                cursor,
                id_shape,
                self.options.page_size,
            )
            .await?
        else {
            return Ok(PimDeltaOutcome::CursorExpired);
        };
        let ids: Vec<ItemRef> = changes
            .created
            .iter()
            .chain(&changes.updated)
            .filter_map(|i| i.child("ItemId").and_then(ItemRef::from_element))
            .map(|r| ItemRef::new(r.id))
            .collect();
        let upserted = match folder.kind {
            PimFolderKind::Calendar => self.read_events(mailbox, &ids).await?,
            PimFolderKind::Tasks => {
                let shape = calendar::shape(&TASK_FIELDS);
                self.read_with(mailbox, &ids, &shape, |e| {
                    parse_task(e).map(|t| ExchangePimItem::Task(Box::new(t)))
                })
                .await?
            }
            PimFolderKind::Contacts => {
                self.read_with(mailbox, &ids, CONTACT_SHAPE, |e| {
                    parse_contact(e).map(|c| ExchangePimItem::Contact(Box::new(c)))
                })
                .await?
            }
        };
        Ok(PimDeltaOutcome::Delta(PimDelta {
            cursor: changes.sync_state,
            upserted,
            removed: changes.deleted,
            more: !changes.complete,
        }))
    }

    async fn read_with(
        &self,
        mailbox: Option<&str>,
        ids: &[ItemRef],
        shape: &str,
        parse: impl Fn(&Element) -> Option<ExchangePimItem>,
    ) -> Result<Vec<ExchangePimItem>, EwsError> {
        let mut out = Vec::new();
        for item in self.client.get_items(mailbox, ids, shape).await? {
            match item {
                Ok(item) => out.extend(parse(&item)),
                Err(EwsError::NotFound(_)) => {}
                Err(e) => return Err(e),
            }
        }
        Ok(out)
    }

    async fn pim_write(
        &self,
        mailbox: Option<&str>,
        write: &ExchangePimWrite,
    ) -> Result<ExchangePimWriteOutcome, EwsError> {
        match write {
            ExchangePimWrite::Create { folder, item, send } => {
                let (xml, zone, attributes) = match item {
                    ExchangePimItem::Event(event) => (
                        event_create_xml(event)?,
                        write_zone(event)?,
                        format!(r#" SendMeetingInvitations="{}""#, create_send(*send)),
                    ),
                    ExchangePimItem::Task(task) => {
                        (task_create_xml(task)?, task_zone(task)?, String::new())
                    }
                    ExchangePimItem::Contact(contact) => {
                        (contact_create_xml(contact), None, String::new())
                    }
                };
                let body = format!(
                    r#"<m:CreateItem{attributes}><m:SavedItemFolderId>{}</m:SavedItemFolderId><m:Items>{xml}</m:Items></m:CreateItem>"#,
                    FolderRef::named(mailbox, folder).xml()
                );
                let response = self
                    .client
                    .call(
                        &Operation::write("CreateItem", body)
                            .anchored(mailbox)
                            .in_zone(zone.as_deref()),
                    )
                    .await?;
                let message = soap::single_message(&response.body)?;
                let created = mail::first_item(message)?
                    .child("ItemId")
                    .and_then(ItemRef::from_element)
                    .ok_or_else(|| EwsError::Protocol("CreateItem without an item id".into()))?;
                if let ExchangePimItem::Event(event) = item
                    && (!event.exceptions.is_empty() || !event.cancelled_occurrences.is_empty())
                {
                    let key = self
                        .apply_occurrences(mailbox, Some(folder), &created.id, event, *send)
                        .await?;
                    return Ok(ExchangePimWriteOutcome::Done {
                        remote_id: created.id,
                        change_key: key.or(created.change_key),
                    });
                }
                Ok(ExchangePimWriteOutcome::Done {
                    remote_id: created.id,
                    change_key: created.change_key,
                })
            }
            ExchangePimWrite::Update { item, send } => {
                let reference =
                    ItemRef::with_key(item.remote_id(), item.change_key().map(str::to_owned));
                let (tag, updates, zone, attributes) = match item {
                    ExchangePimItem::Event(event) => (
                        "CalendarItem",
                        event_updates(
                            event,
                            event.kind != nail_api::exchange::pim::EventKind::SeriesMaster
                                && event.kind != nail_api::exchange::pim::EventKind::SingleInstance,
                        )?,
                        write_zone(event)?,
                        format!(
                            r#" SendMeetingInvitationsOrCancellations="{}""#,
                            update_send(*send)
                        ),
                    ),
                    ExchangePimItem::Task(task) => {
                        ("Task", task_updates(task)?, task_zone(task)?, String::new())
                    }
                    ExchangePimItem::Contact(contact) => (
                        contact_tag(contact),
                        contact_updates(contact),
                        None,
                        String::new(),
                    ),
                };
                let result = self
                    .update_in_zone(
                        mailbox,
                        &reference,
                        tag,
                        &updates,
                        &attributes,
                        zone.as_deref(),
                    )
                    .await;
                let updated = match result {
                    Ok(updated) => updated,
                    Err(EwsError::Conflict(_)) => return Ok(ExchangePimWriteOutcome::Conflict),
                    Err(EwsError::NotFound(_)) => return Ok(ExchangePimWriteOutcome::Gone),
                    Err(e) => return Err(e),
                };
                if let ExchangePimItem::Event(event) = item
                    && (!event.exceptions.is_empty() || !event.cancelled_occurrences.is_empty())
                {
                    let key = self
                        .apply_occurrences(mailbox, None, &updated.id, event, *send)
                        .await?;
                    return Ok(ExchangePimWriteOutcome::Done {
                        remote_id: updated.id,
                        change_key: key.or(updated.change_key),
                    });
                }
                Ok(ExchangePimWriteOutcome::Done {
                    remote_id: updated.id,
                    change_key: updated.change_key,
                })
            }
            ExchangePimWrite::Delete { remote_id, send } => {
                let attributes = format!(
                    r#" SendMeetingCancellations="{}" AffectedTaskOccurrences="AllOccurrences""#,
                    delete_send(*send)
                );
                match self
                    .client
                    .delete_item(
                        mailbox,
                        &ItemRef::new(remote_id.clone()),
                        "MoveToDeletedItems",
                        &attributes,
                    )
                    .await
                {
                    Ok(()) => Ok(ExchangePimWriteOutcome::Done {
                        remote_id: remote_id.clone(),
                        change_key: None,
                    }),
                    Err(EwsError::NotFound(_)) => Ok(ExchangePimWriteOutcome::Gone),
                    Err(e) => Err(e),
                }
            }
        }
    }

    async fn update_in_zone(
        &self,
        mailbox: Option<&str>,
        item: &ItemRef,
        tag: &str,
        updates: &[Update],
        attributes: &str,
        zone: Option<&str>,
    ) -> Result<ItemRef, EwsError> {
        if zone.is_none() {
            return self
                .client
                .update_item(mailbox, item, tag, updates, attributes)
                .await;
        }
        let conflict = if item.change_key.is_some() {
            "NeverOverwrite"
        } else {
            "AlwaysOverwrite"
        };
        let changes: String = updates.iter().map(|u| u.xml(tag)).collect();
        let body = format!(
            r#"<m:UpdateItem ConflictResolution="{conflict}" MessageDisposition="SaveOnly"{attributes}><m:ItemChanges><t:ItemChange>{}<t:Updates>{changes}</t:Updates></t:ItemChange></m:ItemChanges></m:UpdateItem>"#,
            item.xml()
        );
        let response = self
            .client
            .call(
                &Operation::write("UpdateItem", body)
                    .anchored(mailbox)
                    .in_zone(zone),
            )
            .await?;
        let message = soap::single_message(&response.body)?;
        Ok(message
            .child("Items")
            .and_then(|items| items.children.first())
            .and_then(|i| i.child("ItemId"))
            .and_then(ItemRef::from_element)
            .unwrap_or_else(|| item.clone()))
    }

    /// Writes a series' exceptions and cancelled occurrences that the
    /// server does not have yet; answers the master's change key after.
    async fn apply_occurrences(
        &self,
        mailbox: Option<&str>,
        folder: Option<&str>,
        master_id: &str,
        event: &ExchangeEvent,
        send: MeetingSend,
    ) -> Result<Option<String>, EwsError> {
        let shape = calendar::shape(&EVENT_FIELDS);
        let master = self
            .client
            .get_item(mailbox, &ItemRef::new(master_id), &shape)
            .await?;
        let Some(server) = parse_event(&master) else {
            return Ok(None);
        };
        let Some(uid) = server.uid.clone() else {
            return Ok(server.change_key);
        };
        let bias = master
            .child("StartTimeZone")
            .and_then(|z| z.child("Periods"))
            .and_then(|p| {
                p.children
                    .iter()
                    .find(|x| x.attr("Name") == Some("Standard"))
                    .and_then(|x| x.attr("Bias"))
            })
            .and_then(calendar::parse_bias)
            .unwrap_or(0);
        let parent = master
            .child("ParentFolderId")
            .and_then(|p| p.attr("Id"))
            .map(str::to_owned)
            .or_else(|| folder.map(str::to_owned))
            .unwrap_or_else(|| "calendar".to_owned());
        let parent_ref = FolderRef::named(mailbox, &parent);
        let attributes = format!(
            r#" SendMeetingInvitationsOrCancellations="{}""#,
            update_send(send)
        );
        for date in &event.cancelled_occurrences {
            if server.cancelled_occurrences.contains(date) {
                continue;
            }
            if let Some(occurrence) = self
                .client
                .find_occurrence(mailbox, parent_ref, &uid, date, bias)
                .await?
            {
                let attrs = format!(r#" SendMeetingCancellations="{}""#, delete_send(send));
                match self
                    .client
                    .delete_item(
                        mailbox,
                        &ItemRef::new(occurrence.remote_id),
                        "MoveToDeletedItems",
                        &attrs,
                    )
                    .await
                {
                    Ok(()) | Err(EwsError::NotFound(_)) => {}
                    Err(e) => return Err(e),
                }
            }
        }
        for exception in &event.exceptions {
            let date = exception.original_start.as_ref().map(|s| {
                if s.time_zone == "UTC" {
                    calendar::local_date(&format!("{}Z", s.date_time), bias)
                        .map(|d| d.format("%Y-%m-%d").to_string())
                        .unwrap_or_else(|| s.date().to_owned())
                } else {
                    s.date().to_owned()
                }
            });
            let target = if server
                .exceptions
                .iter()
                .any(|e| e.remote_id == exception.remote_id)
                || (!exception.remote_id.is_empty()
                    && modified_occurrences(&master)
                        .iter()
                        .any(|o| o.id == exception.remote_id))
            {
                Some(ItemRef::new(exception.remote_id.clone()))
            } else if let Some(date) = date {
                self.client
                    .find_occurrence(mailbox, parent_ref, &uid, &date, bias)
                    .await?
                    .map(|o| ItemRef::new(o.remote_id))
            } else {
                None
            };
            let Some(target) = target else { continue };
            let updates = event_updates(exception, true)?;
            let zone = write_zone(exception)?;
            match self
                .update_in_zone(
                    mailbox,
                    &target,
                    "CalendarItem",
                    &updates,
                    &attributes,
                    zone.as_deref(),
                )
                .await
            {
                Ok(_) | Err(EwsError::NotFound(_)) => {}
                Err(e) => return Err(e),
            }
        }
        let id_shape = "<m:ItemShape><t:BaseShape>IdOnly</t:BaseShape></m:ItemShape>";
        let after = self
            .client
            .get_item(mailbox, &ItemRef::new(master_id), id_shape)
            .await?;
        Ok(after
            .child("ItemId")
            .and_then(ItemRef::from_element)
            .and_then(|r| r.change_key))
    }

    async fn free_busy_of(&self, query: &FreeBusyQuery) -> Result<Vec<AttendeeFreeBusy>, EwsError> {
        let rooms: Vec<String> = self
            .rooms
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .cloned()
            .collect();
        let answers = self
            .client
            .get_availability(&query.attendees, &rooms, &query.from, &query.to)
            .await?;
        Ok(answers
            .into_iter()
            .filter_map(|a| {
                let source = FreeBusySource::Exchange {
                    account_id: self.account_id.clone(),
                };
                match a.result {
                    Ok(periods) => Some(AttendeeFreeBusy {
                        email: a.address,
                        source,
                        periods,
                        error: None,
                        localized: None,
                    }),
                    // Unknown to this server: another source may know them.
                    Err(EwsError::NotFound(_)) => None,
                    Err(e) => Some(AttendeeFreeBusy {
                        email: a.address,
                        source,
                        periods: Vec::new(),
                        error: Some(e.code()),
                        localized: Some(Box::new(e.localized())),
                    }),
                }
            })
            .collect())
    }

    async fn send_message(
        &self,
        envelope: &Envelope,
        message: &[u8],
    ) -> Result<SendReceipt, EwsError> {
        if envelope.to.is_empty() {
            return Err(EwsError::Rejected {
                code: "NoRecipients".into(),
                message: "a message needs at least one recipient".into(),
            });
        }
        let raw = with_envelope_recipients(message, &envelope.to)?;
        let sending = self.sending_mailbox(&raw);
        self.client
            .send_mime_from(&raw, self.options.save_sent_copy, &sending)
            .await?;
        Ok(SendReceipt {
            response: format!(
                "EWS CreateItem {}: NoError",
                if self.options.save_sent_copy {
                    "SendAndSaveCopy"
                } else {
                    "SendOnly"
                }
            ),
        })
    }
}

impl EwsEngine {
    /// Who a message is sent as ([`SendingMailbox::of`]: its `From`, its
    /// `Sender` and the account's address).
    pub fn sending_mailbox(&self, raw: &[u8]) -> SendingMailbox {
        let header = |name: &str| mail::header_addresses(raw, name).into_iter().next();
        let (Some(from), Some(own)) = (header("from"), self.client.own_address()) else {
            return SendingMailbox::Own;
        };
        SendingMailbox::of(&from, header("sender").as_deref(), own)
    }

    /// The headers of a message without its body, for headers-only sync
    /// policies: the transport headers (`PR_TRANSPORT_MESSAGE_HEADERS`), or
    /// for messages without them (sent mail, drafts) the MIME cut at the
    /// end of its headers. Ends with the empty line.
    pub async fn fetch_headers(
        &self,
        mailbox: Option<&str>,
        remote_id: &str,
    ) -> Result<Vec<u8>, EwsError> {
        if let Some(headers) = self.client.transport_headers(mailbox, remote_id).await? {
            return Ok(mail::header_block(headers.as_bytes()));
        }
        let raw = self.client.get_mime(mailbox, remote_id).await?;
        Ok(mail::header_block(&raw))
    }

    /// Moves a message to a folder of another mailbox (`None` = the
    /// user's own): `MoveItem` across mailboxes where the server allows it,
    /// else (`ErrorCrossMailboxMoveCopy`) a MIME copy in the target with
    /// the flags and a hard delete at the source. Answers the new id.
    pub async fn move_to_mailbox(
        &self,
        from_mailbox: Option<&str>,
        remote_id: &str,
        to_mailbox: Option<&str>,
        to_folder: &str,
        flags: nail_api::domain::Flags,
    ) -> Result<String, EwsError> {
        let target = FolderRef::named(to_mailbox, to_folder);
        match self
            .client
            .move_item(from_mailbox, remote_id, target, false)
            .await
        {
            Ok(moved) => return Ok(moved.id),
            Err(EwsError::Rejected { code, .. }) if code == "ErrorCrossMailboxMoveCopy" => {
                tracing::debug!("no cross-mailbox MoveItem; copying the MIME");
            }
            Err(e) => return Err(e),
        }
        let raw = self.client.get_mime(from_mailbox, remote_id).await?;
        let created = self
            .client
            .create_mime_item(to_mailbox, target, &raw, flags)
            .await?;
        match self
            .client
            .delete_item(from_mailbox, &ItemRef::new(remote_id), "HardDelete", "")
            .await
        {
            Ok(()) | Err(EwsError::NotFound(_)) => Ok(created.id),
            Err(e) => Err(e),
        }
    }
}

fn engine<T>(result: Result<T, EwsError>) -> Result<T, EngineError> {
    result.map_err(EngineError::from)
}

impl ExchangeMailEngine for EwsEngine {
    fn server_info(&self) -> BoxFuture<'_, Result<ExchangeServerInfo, EngineError>> {
        Box::pin(async move {
            // A live call (not the cached folder list): this also checks
            // the sign-in.
            let root = [FolderRef::distinguished("msgfolderroot", None)];
            let mut answers = engine(self.client.get_folders(None, &root).await)?;
            if let Some(Err(e)) = answers.pop() {
                return Err(e.into());
            }
            let address = self.client.own_address().unwrap_or_default().to_owned();
            let display_name = if address.is_empty() {
                None
            } else {
                self.client
                    .resolve_name(&address)
                    .await
                    .ok()
                    .and_then(|(_, n)| n)
            };
            let server_version = self.client.server_version().map(|v| match v.product() {
                Some(product) => format!("{product} ({v})"),
                None => v.to_string(),
            });
            Ok(ExchangeServerInfo {
                address,
                display_name,
                server_version,
                time_zone: None,
                focused_inbox: self.options.focused_inbox,
            })
        })
    }

    fn folder_hierarchy<'a>(
        &'a self,
        mailbox: Option<&'a str>,
        cursor: Option<&'a str>,
    ) -> BoxFuture<'a, Result<HierarchyOutcome, EngineError>> {
        Box::pin(async move { engine(self.hierarchy(mailbox, cursor).await) })
    }

    fn folder_items<'a>(
        &'a self,
        mailbox: Option<&'a str>,
        folder: &'a str,
        cursor: Option<&'a str>,
        since: Option<i64>,
    ) -> BoxFuture<'a, Result<DeltaOutcome, EngineError>> {
        Box::pin(async move { engine(self.items(mailbox, folder, cursor, since).await) })
    }

    fn fetch_mime<'a>(
        &'a self,
        mailbox: Option<&'a str>,
        remote_id: &'a str,
    ) -> BoxFuture<'a, Result<Vec<u8>, EngineError>> {
        Box::pin(async move { engine(self.client.get_mime(mailbox, remote_id).await) })
    }

    fn write<'a>(
        &'a self,
        mailbox: Option<&'a str>,
        write: &'a RemoteWrite,
    ) -> BoxFuture<'a, Result<WriteOutcome, EngineError>> {
        Box::pin(async move { engine(self.replay(mailbox, write).await) })
    }

    fn classify<'a>(
        &'a self,
        mailbox: Option<&'a str>,
        remote_ids: &'a [String],
        classification: InferenceClassification,
        always: bool,
    ) -> BoxFuture<'a, Result<(), EngineError>> {
        Box::pin(async move {
            engine(
                self.classify_items(mailbox, remote_ids, classification, always)
                    .await,
            )
        })
    }

    fn wait_for_changes<'a>(
        &'a self,
        mailbox: Option<&'a str>,
        folders: &'a [String],
        timeout_secs: u32,
    ) -> BoxFuture<'a, Result<Option<Vec<String>>, EngineError>> {
        Box::pin(async move {
            if !self.options.streaming {
                return Ok(None);
            }
            let changed = engine(
                self.watcher
                    .wait(
                        &self.client,
                        mailbox,
                        folders,
                        Duration::from_secs(u64::from(timeout_secs)),
                    )
                    .await,
            )?;
            Ok(Some(changed))
        })
    }
}

impl ExchangeSettingsEngine for EwsEngine {
    fn categories(&self) -> BoxFuture<'_, Result<Vec<RemoteCategory>, EngineError>> {
        Box::pin(async move {
            let (root, _) = engine(self.client.get_category_list().await)?;
            Ok(parse_category_list(&root))
        })
    }

    fn create_category<'a>(
        &'a self,
        settings: &'a ExchangeCategorySettings,
    ) -> BoxFuture<'a, Result<String, EngineError>> {
        Box::pin(async move {
            let id = new_guid();
            engine(
                self.client
                    .edit_categories(&CategoryEdit::Create(&id, settings))
                    .await,
            )?;
            Ok(id)
        })
    }

    fn update_category<'a>(
        &'a self,
        id: &'a str,
        settings: &'a ExchangeCategorySettings,
    ) -> BoxFuture<'a, Result<Option<String>, EngineError>> {
        // The CategoryList keeps the GUID across renames.
        Box::pin(async move {
            engine(
                self.client
                    .edit_categories(&CategoryEdit::Update(id, settings))
                    .await,
            )
            .map(|()| None)
        })
    }

    fn delete_category<'a>(&'a self, id: &'a str) -> BoxFuture<'a, Result<(), EngineError>> {
        Box::pin(
            async move { engine(self.client.edit_categories(&CategoryEdit::Delete(id)).await) },
        )
    }

    fn automatic_replies(&self) -> BoxFuture<'_, Result<AutomaticRepliesSettings, EngineError>> {
        Box::pin(async move { engine(self.client.get_oof().await) })
    }

    fn set_automatic_replies<'a>(
        &'a self,
        settings: &'a AutomaticRepliesSettings,
    ) -> BoxFuture<'a, Result<(), EngineError>> {
        Box::pin(async move { engine(self.client.set_oof(settings).await) })
    }

    fn inbox_rules(&self) -> BoxFuture<'_, Result<Vec<ExchangeInboxRule>, EngineError>> {
        Box::pin(async move {
            Ok(engine(self.client.get_inbox_rules().await)?
                .into_iter()
                .map(|(rule, _)| rule)
                .collect())
        })
    }

    fn create_inbox_rule<'a>(
        &'a self,
        rule: &'a ExchangeInboxRule,
    ) -> BoxFuture<'a, Result<String, EngineError>> {
        Box::pin(async move { engine(self.create_rule(rule).await) })
    }

    fn update_inbox_rule<'a>(
        &'a self,
        rule: &'a ExchangeInboxRule,
    ) -> BoxFuture<'a, Result<(), EngineError>> {
        Box::pin(async move { engine(self.update_rule(rule).await) })
    }

    fn delete_inbox_rule<'a>(&'a self, id: &'a str) -> BoxFuture<'a, Result<(), EngineError>> {
        Box::pin(async move { engine(self.delete_rule(id).await) })
    }
}

impl ExchangeDirectoryEngine for EwsEngine {
    fn room_lists(&self) -> BoxFuture<'_, Result<Vec<RoomList>, EngineError>> {
        Box::pin(async move { engine(self.client.get_room_lists().await) })
    }

    fn rooms<'a>(
        &'a self,
        room_list: Option<&'a str>,
    ) -> BoxFuture<'a, Result<Vec<Room>, EngineError>> {
        Box::pin(async move { engine(self.all_rooms(room_list).await) })
    }

    fn check_mailbox<'a>(
        &'a self,
        address: &'a str,
    ) -> BoxFuture<'a, Result<Option<String>, EngineError>> {
        Box::pin(async move { engine(self.check(address).await) })
    }
}

impl ExchangePimEngine for EwsEngine {
    fn pim_folders<'a>(
        &'a self,
        mailbox: Option<&'a str>,
    ) -> BoxFuture<'a, Result<Vec<ExchangePimFolder>, EngineError>> {
        Box::pin(async move { engine(self.pim_folder_list(mailbox).await) })
    }

    fn pim_changes<'a>(
        &'a self,
        mailbox: Option<&'a str>,
        folder: &'a ExchangePimFolder,
        cursor: Option<&'a str>,
    ) -> BoxFuture<'a, Result<PimDeltaOutcome, EngineError>> {
        Box::pin(async move { engine(self.pim_delta(mailbox, folder, cursor).await) })
    }

    fn write_pim<'a>(
        &'a self,
        mailbox: Option<&'a str>,
        write: &'a ExchangePimWrite,
    ) -> BoxFuture<'a, Result<ExchangePimWriteOutcome, EngineError>> {
        Box::pin(async move { engine(self.pim_write(mailbox, write).await) })
    }

    fn respond_to_meeting<'a>(
        &'a self,
        mailbox: Option<&'a str>,
        event_remote_id: &'a str,
        response: InvitationResponse,
        comment: Option<&'a str>,
        send: bool,
    ) -> BoxFuture<'a, Result<(), EngineError>> {
        Box::pin(async move {
            engine(
                self.client
                    .respond_to_meeting(mailbox, event_remote_id, response, comment, send)
                    .await,
            )
        })
    }
}

impl FreeBusyProvider for EwsEngine {
    fn free_busy<'a>(
        &'a self,
        query: &'a FreeBusyQuery,
    ) -> BoxFuture<'a, Result<Vec<AttendeeFreeBusy>, EngineError>> {
        Box::pin(async move { engine(self.free_busy_of(query).await) })
    }
}

impl OutgoingTransport for EwsEngine {
    fn send<'a>(
        &'a self,
        envelope: &'a Envelope,
        message: &'a [u8],
    ) -> BoxFuture<'a, Result<SendReceipt, EngineError>> {
        Box::pin(async move { engine(self.send_message(envelope, message).await) })
    }
}

impl ExchangeEngine for EwsEngine {
    fn kind(&self) -> EngineKind {
        EngineKind::Ews
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nail_api::accounts::{SecretRef, ServerEndpoint, TlsMode};
    use nail_api::engine::MemorySecretStore;
    use nail_api::exchange::ExchangeOptions;

    #[derive(Debug)]
    struct NoTokens;

    impl AccessTokenProvider for NoTokens {
        fn access_token<'a>(
            &'a self,
            _provider: &'a str,
            _secret: &'a SecretRef,
            _force: bool,
        ) -> BoxFuture<'a, Result<String, EngineError>> {
            Box::pin(async { Ok("token".to_owned()) })
        }
    }

    fn account(auth: AuthConfig, engine: EngineKind) -> AccountConfig {
        let mut account = AccountConfig {
            id: "work".into(),
            name: "Work".into(),
            email: "ada@corp.example".into(),
            enabled: true,
            incoming: IncomingServer::Exchange {
                server: ServerEndpoint {
                    host: String::new(),
                    port: 443,
                    tls: TlsMode::Implicit,
                    auth: AuthConfig::None,
                },
                options: ExchangeOptions::default(),
            },
            outgoing: None,
            identities: vec![],
            sync: nail_api::accounts::SyncPolicy::default(),
            save_sent_copy: None,
            authserv_ids: Vec::new(),
            sieve: None,
        };
        account.incoming = IncomingServer::Exchange {
            server: ServerEndpoint {
                host: "mail.corp.example".into(),
                port: 443,
                tls: TlsMode::Implicit,
                auth,
            },
            options: ExchangeOptions {
                streaming: Some(false),
                ..ExchangeOptions::for_engine(engine)
            },
        };
        account
    }

    #[test]
    fn engines_for_accounts() {
        let secrets: Arc<dyn SecretStore> = Arc::new(MemorySecretStore::new());
        let tokens: Arc<dyn AccessTokenProvider> = Arc::new(NoTokens);
        let basic = account(
            AuthConfig::Password {
                username: "CORP\\ada".into(),
                secret: None,
                mechanism: None,
            },
            EngineKind::Ews,
        );
        let engine = EwsEngine::for_account(&basic, secrets.clone(), tokens.clone(), None).unwrap();
        assert_eq!(engine.kind(), EngineKind::Ews);
        assert_eq!(
            engine.client().url().as_str(),
            "https://mail.corp.example/EWS/Exchange.asmx"
        );
        assert_eq!(engine.client().own_address(), Some("ada@corp.example"));
        assert!(!engine.options().streaming);
        assert!(matches!(engine.client().auth(), EwsAuth::Basic { .. }));
        let windows = account(
            AuthConfig::Windows {
                username: None,
                secret: None,
                package: None,
            },
            EngineKind::Ews,
        );
        assert!(matches!(
            EwsEngine::for_account(&windows, secrets.clone(), tokens.clone(), None),
            Err(EwsError::Config(_))
        ));
        let graph = account(AuthConfig::None, EngineKind::Graph);
        assert!(matches!(
            EwsEngine::for_account(&graph, secrets, tokens, None),
            Err(EwsError::Config(_))
        ));
    }
}
