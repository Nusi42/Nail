//! Backups and restores (ADRs 0094, 0096, 0099): the core's
//! [`BackupSource`] (what goes into a `.nailbackup`) and [`RestoreTarget`]
//! (where a restore writes).
//!
//! Files per item (relative to the item's archive folder):
//!
//! | Item | Files |
//! | --- | --- |
//! | settings | `settings.json` — the app settings by key, without machine-bound ones ([`EXCLUDED_SETTINGS`]); tokens, remote-UI users, MCP clients and pairing codes live in their own tables and never go in |
//! | theme / layout / accounts / rules | `theme.kdl`, `layout.kdl`, `accounts.kdl` (`keychain:` references only), `rules.kdl` + `saved-searches.json` |
//! | keys | `keyring.json` — every key with its public part, trust decision and secret part as stored (passphrase-protected or wrapped; the keychain secret of a wrapped key goes along, so the key works after a restore on another machine) |
//! | plugins | `<plugin id>/package.nailplugin` — the approved package |
//!
//! Restores replace configuration files through the configuration manager
//! (the old file is kept as a configuration backup, revertible with
//! `RevertConfig`), settings key by key; keys are merged by fingerprint;
//! plug-ins that are not installed are installed **disabled without
//! permissions** (the user reviews them); mail and PIM data are merged.

use std::collections::BTreeMap;
use std::io::Write;

use base64::Engine as _;
use nail_api::backup::BackupItem;
use nail_api::customize::ConfigFile;
use nail_api::portability::LOCAL_FOLDERS_ACCOUNT_ID;
use nail_api::security::{CryptoTechnology, KeySource, SecretStorage, TrustDecision};
use nail_api::{AccountId, AccountKind, ConfigBackupId, PluginId, SecretRef};
use nail_portability::SinkError;
use nail_portability::backup::{BackupFile, BackupInfo, BackupSource, MergeOutcome, RestoreTarget};
use nail_portability::sink::{
    DuplicateKey, ImportedMessage, MailExportSource, MailImportSink, PimExportSource,
    PimImportSink, SourceCollection, SourceFolder,
};
use nail_store::{KeyData, SecretKeyData};
use serde::{Deserialize, Serialize};

use super::mail::{ImportTarget, StoreMailSource};
use super::pim::{PimBuffer, PimMode, StorePimSource};
use crate::service::Service;

/// Settings that belong to this machine / installation and are neither
/// backed up nor restored: the remote-UI certificate and first-run state,
/// and the machine-bound security settings — PKCS#11 module paths (DLLs
/// loaded into the process), the remote-UI listener and MCP access. Those
/// are set up again on the new machine through their validated setters.
pub(crate) const EXCLUDED_SETTINGS: &[&str] = &[
    nail_store::REMOTE_UI_CERTIFICATE_KEY,
    crate::desktop::FIRST_RUN_KEY,
    crate::smartcard::SMARTCARD_SETTINGS_KEY,
    nail_store::REMOTE_UI_SETTINGS_KEY,
    nail_store::MCP_SETTINGS_KEY,
];

/// Checks a restored setting the way its setter would: known keys must
/// parse as their type and pass its checks. `Err` = why it is not
/// restored. Unknown keys (sync state, caches) are kept as they are.
fn check_restored_setting(key: &str, value: &serde_json::Value) -> Result<(), String> {
    fn parse<T: serde::de::DeserializeOwned>(value: &serde_json::Value) -> Result<T, String> {
        T::deserialize(value).map_err(|e| e.to_string())
    }
    match key {
        k if EXCLUDED_SETTINGS.contains(&k) => Err("bound to the machine".to_owned()),
        nail_store::SECURITY_SETTINGS_KEY => {
            parse::<nail_api::security::SecuritySettings>(value).map(drop)
        }
        nail_store::scheduling::SCHEDULING_SETTINGS_KEY => {
            parse::<nail_api::scheduling::SchedulingSettings>(value).map(drop)
        }
        nail_store::NOTIFICATION_SETTINGS_KEY => {
            nail_store::check_stored_notification_settings(value)
        }
        crate::desktop::UPDATE_SETTINGS_KEY => {
            parse::<nail_api::desktop::UpdateSettings>(value).map(drop)
        }
        crate::desktop::DESKTOP_SETTINGS_KEY => {
            parse::<nail_api::desktop::DesktopSettings>(value).map(drop)
        }
        nail_api::palette::RECENT_ACTIONS_KEY => {
            parse::<nail_api::palette::RecentActions>(value).map(drop)
        }
        _ => Ok(()),
    }
}

const SETTINGS_FILE: &str = "settings.json";
const KEYRING_FILE: &str = "keyring.json";
const SAVED_SEARCHES_FILE: &str = "saved-searches.json";
const PACKAGE_FILE: &str = "package.nailplugin";

fn item_err(err: impl std::fmt::Display) -> SinkError {
    SinkError::Item(err.to_string())
}

fn fatal(err: impl std::fmt::Display) -> SinkError {
    SinkError::Fatal(err.to_string())
}

fn config_file(item: BackupItem, name: &str) -> Option<ConfigFile> {
    match (item, name) {
        (BackupItem::Theme, "theme.kdl") => Some(ConfigFile::Theme),
        (BackupItem::Layout, "layout.kdl") => Some(ConfigFile::Layout),
        (BackupItem::Accounts, "accounts.kdl") => Some(ConfigFile::Accounts),
        (BackupItem::Rules, "rules.kdl") => Some(ConfigFile::Rules),
        _ => None,
    }
}

/// A key as backed up.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct BackupKey {
    technology: CryptoTechnology,
    fingerprint: String,
    user_ids: Vec<String>,
    emails: Vec<String>,
    algorithm: String,
    created_at: i64,
    expires_at: Option<i64>,
    revoked: bool,
    can_sign: bool,
    can_encrypt: bool,
    source: KeySource,
    /// Base64.
    public_data: String,
    issuer: Option<String>,
    serial: Option<String>,
    decision: Option<TrustDecision>,
    secret: Option<BackupSecret>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct BackupSecret {
    storage: SecretStorage,
    /// Base64 of the encrypted secret material.
    data: Option<String>,
    reference: Option<String>,
    /// The keychain secret behind `reference` (wrapped keys), base64.
    keychain_secret: Option<String>,
}

impl BackupKey {
    /// The only keychain reference a restored key may name: its own
    /// wrapping entry (`keychain:nail/openpgp/<fpr>` or
    /// `keychain:nail/smime-key/<fpr>`). Anything else (an account
    /// password, the vault's data key) is refused, so a crafted backup
    /// cannot overwrite or point at other keychain entries.
    fn check_reference(&self) -> Result<(), String> {
        let Some(secret) = &self.secret else {
            return Ok(());
        };
        let own = match self.technology {
            CryptoTechnology::OpenPgp => nail_pgp::keyring::keychain_ref(&self.fingerprint),
            CryptoTechnology::Smime => {
                nail_smime::keyring::keychain_ref(&self.fingerprint).map_err(|e| e.to_string())?
            }
        };
        let names_keychain = secret
            .reference
            .as_deref()
            .is_some_and(|r| r.starts_with(SecretRef::SCHEME));
        let is_own = secret.reference.as_deref() == Some(own.0.as_str());
        let keychain_storage =
            secret.storage == SecretStorage::Keychain && secret.reference.is_some();
        if !is_own && (names_keychain || keychain_storage || secret.keychain_secret.is_some()) {
            return Err(format!(
                "key {} names a keychain entry that is not its own",
                self.fingerprint
            ));
        }
        Ok(())
    }
}

/// The core as a backup's source (blocking).
pub(crate) struct CoreBackupSource {
    pub(crate) service: Service,
}

impl CoreBackupSource {
    fn settings(&self) -> Result<BTreeMap<String, serde_json::Value>, SinkError> {
        let mut all = self.service.store().all_settings().map_err(fatal)?;
        all.retain(|k, _| !EXCLUDED_SETTINGS.contains(&k.as_str()));
        Ok(all)
    }

    fn keyring(&self) -> Result<Vec<BackupKey>, SinkError> {
        let store = self.service.store();
        let secrets = self.service.secrets();
        let b64 = base64::engine::general_purpose::STANDARD;
        let mut keys: Vec<BackupKey> = store
            .list_keys(None, false)
            .map_err(fatal)?
            .into_iter()
            .map(|k| {
                let secret = k.data.secret.as_ref().map(|s| BackupSecret {
                    storage: s.storage,
                    data: s.data.as_ref().map(|d| b64.encode(d)),
                    reference: s.reference.clone(),
                    keychain_secret: s
                        .reference
                        .as_deref()
                        .filter(|r| r.starts_with("keychain:"))
                        .and_then(|r| secrets.get(&SecretRef(r.to_owned())).ok().flatten())
                        .map(|v| b64.encode(v.as_bytes())),
                });
                BackupKey {
                    technology: k.data.technology,
                    fingerprint: k.data.fingerprint,
                    user_ids: k.data.user_ids,
                    emails: k.data.emails,
                    algorithm: k.data.algorithm,
                    created_at: k.data.created_at,
                    expires_at: k.data.expires_at,
                    revoked: k.data.revoked,
                    can_sign: k.data.can_sign,
                    can_encrypt: k.data.can_encrypt,
                    source: k.data.source,
                    public_data: b64.encode(&k.data.public_data),
                    issuer: k.data.issuer,
                    serial: k.data.serial,
                    decision: k.decision,
                    secret,
                }
            })
            .collect();
        keys.sort_by(|a, b| a.fingerprint.cmp(&b.fingerprint));
        Ok(keys)
    }

    fn plugins(&self) -> Result<Vec<PluginId>, SinkError> {
        let mut ids: Vec<PluginId> = self
            .service
            .store()
            .plugins()
            .map_err(fatal)?
            .into_iter()
            .filter(|p| !matches!(p.source, nail_api::plugins::PluginPackageSource::Dev { .. }))
            .map(|p| p.manifest.id)
            .collect();
        ids.sort();
        Ok(ids)
    }
}

impl BackupSource for CoreBackupSource {
    fn info(&mut self) -> BackupInfo {
        BackupInfo {
            app_version: env!("CARGO_PKG_VERSION").to_owned(),
            schema_version: self.service.store().schema_version().unwrap_or_default(),
            accounts: self
                .service
                .inner
                .accounts
                .config()
                .accounts
                .iter()
                .map(|a| a.id.clone())
                .collect(),
            plugins: self.plugins().unwrap_or_default(),
        }
    }

    fn files(&mut self, item: BackupItem) -> Result<Vec<BackupFile>, SinkError> {
        let config = self.service.config();
        let file = |name: &str| BackupFile {
            name: name.to_owned(),
        };
        let exists = |f: ConfigFile| config.file_text(f).map(|t| t.is_some()).map_err(fatal);
        Ok(match item {
            BackupItem::Settings => vec![file(SETTINGS_FILE)],
            BackupItem::Theme => exists(ConfigFile::Theme)?
                .then(|| file("theme.kdl"))
                .into_iter()
                .collect(),
            BackupItem::Layout => exists(ConfigFile::Layout)?
                .then(|| file("layout.kdl"))
                .into_iter()
                .collect(),
            BackupItem::Accounts => exists(ConfigFile::Accounts)?
                .then(|| file("accounts.kdl"))
                .into_iter()
                .collect(),
            BackupItem::Rules => {
                let mut files: Vec<BackupFile> = exists(ConfigFile::Rules)?
                    .then(|| file("rules.kdl"))
                    .into_iter()
                    .collect();
                files.push(file(SAVED_SEARCHES_FILE));
                files
            }
            BackupItem::Keys => vec![file(KEYRING_FILE)],
            BackupItem::Plugins => self
                .plugins()?
                .into_iter()
                .map(|id| file(&format!("{id}/{PACKAGE_FILE}")))
                .collect(),
            BackupItem::LocalMail | BackupItem::PimLocal => Vec::new(),
        })
    }

    fn write_file(
        &mut self,
        item: BackupItem,
        name: &str,
        out: &mut dyn Write,
    ) -> Result<(), SinkError> {
        let bytes: Vec<u8> = if let Some(file) = config_file(item, name) {
            self.service
                .config()
                .file_text(file)
                .map_err(fatal)?
                .unwrap_or_default()
                .into_bytes()
        } else {
            match (item, name) {
                (BackupItem::Settings, SETTINGS_FILE) => {
                    serde_json::to_vec_pretty(&self.settings()?).map_err(fatal)?
                }
                (BackupItem::Rules, SAVED_SEARCHES_FILE) => {
                    let mut searches = self.service.store().list_saved_searches().map_err(fatal)?;
                    searches.sort_by(|a, b| a.id.cmp(&b.id));
                    serde_json::to_vec_pretty(&searches).map_err(fatal)?
                }
                (BackupItem::Keys, KEYRING_FILE) => {
                    serde_json::to_vec_pretty(&self.keyring()?).map_err(fatal)?
                }
                (BackupItem::Plugins, _) => {
                    let id = name
                        .strip_suffix(&format!("/{PACKAGE_FILE}"))
                        .ok_or_else(|| fatal(format!("unknown plug-in file {name}")))?;
                    let packages = self.service.inner.plugins.packages().map_err(fatal)?;
                    std::fs::read(packages.path(&PluginId::new(id.to_owned()))).map_err(fatal)?
                }
                _ => return Err(fatal(format!("unknown backup file {name}"))),
            }
        };
        out.write_all(&bytes).map_err(fatal)
    }

    fn local_mail_accounts(&mut self) -> Result<Vec<AccountId>, SinkError> {
        let mut ids: Vec<AccountId> = self
            .service
            .store()
            .list_accounts()
            .map_err(fatal)?
            .into_iter()
            .filter(|a| a.kind == AccountKind::Local)
            .map(|a| a.id)
            .collect();
        ids.sort();
        Ok(ids)
    }

    fn local_mail(
        &mut self,
        account: &AccountId,
    ) -> Result<Box<dyn MailExportSource + '_>, SinkError> {
        Ok(Box::new(
            StoreMailSource::account(&self.service, account).map_err(fatal)?,
        ))
    }

    fn local_calendars(&mut self) -> Result<Box<dyn PimExportSource + '_>, SinkError> {
        Ok(Box::new(
            StorePimSource::local_calendars(&self.service).map_err(fatal)?,
        ))
    }

    fn local_address_books(&mut self) -> Result<Box<dyn PimExportSource + '_>, SinkError> {
        Ok(Box::new(
            StorePimSource::local_address_books(&self.service).map_err(fatal)?,
        ))
    }
}

/// The core as a restore's target (blocking).
pub(crate) struct CoreRestoreTarget {
    service: Service,
    dry_run: bool,
    mail: Option<ImportTarget>,
    /// Mail targets done (their changes are announced afterwards).
    pub(crate) done: Vec<ImportTarget>,
    /// PIM items to write afterwards.
    pub(crate) pim: PimBuffer,
    /// `accounts.kdl` / `rules.kdl` were replaced (reload them).
    pub(crate) reload: Vec<ConfigFile>,
    /// Theme / layout updates to broadcast.
    pub(crate) updates: Vec<nail_config::ConfigUpdate>,
    /// Plug-ins installed (disabled).
    pub(crate) plugins_installed: bool,
}

impl CoreRestoreTarget {
    pub(crate) fn new(service: &Service, dry_run: bool) -> Self {
        Self {
            service: service.clone(),
            dry_run,
            mail: None,
            done: Vec::new(),
            pim: PimBuffer::new(service, PimMode::Restore, None, None),
            reload: Vec::new(),
            updates: Vec::new(),
            plugins_installed: false,
        }
    }

    fn mail(&mut self) -> Result<&mut ImportTarget, SinkError> {
        let target = match self.mail.take() {
            Some(target) => target,
            // Mail before `begin_mail_account`: Local Folders.
            None => ImportTarget::local_account(
                &self.service,
                &AccountId::new(LOCAL_FOLDERS_ACCOUNT_ID),
                self.dry_run,
            )
            .map_err(fatal)?,
        };
        Ok(self.mail.insert(target))
    }

    /// Writes what is pending and collects the mail targets.
    pub(crate) fn finish(&mut self) -> Result<(), SinkError> {
        if let Some(mut target) = self.mail.take() {
            target.flush()?;
            self.done.push(target);
        }
        Ok(())
    }

    fn current_settings(&self) -> Result<BTreeMap<String, serde_json::Value>, SinkError> {
        let mut all = self.service.store().all_settings().map_err(fatal)?;
        all.retain(|k, _| !EXCLUDED_SETTINGS.contains(&k.as_str()));
        Ok(all)
    }

    fn merge_keys(&self, content: &[u8], dry_run: bool) -> Result<MergeOutcome, SinkError> {
        let keys: Vec<BackupKey> = serde_json::from_slice(content).map_err(item_err)?;
        // Checked before anything is written (also in a dry run).
        for key in &keys {
            key.check_reference().map_err(item_err)?;
        }
        let store = self.service.store();
        let b64 = base64::engine::general_purpose::STANDARD;
        let mut outcome = MergeOutcome::default();
        let existing = store.list_keys(None, false).map_err(fatal)?;
        for key in keys {
            let found = existing.iter().find(|k| {
                k.data.technology == key.technology && k.data.fingerprint == key.fingerprint
            });
            match found {
                Some(k) if k.data.secret.is_some() || key.secret.is_none() => {
                    outcome.unchanged += 1;
                    continue;
                }
                Some(_) => outcome.replaced += 1,
                None => outcome.added += 1,
            }
            if dry_run {
                continue;
            }
            if let Some(secret) = &key.secret
                && let (Some(reference), Some(value)) = (&secret.reference, &secret.keychain_secret)
            {
                let value = b64.decode(value).map_err(item_err)?;
                let value = String::from_utf8(value).map_err(item_err)?;
                self.service
                    .secrets()
                    .set(&SecretRef(reference.clone()), &value)
                    .map_err(item_err)?;
            }
            let data = KeyData {
                technology: key.technology,
                fingerprint: key.fingerprint.clone(),
                user_ids: key.user_ids.clone(),
                emails: key.emails.clone(),
                algorithm: key.algorithm.clone(),
                created_at: key.created_at,
                expires_at: key.expires_at,
                revoked: key.revoked,
                can_sign: key.can_sign,
                can_encrypt: key.can_encrypt,
                source: key.source,
                public_data: b64.decode(&key.public_data).map_err(item_err)?,
                secret: match &key.secret {
                    Some(s) => Some(SecretKeyData {
                        storage: s.storage,
                        data: s
                            .data
                            .as_ref()
                            .map(|d| b64.decode(d))
                            .transpose()
                            .map_err(item_err)?,
                        reference: s.reference.clone(),
                    }),
                    None => None,
                },
                issuer: key.issuer.clone(),
                serial: key.serial.clone(),
            };
            store.upsert_key(&data).map_err(item_err)?;
            if let Some(decision) = key.decision {
                store
                    .set_trust_decision(key.technology, &key.fingerprint, decision)
                    .map_err(item_err)?;
            }
        }
        if !dry_run && (outcome.added > 0 || outcome.replaced > 0) {
            self.service.broadcast(nail_api::Event::KeysChanged);
        }
        Ok(outcome)
    }

    fn merge_plugin(
        &mut self,
        name: &str,
        content: &[u8],
        dry_run: bool,
    ) -> Result<MergeOutcome, SinkError> {
        let id = name
            .strip_suffix(&format!("/{PACKAGE_FILE}"))
            .ok_or_else(|| item_err(format!("unknown plug-in file {name}")))?;
        let installed = self
            .service
            .store()
            .plugin(&PluginId::new(id.to_owned()))
            .is_ok();
        if installed {
            return Ok(MergeOutcome {
                unchanged: 1,
                ..MergeOutcome::default()
            });
        }
        if dry_run {
            return Ok(MergeOutcome {
                added: 1,
                ..MergeOutcome::default()
            });
        }
        // Through the normal install path: checked like any package,
        // installed disabled and without permissions (reviewed by the user).
        let packages = self.service.inner.plugins.packages().map_err(item_err)?;
        let path = packages.dir().join(format!("restore-{id}.nailplugin.tmp"));
        std::fs::create_dir_all(packages.dir()).map_err(item_err)?;
        std::fs::write(&path, content).map_err(item_err)?;
        let source = nail_api::plugins::PluginPackageSource::File {
            path: path.display().to_string(),
        };
        let service = self.service.clone();
        let sha = nail_plugins::package::sha256_hex(content);
        let answer = tokio::runtime::Handle::current().block_on(async move {
            service
                .handle(nail_api::Request::InstallPlugin {
                    source,
                    package_sha256: sha,
                    grants: Vec::new(),
                    enable: false,
                })
                .await
        });
        drop(std::fs::remove_file(&path));
        if let nail_api::Response::Error { message, .. } = answer {
            return Err(SinkError::Item(message));
        }
        self.plugins_installed = true;
        Ok(MergeOutcome {
            added: 1,
            ..MergeOutcome::default()
        })
    }
}

impl MailImportSink for CoreRestoreTarget {
    fn ensure_folder(&mut self, folder: &SourceFolder) -> Result<(), SinkError> {
        self.mail()?.ensure_folder(folder)
    }

    fn contains(&mut self, folder: &SourceFolder, key: &DuplicateKey) -> Result<bool, SinkError> {
        self.mail()?.contains(folder, key)
    }

    fn append(&mut self, folder: &SourceFolder, message: ImportedMessage) -> Result<(), SinkError> {
        self.mail()?.append(folder, message)
    }
}

impl PimImportSink for CoreRestoreTarget {
    fn event(&mut self, c: &SourceCollection, e: nail_ical::ParsedEvent) -> Result<(), SinkError> {
        self.pim.event(c, e)
    }
    fn task(&mut self, c: &SourceCollection, t: nail_ical::ParsedTask) -> Result<(), SinkError> {
        self.pim.task(c, t)
    }
    fn contact(
        &mut self,
        c: &SourceCollection,
        p: nail_ical::vcard::ParsedContact,
    ) -> Result<(), SinkError> {
        self.pim.contact(c, p)
    }
    fn group(
        &mut self,
        c: &SourceCollection,
        g: nail_ical::vcard::ParsedGroup,
    ) -> Result<(), SinkError> {
        self.pim.group(c, g)
    }
    fn exists(&mut self, c: &SourceCollection, uid: &str) -> bool {
        self.pim.exists(c, uid)
    }
}

impl RestoreTarget for CoreRestoreTarget {
    fn current_file(&mut self, item: BackupItem, name: &str) -> Result<Option<Vec<u8>>, SinkError> {
        if let Some(file) = config_file(item, name) {
            return Ok(self
                .service
                .config()
                .file_text(file)
                .map_err(fatal)?
                .map(String::into_bytes));
        }
        match (item, name) {
            (BackupItem::Settings, SETTINGS_FILE) => Ok(Some(
                serde_json::to_vec_pretty(&self.current_settings()?).map_err(fatal)?,
            )),
            (BackupItem::Rules, SAVED_SEARCHES_FILE) => {
                let mut searches = self.service.store().list_saved_searches().map_err(fatal)?;
                searches.sort_by(|a, b| a.id.cmp(&b.id));
                Ok(Some(serde_json::to_vec_pretty(&searches).map_err(fatal)?))
            }
            _ => Ok(None),
        }
    }

    fn replace_file(
        &mut self,
        item: BackupItem,
        name: &str,
        content: &[u8],
    ) -> Result<Option<ConfigBackupId>, SinkError> {
        if let Some(file) = config_file(item, name) {
            let text = std::str::from_utf8(content).map_err(item_err)?;
            let (backup, updates) = self
                .service
                .config()
                .replace_file(file, text)
                .map_err(item_err)?;
            self.updates.extend(updates);
            if matches!(file, ConfigFile::Accounts | ConfigFile::Rules) {
                self.reload.push(file);
            }
            return Ok(Some(backup.id));
        }
        match (item, name) {
            (BackupItem::Settings, SETTINGS_FILE) => {
                let settings: BTreeMap<String, serde_json::Value> =
                    serde_json::from_slice(content).map_err(item_err)?;
                let store = self.service.store();
                let mut refused = Vec::new();
                for (key, value) in settings {
                    if EXCLUDED_SETTINGS.contains(&key.as_str()) {
                        continue;
                    }
                    if let Err(why) = check_restored_setting(&key, &value) {
                        tracing::warn!(%key, %why, "a backed-up setting is not restored");
                        refused.push(format!("{key}: {why}"));
                        continue;
                    }
                    store.set_setting(&key, &value).map_err(item_err)?;
                }
                if refused.is_empty() {
                    Ok(None)
                } else {
                    // Reported as an issue of the settings file; the valid
                    // settings are restored.
                    Err(item_err(format!(
                        "settings not restored: {}",
                        refused.join("; ")
                    )))
                }
            }
            (BackupItem::Rules, SAVED_SEARCHES_FILE) => {
                let searches: Vec<nail_api::SavedSearch> =
                    serde_json::from_slice(content).map_err(item_err)?;
                let store = self.service.store();
                for search in searches {
                    store.save_saved_search(&search).map_err(item_err)?;
                }
                self.service
                    .broadcast(nail_api::Event::SavedSearchesChanged);
                Ok(None)
            }
            _ => Err(item_err(format!("unknown backup file {name}"))),
        }
    }

    fn merge_file(
        &mut self,
        item: BackupItem,
        name: &str,
        content: &[u8],
        dry_run: bool,
    ) -> Result<MergeOutcome, SinkError> {
        match (item, name) {
            (BackupItem::Keys, KEYRING_FILE) => self.merge_keys(content, dry_run),
            (BackupItem::Plugins, _) => self.merge_plugin(name, content, dry_run),
            _ => Err(item_err(format!("unknown backup file {name}"))),
        }
    }

    fn begin_mail_account(&mut self, account: &AccountId, dry_run: bool) -> Result<(), SinkError> {
        self.finish()?;
        self.mail = Some(
            ImportTarget::local_account(&self.service, account, dry_run || self.dry_run)
                .map_err(fatal)?,
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn service() -> (Service, tempfile::TempDir, tempfile::TempDir) {
        let data = tempfile::tempdir().unwrap();
        let config = tempfile::tempdir().unwrap();
        let mut options = crate::ServiceOptions::new(data.path(), config.path());
        options.watch_config = false;
        options.start_engines = false;
        let service = Service::open(crate::demo::demo_options(options)).unwrap();
        (service, data, config)
    }

    fn restore_settings(service: &Service, settings: &serde_json::Value) -> Result<(), SinkError> {
        let mut target = CoreRestoreTarget::new(service, false);
        target
            .replace_file(
                BackupItem::Settings,
                SETTINGS_FILE,
                &serde_json::to_vec(settings).unwrap(),
            )
            .map(drop)
    }

    /// Machine-bound security settings never come from a backup, and
    /// known settings that fail their setter's checks are refused (with an
    /// issue) while the valid ones are restored.
    #[test]
    fn restored_settings_skip_machine_bound_keys_and_invalid_values() {
        let (service, _data, _config) = service();
        let store = service.store();
        let result = restore_settings(
            &service,
            &(serde_json::json!({
                crate::smartcard::SMARTCARD_SETTINGS_KEY: {
                    "modules": [{ "path": "evil.dll", "enabled": true }],
                    "pin_cache_minutes": 5
                },
                nail_store::REMOTE_UI_SETTINGS_KEY: {
                    "enabled": true, "bind_address": "192.0.2.1", "tls": "off"
                },
                nail_store::MCP_SETTINGS_KEY: { "enabled": true },
                nail_store::NOTIFICATION_SETTINGS_KEY: {
                    "quiet_hours": { "start": "25:99", "end": "07:00" }
                },
                nail_store::SECURITY_SETTINGS_KEY: "not settings",
                "some.unknown.key": 42
            })),
        );
        let Err(SinkError::Item(detail)) = result else {
            panic!("an issue for the refused settings expected, got {result:?}");
        };
        assert!(
            detail.contains(nail_store::NOTIFICATION_SETTINGS_KEY),
            "{detail}"
        );
        assert!(
            detail.contains(nail_store::SECURITY_SETTINGS_KEY),
            "{detail}"
        );
        let all = store.all_settings().unwrap();
        for key in [
            crate::smartcard::SMARTCARD_SETTINGS_KEY,
            nail_store::REMOTE_UI_SETTINGS_KEY,
            nail_store::MCP_SETTINGS_KEY,
            nail_store::NOTIFICATION_SETTINGS_KEY,
            nail_store::SECURITY_SETTINGS_KEY,
        ] {
            assert!(!all.contains_key(key), "{key} must not be restored");
        }
        assert_eq!(all.get("some.unknown.key"), Some(&serde_json::json!(42)));
        // Valid settings alone restore without an issue.
        restore_settings(
            &service,
            &(serde_json::json!({ nail_store::NOTIFICATION_SETTINGS_KEY: {
                "enabled": true, "show_preview": true, "sound": false,
                "folders": "inbox", "quiet_hours": null
            } })),
        )
        .unwrap();
        assert!(!store.notification_settings().unwrap().sound);
    }

    #[test]
    fn machine_bound_settings_are_not_backed_up() {
        let (service, _data, _config) = service();
        let store = service.store();
        for key in [
            crate::smartcard::SMARTCARD_SETTINGS_KEY,
            nail_store::REMOTE_UI_SETTINGS_KEY,
            nail_store::MCP_SETTINGS_KEY,
        ] {
            store.set_setting(key, &serde_json::json!({})).unwrap();
        }
        let source = CoreBackupSource {
            service: service.clone(),
        };
        let settings = source.settings().unwrap();
        for key in EXCLUDED_SETTINGS {
            assert!(!settings.contains_key(*key), "{key} must not be backed up");
        }
    }

    fn key_with_secret(reference: &str, storage: SecretStorage) -> serde_json::Value {
        let b64 = base64::engine::general_purpose::STANDARD;
        serde_json::to_value(BackupKey {
            technology: CryptoTechnology::OpenPgp,
            fingerprint: "ABCDEF0123456789".into(),
            user_ids: vec!["Mallory <m@example.org>".into()],
            emails: vec!["m@example.org".into()],
            algorithm: "ed25519".into(),
            created_at: 0,
            expires_at: None,
            revoked: false,
            can_sign: true,
            can_encrypt: false,
            source: KeySource::Generated,
            public_data: b64.encode(b"public"),
            issuer: None,
            serial: None,
            decision: None,
            secret: Some(BackupSecret {
                storage,
                data: Some(b64.encode(b"sealed")),
                reference: Some(reference.into()),
                keychain_secret: Some(b64.encode(b"backed-up value")),
            }),
        })
        .unwrap()
    }

    fn merge(
        service: &Service,
        keys: &serde_json::Value,
        dry_run: bool,
    ) -> Result<MergeOutcome, SinkError> {
        let target = CoreRestoreTarget::new(service, false);
        target.merge_keys(&serde_json::to_vec(keys).unwrap(), dry_run)
    }

    /// A restored key may only write its own wrapping entry: references to
    /// account passwords or the vault's data key are refused before
    /// anything is written.
    #[test]
    fn restored_keys_cannot_write_foreign_keychain_entries() {
        let (service, _data, _config) = service();
        let secrets = service.secrets();
        let password = SecretRef("keychain:nail/work/password".into());
        secrets.set(&password, "the real password").unwrap();
        let vault_key = SecretRef("keychain:nail/vault-1234/data-key".into());
        secrets.set(&vault_key, "the data key").unwrap();
        for (reference, storage) in [
            (password.0.as_str(), SecretStorage::Keychain),
            (vault_key.0.as_str(), SecretStorage::Keychain),
            (password.0.as_str(), SecretStorage::Passphrase),
            (
                "keychain:nail/openpgp/0000000000000000",
                SecretStorage::Keychain,
            ),
        ] {
            for dry_run in [true, false] {
                let result = merge(
                    &service,
                    &(serde_json::json!([key_with_secret(reference, storage)])),
                    dry_run,
                );
                assert!(
                    matches!(result, Err(SinkError::Item(_))),
                    "{reference} must be refused, got {result:?}"
                );
            }
        }
        let value = |r: &SecretRef| secrets.get(r).unwrap().map(|v| v.as_str().to_owned());
        assert_eq!(value(&password).as_deref(), Some("the real password"));
        assert_eq!(value(&vault_key).as_deref(), Some("the data key"));
        assert!(service.store().list_keys(None, false).unwrap().is_empty());

        // The key's own entry is restored.
        let own = nail_pgp::keyring::keychain_ref("ABCDEF0123456789");
        let outcome = merge(
            &service,
            &(serde_json::json!([key_with_secret(&own.0, SecretStorage::Keychain)])),
            false,
        )
        .unwrap();
        assert_eq!(outcome.added, 1);
        assert_eq!(value(&own).as_deref(), Some("backed-up value"));
    }
}
