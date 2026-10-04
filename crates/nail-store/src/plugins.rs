//! Schema v9 (M2d, ADR 0050): installed plug-ins, their grants, settings,
//! key-value storage (with quotas) and logs.
//!
//! The store keeps what the core needs to list and restart plug-ins; the
//! package files live in the data directory (`plugins/<id>/`, written by
//! the host). Everything of a plug-in is removed with it.
//!
//! Storage values and log messages are sealed at rest when encryption at
//! rest is on ([`crate::SEALED_COLUMNS`]); sizes and quotas count the plain
//! JSON.

use std::collections::BTreeMap;

use nail_api::plugins::admin::{
    DEFAULT_STORAGE_QUOTA, MAX_LOG_LINES, MAX_LOG_MESSAGE, MAX_STORAGE_KEY, MAX_STORAGE_VALUE,
};
use nail_api::plugins::{
    PluginGrant, PluginLogEntry, PluginLogLevel, PluginLogSource, PluginManifest,
    PluginPackageSource, PluginPermission, PluginStorageUsage,
};
use nail_api::{AccountId, ApiError, LocalizedMessage, PluginId};
use rusqlite::{OptionalExtension, Row, Transaction, params};
use serde_json::Value;

use crate::error::{Error, Result};
use crate::store::Store;

const RFC3339: &str = "'%Y-%m-%dT%H:%M:%SZ'";

/// An installed plug-in as stored.
#[derive(Debug, Clone, PartialEq)]
pub struct PluginRecord {
    /// Its id.
    pub id: PluginId,
    /// Its manifest as installed (texts not resolved).
    pub manifest: PluginManifest,
    /// Where it came from.
    pub source: PluginPackageSource,
    /// SHA-256 (hex) of the package; `None` for dev plug-ins.
    pub package_sha256: Option<String>,
    /// Whether the user enabled it.
    pub enabled: bool,
    /// Its storage quota (bytes).
    pub storage_quota: u64,
    /// When it was installed (RFC 3339).
    pub installed_at: String,
    /// When it was last updated (RFC 3339).
    pub updated_at: String,
    /// What stopped it last.
    pub last_error: Option<ApiError>,
}

/// A plug-in to install or update ([`Store::install_plugin`]).
#[derive(Debug, Clone)]
pub struct PluginInstall<'a> {
    /// Its manifest (validated by the caller).
    pub manifest: &'a PluginManifest,
    /// Where it comes from.
    pub source: &'a PluginPackageSource,
    /// SHA-256 (hex) of the package; `None` for dev plug-ins.
    pub package_sha256: Option<&'a str>,
    /// The granted permissions (replace the previous ones).
    pub grants: &'a [PluginGrant],
    /// Whether it is enabled.
    pub enabled: bool,
}

/// A log line to append ([`Store::append_plugin_log`]).
#[derive(Debug, Clone)]
pub struct PluginLogLine<'a> {
    /// The plug-in.
    pub plugin_id: &'a PluginId,
    /// When (Unix milliseconds).
    pub at_ms: i64,
    /// The level.
    pub level: PluginLogLevel,
    /// Who wrote it.
    pub source: PluginLogSource,
    /// The text (cut to [`MAX_LOG_MESSAGE`] bytes).
    pub message: &'a str,
    /// For host lines: the user-facing text.
    pub localized: Option<&'a LocalizedMessage>,
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

fn json_col<T: serde::de::DeserializeOwned>(idx: usize, text: &str) -> rusqlite::Result<T> {
    serde_json::from_str(text).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(idx, rusqlite::types::Type::Text, Box::new(e))
    })
}

fn plugin_cols() -> String {
    format!(
        "id, manifest, source_kind, source, package_sha256, enabled, storage_quota,
         strftime({RFC3339}, installed_at, 'unixepoch'),
         strftime({RFC3339}, updated_at, 'unixepoch'), last_error"
    )
}

fn plugin_from_row(row: &Row<'_>) -> rusqlite::Result<PluginRecord> {
    let kind: String = row.get(2)?;
    let location: String = row.get(3)?;
    let source = PluginPackageSource::from_parts(&kind, location).ok_or_else(|| {
        rusqlite::Error::FromSqlConversionFailure(
            2,
            rusqlite::types::Type::Text,
            format!("unknown source kind {kind}").into(),
        )
    })?;
    let last_error: Option<String> = row.get(9)?;
    Ok(PluginRecord {
        id: PluginId(row.get(0)?),
        manifest: json_col(1, &row.get::<_, String>(1)?)?,
        source,
        package_sha256: row.get(4)?,
        enabled: row.get(5)?,
        storage_quota: u64::try_from(row.get::<_, i64>(6)?).unwrap_or(0),
        installed_at: row.get(7)?,
        updated_at: row.get(8)?,
        last_error: last_error.map(|e| json_col(9, &e)).transpose()?,
    })
}

fn get_plugin(tx: &rusqlite::Connection, id: &PluginId) -> Result<PluginRecord> {
    tx.query_row(
        &format!("SELECT {} FROM plugins WHERE id = ?1", plugin_cols()),
        [id.as_str()],
        plugin_from_row,
    )
    .optional()?
    .ok_or_else(|| Error::not_found("plug-in", id))
}

/// Checks grants against a manifest: only declared permissions, each
/// once, account lists only on mail permissions (and not empty).
pub fn check_grants(manifest: &PluginManifest, grants: &[PluginGrant]) -> Result<()> {
    let mut seen = Vec::new();
    for grant in grants {
        let name = grant.permission.name();
        if !manifest.declares(&grant.permission) {
            return Err(Error::InvalidData(format!(
                "permission {name} is not declared by plug-in {}",
                manifest.id
            )));
        }
        if seen.contains(&&grant.permission) {
            return Err(Error::InvalidData(format!(
                "permission {name} granted twice"
            )));
        }
        seen.push(&grant.permission);
        if let Some(accounts) = &grant.accounts
            && (!grant.permission.takes_accounts() || accounts.is_empty())
        {
            return Err(Error::InvalidData(format!(
                "permission {name} cannot be limited to these accounts"
            )));
        }
    }
    Ok(())
}

fn write_grants(tx: &Transaction<'_>, id: &PluginId, grants: &[PluginGrant]) -> Result<()> {
    tx.execute(
        "DELETE FROM plugin_grants WHERE plugin_id = ?1",
        [id.as_str()],
    )?;
    let mut stmt = tx.prepare(
        "INSERT INTO plugin_grants (plugin_id, permission, accounts, granted_at)
         VALUES (?1, ?2, ?3, ?4)",
    )?;
    let at = now();
    for grant in grants {
        let accounts = grant
            .accounts
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?;
        stmt.execute(params![id.as_str(), grant.permission.name(), accounts, at])?;
    }
    Ok(())
}

fn read_grants(conn: &rusqlite::Connection, id: &PluginId) -> Result<Vec<PluginGrant>> {
    let mut stmt = conn.prepare(
        "SELECT permission, accounts FROM plugin_grants WHERE plugin_id = ?1 ORDER BY permission",
    )?;
    let rows = stmt
        .query_map([id.as_str()], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut grants = Vec::new();
    for (name, accounts) in rows {
        // A permission this build no longer knows grants nothing.
        let Ok(permission) = PluginPermission::parse(&name) else {
            continue;
        };
        let accounts: Option<Vec<AccountId>> =
            accounts.map(|a| serde_json::from_str(&a)).transpose()?;
        grants.push(PluginGrant {
            permission,
            accounts,
        });
    }
    Ok(grants)
}

fn usage(conn: &rusqlite::Connection, id: &PluginId) -> Result<PluginStorageUsage> {
    let (keys, used, quota): (i64, i64, Option<i64>) = conn.query_row(
        "SELECT (SELECT COUNT(*) FROM plugin_storage WHERE plugin_id = ?1),
                (SELECT COALESCE(SUM(size), 0) FROM plugin_storage WHERE plugin_id = ?1),
                (SELECT storage_quota FROM plugins WHERE id = ?1)",
        [id.as_str()],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    let quota = quota.ok_or_else(|| Error::not_found("plug-in", id))?;
    Ok(PluginStorageUsage {
        keys: u32::try_from(keys).unwrap_or(u32::MAX),
        used_bytes: u64::try_from(used).unwrap_or(0),
        quota_bytes: u64::try_from(quota).unwrap_or(0),
    })
}

fn cut(text: &str, max: usize) -> &str {
    if text.len() <= max {
        return text;
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

impl Store {
    /// Installs a plug-in, or updates an installed one with the same id:
    /// manifest, source, hash, grants and `enabled` are replaced; settings
    /// of keys the new manifest no longer has are dropped; storage, logs
    /// and the install time are kept. Clears the last error.
    pub fn install_plugin(&self, install: &PluginInstall<'_>) -> Result<PluginRecord> {
        let manifest = install.manifest;
        check_grants(manifest, install.grants)?;
        if let Some(hash) = install.package_sha256
            && (hash.len() != 64
                || !hash
                    .bytes()
                    .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()))
        {
            return Err(Error::InvalidData(format!("invalid package hash {hash}")));
        }
        let id = &manifest.id;
        let manifest_json = serde_json::to_string(manifest)?;
        self.with_tx(|tx| {
            let at = now();
            tx.execute(
                "INSERT INTO plugins (id, version, manifest, source_kind, source, package_sha256,
                                      enabled, storage_quota, installed_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9)
                 ON CONFLICT (id) DO UPDATE SET
                    version = excluded.version, manifest = excluded.manifest,
                    source_kind = excluded.source_kind, source = excluded.source,
                    package_sha256 = excluded.package_sha256, enabled = excluded.enabled,
                    updated_at = excluded.updated_at, last_error = NULL",
                params![
                    id.as_str(),
                    manifest.version,
                    manifest_json,
                    install.source.kind(),
                    install.source.location(),
                    install.package_sha256,
                    install.enabled,
                    i64::try_from(DEFAULT_STORAGE_QUOTA).unwrap_or(i64::MAX),
                    at
                ],
            )?;
            write_grants(tx, id, install.grants)?;
            // Keep only settings the new manifest still has, with valid values.
            let settings: String = tx.query_row(
                "SELECT settings FROM plugins WHERE id = ?1",
                [id.as_str()],
                |r| r.get(0),
            )?;
            let mut values: BTreeMap<String, Value> = serde_json::from_str(&settings)?;
            let invalid = manifest.invalid_settings(&values);
            values.retain(|k, _| !invalid.contains(k));
            tx.execute(
                "UPDATE plugins SET settings = ?2 WHERE id = ?1",
                params![id.as_str(), serde_json::to_string(&values)?],
            )?;
            get_plugin(tx, id)
        })
    }

    /// One plug-in.
    pub fn plugin(&self, id: &PluginId) -> Result<PluginRecord> {
        self.with_conn(|conn| get_plugin(conn, id))
    }

    /// Every plug-in, by id.
    pub fn plugins(&self) -> Result<Vec<PluginRecord>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(&format!(
                "SELECT {} FROM plugins ORDER BY id",
                plugin_cols()
            ))?;
            Ok(stmt
                .query_map([], plugin_from_row)?
                .collect::<rusqlite::Result<Vec<_>>>()?)
        })
    }

    /// Enables or disables a plug-in; enabling clears its last error.
    pub fn set_plugin_enabled(&self, id: &PluginId, enabled: bool) -> Result<()> {
        self.with_conn(|conn| {
            let changed = conn.execute(
                "UPDATE plugins SET enabled = ?2,
                    last_error = CASE WHEN ?2 THEN NULL ELSE last_error END
                 WHERE id = ?1",
                params![id.as_str(), enabled],
            )?;
            if changed == 0 {
                return Err(Error::not_found("plug-in", id));
            }
            Ok(())
        })
    }

    /// Records (or clears) what stopped a plug-in.
    pub fn set_plugin_error(&self, id: &PluginId, error: Option<&ApiError>) -> Result<()> {
        let json = error.map(serde_json::to_string).transpose()?;
        self.with_conn(|conn| {
            let changed = conn.execute(
                "UPDATE plugins SET last_error = ?2 WHERE id = ?1",
                params![id.as_str(), json],
            )?;
            if changed == 0 {
                return Err(Error::not_found("plug-in", id));
            }
            Ok(())
        })
    }

    /// Removes a plug-in with its grants, settings, storage and logs.
    pub fn remove_plugin(&self, id: &PluginId) -> Result<()> {
        self.with_conn(|conn| {
            let removed = conn.execute("DELETE FROM plugins WHERE id = ?1", [id.as_str()])?;
            if removed == 0 {
                return Err(Error::not_found("plug-in", id));
            }
            Ok(())
        })
    }

    /// A plug-in's grants, by permission name.
    pub fn plugin_grants(&self, id: &PluginId) -> Result<Vec<PluginGrant>> {
        self.with_conn(|conn| {
            get_plugin(conn, id)?;
            read_grants(conn, id)
        })
    }

    /// Replaces a plug-in's grants ([`check_grants`]).
    pub fn set_plugin_grants(&self, id: &PluginId, grants: &[PluginGrant]) -> Result<()> {
        self.with_tx(|tx| {
            let record = get_plugin(tx, id)?;
            check_grants(&record.manifest, grants)?;
            write_grants(tx, id, grants)
        })
    }

    /// Every setting's value: the manifest's defaults overlaid with what
    /// the user changed.
    pub fn plugin_settings(&self, id: &PluginId) -> Result<BTreeMap<String, Value>> {
        self.with_conn(|conn| {
            let record = get_plugin(conn, id)?;
            let stored: String = conn.query_row(
                "SELECT settings FROM plugins WHERE id = ?1",
                [id.as_str()],
                |r| r.get(0),
            )?;
            let stored: BTreeMap<String, Value> = serde_json::from_str(&stored)?;
            let mut values = record.manifest.default_settings();
            for (key, value) in stored {
                if values.contains_key(&key) {
                    values.insert(key, value);
                }
            }
            Ok(values)
        })
    }

    /// Changes settings (other keys keep their value); refuses unknown keys
    /// and invalid values ([`Error::InvalidData`] naming them). Returns
    /// every setting's value.
    pub fn update_plugin_settings(
        &self,
        id: &PluginId,
        values: &BTreeMap<String, Value>,
    ) -> Result<BTreeMap<String, Value>> {
        self.with_tx(|tx| {
            let record = get_plugin(tx, id)?;
            let invalid = record.manifest.invalid_settings(values);
            if !invalid.is_empty() {
                return Err(Error::InvalidData(format!(
                    "invalid plug-in settings: {}",
                    invalid.join(", ")
                )));
            }
            let stored: String = tx.query_row(
                "SELECT settings FROM plugins WHERE id = ?1",
                [id.as_str()],
                |r| r.get(0),
            )?;
            let mut stored: BTreeMap<String, Value> = serde_json::from_str(&stored)?;
            stored.extend(values.iter().map(|(k, v)| (k.clone(), v.clone())));
            tx.execute(
                "UPDATE plugins SET settings = ?2 WHERE id = ?1",
                params![id.as_str(), serde_json::to_string(&stored)?],
            )?;
            Ok(())
        })?;
        self.plugin_settings(id)
    }

    /// Sets a plug-in's storage quota (bytes).
    pub fn set_plugin_quota(&self, id: &PluginId, quota: u64) -> Result<()> {
        self.with_conn(|conn| {
            let changed = conn.execute(
                "UPDATE plugins SET storage_quota = ?2 WHERE id = ?1",
                params![id.as_str(), i64::try_from(quota).unwrap_or(i64::MAX)],
            )?;
            if changed == 0 {
                return Err(Error::not_found("plug-in", id));
            }
            Ok(())
        })
    }

    /// How much storage a plug-in uses.
    pub fn plugin_storage_usage(&self, id: &PluginId) -> Result<PluginStorageUsage> {
        self.with_conn(|conn| usage(conn, id))
    }

    /// A stored value.
    pub fn plugin_storage_get(&self, id: &PluginId, key: &str) -> Result<Option<Value>> {
        self.with_conn(|conn| {
            let text: Option<String> = conn
                .query_row(
                    "SELECT nail_open('plugin_storage.value', value) FROM plugin_storage
                     WHERE plugin_id = ?1 AND key = ?2",
                    params![id.as_str(), key],
                    |r| r.get(0),
                )
                .optional()?;
            Ok(text.map(|t| serde_json::from_str(&t)).transpose()?)
        })
    }

    /// Stores a value (replacing the key's previous one). Refuses keys
    /// longer than [`MAX_STORAGE_KEY`], values larger than
    /// [`MAX_STORAGE_VALUE`] ([`Error::InvalidData`]) and writes that would
    /// exceed the quota ([`Error::QuotaExceeded`]). Returns the new usage.
    pub fn plugin_storage_set(
        &self,
        id: &PluginId,
        key: &str,
        value: &Value,
    ) -> Result<PluginStorageUsage> {
        if key.is_empty() || key.len() > MAX_STORAGE_KEY {
            return Err(Error::InvalidData(format!(
                "storage keys have 1 to {MAX_STORAGE_KEY} bytes"
            )));
        }
        let json = serde_json::to_string(value)?;
        if json.len() > MAX_STORAGE_VALUE {
            return Err(Error::InvalidData(format!(
                "storage values have at most {MAX_STORAGE_VALUE} bytes"
            )));
        }
        let size = i64::try_from(key.len() + json.len()).unwrap_or(i64::MAX);
        self.with_tx(|tx| {
            let current = usage(tx, id)?;
            let previous: i64 = tx
                .query_row(
                    "SELECT size FROM plugin_storage WHERE plugin_id = ?1 AND key = ?2",
                    params![id.as_str(), key],
                    |r| r.get(0),
                )
                .optional()?
                .unwrap_or(0);
            let used = i64::try_from(current.used_bytes).unwrap_or(i64::MAX) - previous + size;
            let quota = i64::try_from(current.quota_bytes).unwrap_or(i64::MAX);
            if used > quota {
                return Err(Error::QuotaExceeded {
                    used: u64::try_from(used).unwrap_or(0),
                    quota: current.quota_bytes,
                });
            }
            tx.execute(
                "INSERT INTO plugin_storage (plugin_id, key, value, size, updated_at)
                 VALUES (?1, ?2, nail_seal('plugin_storage.value', ?3), ?4, ?5)
                 ON CONFLICT (plugin_id, key) DO UPDATE SET
                    value = excluded.value, size = excluded.size, updated_at = excluded.updated_at",
                params![id.as_str(), key, json, size, now()],
            )?;
            usage(tx, id)
        })
    }

    /// Removes a key; whether it existed.
    pub fn plugin_storage_delete(&self, id: &PluginId, key: &str) -> Result<bool> {
        self.with_conn(|conn| {
            Ok(conn.execute(
                "DELETE FROM plugin_storage WHERE plugin_id = ?1 AND key = ?2",
                params![id.as_str(), key],
            )? > 0)
        })
    }

    /// The stored keys (optionally only those starting with `prefix`),
    /// sorted.
    pub fn plugin_storage_keys(&self, id: &PluginId, prefix: Option<&str>) -> Result<Vec<String>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT key FROM plugin_storage
                 WHERE plugin_id = ?1 AND (?2 IS NULL OR substr(key, 1, length(?2)) = ?2)
                 ORDER BY key",
            )?;
            Ok(stmt
                .query_map(params![id.as_str(), prefix], |r| r.get(0))?
                .collect::<rusqlite::Result<Vec<String>>>()?)
        })
    }

    /// Removes every stored key of a plug-in.
    pub fn plugin_storage_clear(&self, id: &PluginId) -> Result<()> {
        self.with_conn(|conn| {
            conn.execute(
                "DELETE FROM plugin_storage WHERE plugin_id = ?1",
                [id.as_str()],
            )?;
            Ok(())
        })
    }

    /// Appends a log line (cut to [`MAX_LOG_MESSAGE`] bytes) and drops the
    /// plug-in's oldest lines beyond [`MAX_LOG_LINES`]. Returns the line.
    pub fn append_plugin_log(&self, line: &PluginLogLine<'_>) -> Result<PluginLogEntry> {
        let message = cut(line.message, MAX_LOG_MESSAGE);
        let localized = line.localized.map(serde_json::to_string).transpose()?;
        self.with_tx(|tx| {
            tx.execute(
                "INSERT INTO plugin_logs (plugin_id, at, level, source, message, localized)
                 VALUES (?1, ?2, ?3, ?4, nail_seal('plugin_logs.message', ?5), ?6)",
                params![
                    line.plugin_id.as_str(),
                    line.at_ms,
                    line.level.as_str(),
                    line.source.as_str(),
                    message,
                    localized
                ],
            )?;
            let seq = tx.last_insert_rowid();
            tx.execute(
                "DELETE FROM plugin_logs WHERE plugin_id = ?1 AND seq <= (
                    SELECT seq FROM plugin_logs WHERE plugin_id = ?1
                    ORDER BY seq DESC LIMIT 1 OFFSET ?2)",
                params![line.plugin_id.as_str(), MAX_LOG_LINES],
            )?;
            query_logs(tx, "WHERE l.seq = ?1", params![seq])?
                .pop()
                .ok_or_else(|| Error::not_found("plug-in log line", seq))
        })
    }

    /// The newest `limit` log lines (of one plug-in or all) after
    /// `after_seq`, oldest first.
    pub fn plugin_logs(
        &self,
        plugin: Option<&PluginId>,
        after_seq: Option<u64>,
        limit: u32,
    ) -> Result<Vec<PluginLogEntry>> {
        let after = i64::try_from(after_seq.unwrap_or(0)).unwrap_or(i64::MAX);
        self.with_conn(|conn| {
            let mut lines = query_logs(
                conn,
                "WHERE (?1 IS NULL OR l.plugin_id = ?1) AND l.seq > ?2
                 ORDER BY l.seq DESC LIMIT ?3",
                params![plugin.map(PluginId::as_str), after, limit],
            )?;
            lines.reverse();
            Ok(lines)
        })
    }

    /// Deletes a plug-in's log.
    pub fn clear_plugin_logs(&self, id: &PluginId) -> Result<()> {
        self.with_conn(|conn| {
            conn.execute(
                "DELETE FROM plugin_logs WHERE plugin_id = ?1",
                [id.as_str()],
            )?;
            Ok(())
        })
    }
}

fn query_logs(
    conn: &rusqlite::Connection,
    tail: &str,
    params: impl rusqlite::Params,
) -> Result<Vec<PluginLogEntry>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT l.seq, l.plugin_id, strftime('%Y-%m-%dT%H:%M:%fZ', l.at / 1000.0, 'unixepoch'),
                l.level, l.source, nail_open('plugin_logs.message', l.message), l.localized
         FROM plugin_logs l {tail}"
    ))?;
    let rows = stmt
        .query_map(params, |row| {
            let level: String = row.get(3)?;
            let source: String = row.get(4)?;
            let localized: Option<String> = row.get(6)?;
            Ok(PluginLogEntry {
                seq: u64::try_from(row.get::<_, i64>(0)?).unwrap_or(0),
                plugin_id: PluginId(row.get(1)?),
                at: row.get(2)?,
                level: PluginLogLevel::parse(&level).unwrap_or(PluginLogLevel::Info),
                source: PluginLogSource::parse(&source).unwrap_or(PluginLogSource::Host),
                message: row.get(5)?,
                localized: localized
                    .map(|l| json_col::<LocalizedMessage>(6, &l))
                    .transpose()?
                    .map(Box::new),
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nail_api::ErrorCode;
    use serde_json::json;

    fn manifest(permissions: &Value) -> PluginManifest {
        PluginManifest::parse(
            json!({
                "id": "follow-up", "name": "Follow-up", "version": "1.0.0", "apiVersion": "1",
                "main": "dist/index.js", "permissions": permissions,
                "contributes": {
                    "commands": [{"id": "remind", "title": "Remind"}],
                    "settings": [
                        {"type": "number", "key": "days", "title": "Days", "default": 1, "min": 0},
                        {"type": "boolean", "key": "notify", "title": "Notify"}
                    ]
                }
            })
            .to_string()
            .as_bytes(),
        )
        .unwrap()
    }

    fn dev() -> PluginPackageSource {
        PluginPackageSource::Dev {
            folder: "C:/dev/follow-up".into(),
        }
    }

    fn install(store: &Store, m: &PluginManifest, grants: &[PluginGrant]) -> PluginRecord {
        store
            .install_plugin(&PluginInstall {
                manifest: m,
                source: &dev(),
                package_sha256: None,
                grants,
                enabled: true,
            })
            .unwrap()
    }

    fn id() -> PluginId {
        PluginId::new("follow-up")
    }

    #[test]
    fn install_list_enable_remove() {
        let store = Store::open_in_memory().unwrap();
        let m = manifest(&json!(["mail.read", "storage"]));
        let record = install(&store, &m, &[PluginGrant::all(PluginPermission::Storage)]);
        assert_eq!(record.id, id());
        assert!(record.enabled);
        assert_eq!(record.source, dev());
        assert_eq!(record.storage_quota, DEFAULT_STORAGE_QUOTA);
        assert!(record.installed_at.ends_with('Z'));
        assert_eq!(store.plugins().unwrap(), std::slice::from_ref(&record));

        store.set_plugin_enabled(&id(), false).unwrap();
        let err = ApiError::new(ErrorCode::Internal, "boom");
        store.set_plugin_error(&id(), Some(&err)).unwrap();
        let record = store.plugin(&id()).unwrap();
        assert!(!record.enabled);
        assert_eq!(record.last_error, Some(err));
        store.set_plugin_enabled(&id(), true).unwrap();
        assert_eq!(store.plugin(&id()).unwrap().last_error, None);

        store.remove_plugin(&id()).unwrap();
        assert!(store.plugin(&id()).unwrap_err().is_not_found());
        assert!(store.remove_plugin(&id()).unwrap_err().is_not_found());
        assert!(store.set_plugin_enabled(&id(), true).is_err());
    }

    #[test]
    fn grants_are_checked_and_replaced() {
        let store = Store::open_in_memory().unwrap();
        let m = manifest(&json!(["mail.read", "mail.read.body", "storage"]));
        install(&store, &m, &[]);
        assert!(store.plugin_grants(&id()).unwrap().is_empty());

        let limited = PluginGrant {
            permission: PluginPermission::MailRead,
            accounts: Some(vec!["work".into()]),
        };
        store
            .set_plugin_grants(
                &id(),
                &[limited.clone(), PluginGrant::all(PluginPermission::Storage)],
            )
            .unwrap();
        assert_eq!(
            store.plugin_grants(&id()).unwrap(),
            [limited, PluginGrant::all(PluginPermission::Storage)]
        );

        for bad in [
            vec![PluginGrant::all(PluginPermission::Notifications)],
            vec![
                PluginGrant::all(PluginPermission::Storage),
                PluginGrant::all(PluginPermission::Storage),
            ],
            vec![PluginGrant {
                permission: PluginPermission::Storage,
                accounts: Some(vec!["work".into()]),
            }],
            vec![PluginGrant {
                permission: PluginPermission::MailRead,
                accounts: Some(vec![]),
            }],
        ] {
            assert!(matches!(
                store.set_plugin_grants(&id(), &bad),
                Err(Error::InvalidData(_))
            ));
        }
        assert_eq!(store.plugin_grants(&id()).unwrap().len(), 2);
    }

    #[test]
    fn updates_keep_storage_and_valid_settings() {
        let store = Store::open_in_memory().unwrap();
        let m = manifest(&json!(["storage"]));
        install(&store, &m, &[PluginGrant::all(PluginPermission::Storage)]);
        store
            .update_plugin_settings(
                &id(),
                &BTreeMap::from([
                    ("days".to_owned(), json!(3)),
                    ("notify".to_owned(), json!(true)),
                ]),
            )
            .unwrap();
        store
            .plugin_storage_set(&id(), "k", &json!({"a": 1}))
            .unwrap();

        // Version 2 drops the `notify` setting and the storage permission.
        let mut m2 = manifest(&json!([]));
        m2.version = "2.0.0".into();
        m2.contributes.settings.pop();
        let record = install(&store, &m2, &[]);
        assert_eq!(record.manifest.version, "2.0.0");
        assert!(store.plugin_grants(&id()).unwrap().is_empty());
        assert_eq!(
            store.plugin_settings(&id()).unwrap(),
            BTreeMap::from([("days".to_owned(), json!(3))])
        );
        assert_eq!(
            store.plugin_storage_get(&id(), "k").unwrap(),
            Some(json!({"a": 1}))
        );
        // Grants the new manifest does not declare are refused.
        assert!(
            store
                .install_plugin(&PluginInstall {
                    manifest: &m2,
                    source: &dev(),
                    package_sha256: None,
                    grants: &[PluginGrant::all(PluginPermission::Storage)],
                    enabled: true,
                })
                .is_err()
        );
        assert!(
            store
                .install_plugin(&PluginInstall {
                    manifest: &m2,
                    source: &dev(),
                    package_sha256: Some("ABC"),
                    grants: &[],
                    enabled: true,
                })
                .is_err()
        );
    }

    #[test]
    fn settings_are_validated() {
        let store = Store::open_in_memory().unwrap();
        install(&store, &manifest(&json!([])), &[]);
        assert_eq!(
            store.plugin_settings(&id()).unwrap(),
            BTreeMap::from([
                ("days".to_owned(), json!(1)),
                ("notify".to_owned(), json!(false))
            ])
        );
        let err = store
            .update_plugin_settings(
                &id(),
                &BTreeMap::from([
                    ("days".to_owned(), json!(-1)),
                    ("nope".to_owned(), json!(1)),
                ]),
            )
            .unwrap_err();
        assert!(err.to_string().contains("days, nope"), "{err}");
        let values = store
            .update_plugin_settings(&id(), &BTreeMap::from([("notify".to_owned(), json!(true))]))
            .unwrap();
        assert_eq!(values["notify"], json!(true));
        assert_eq!(values["days"], json!(1));
    }

    #[test]
    fn storage_respects_the_quota() {
        let store = Store::open_in_memory().unwrap();
        install(&store, &manifest(&json!(["storage"])), &[]);
        store.set_plugin_quota(&id(), 20).unwrap();
        let usage = store
            .plugin_storage_set(&id(), "a", &json!("12345"))
            .unwrap();
        assert_eq!(
            usage,
            PluginStorageUsage {
                keys: 1,
                used_bytes: 8,
                quota_bytes: 20
            }
        );
        // Replacing a key counts only the difference.
        store
            .plugin_storage_set(&id(), "a", &json!("1234567890"))
            .unwrap();
        let err = store
            .plugin_storage_set(&id(), "b", &json!("123456789"))
            .unwrap_err();
        assert!(
            matches!(err, Error::QuotaExceeded { quota: 20, .. }),
            "{err}"
        );
        assert_eq!(ApiError::from(err).code, ErrorCode::TooLarge);
        assert_eq!(store.plugin_storage_keys(&id(), None).unwrap(), ["a"]);

        store.set_plugin_quota(&id(), 1000).unwrap();
        store.plugin_storage_set(&id(), "b/1", &json!(1)).unwrap();
        store
            .plugin_storage_set(&id(), "b/2", &json!([true]))
            .unwrap();
        assert_eq!(
            store.plugin_storage_keys(&id(), Some("b/")).unwrap(),
            ["b/1", "b/2"]
        );
        assert!(store.plugin_storage_delete(&id(), "b/1").unwrap());
        assert!(!store.plugin_storage_delete(&id(), "b/1").unwrap());
        assert_eq!(store.plugin_storage_get(&id(), "missing").unwrap(), None);
        assert!(store.plugin_storage_set(&id(), "", &json!(1)).is_err());
        assert!(
            store
                .plugin_storage_set(&id(), &"k".repeat(257), &json!(1))
                .is_err()
        );
        assert!(
            store
                .plugin_storage_set(&id(), "big", &json!("x".repeat(MAX_STORAGE_VALUE)))
                .is_err()
        );
        store.plugin_storage_clear(&id()).unwrap();
        assert_eq!(store.plugin_storage_usage(&id()).unwrap().keys, 0);
        assert!(
            store
                .plugin_storage_set(&PluginId::new("other"), "a", &json!(1))
                .unwrap_err()
                .is_not_found()
        );
    }

    #[test]
    fn storage_and_logs_are_sealed_at_rest() {
        use crate::seal::SealMode;
        use crate::seal::tests::XorCipher;
        let store = Store::open_in_memory().unwrap();
        install(&store, &manifest(&json!(["storage"])), &[]);
        store
            .set_cipher(Some(std::sync::Arc::new(XorCipher)), SealMode::Seal)
            .unwrap();
        store
            .plugin_storage_set(&id(), "k", &json!("Meeting with Sam"))
            .unwrap();
        store
            .append_plugin_log(&PluginLogLine {
                plugin_id: &id(),
                at_ms: 0,
                level: PluginLogLevel::Info,
                source: PluginLogSource::Plugin,
                message: "subject: Meeting with Sam",
                localized: None,
            })
            .unwrap();
        let raw: Vec<String> = store
            .with_conn(|conn| {
                Ok(vec![
                    conn.query_row("SELECT value FROM plugin_storage", [], |r| r.get(0))?,
                    conn.query_row("SELECT message FROM plugin_logs", [], |r| r.get(0))?,
                ])
            })
            .unwrap();
        for value in &raw {
            assert!(crate::seal::is_sealed_field(value), "{value}");
            assert!(!value.contains("Sam"));
        }
        assert_eq!(
            store.plugin_storage_get(&id(), "k").unwrap(),
            Some(json!("Meeting with Sam"))
        );
        assert_eq!(
            store.plugin_logs(None, None, 1).unwrap()[0].message,
            "subject: Meeting with Sam"
        );
        // Quotas count the plain JSON.
        assert_eq!(store.plugin_storage_usage(&id()).unwrap().used_bytes, 19);
        let counts = store.sealed_field_counts().unwrap();
        assert_eq!((counts.sealed, counts.plain), (2, 0));
        // Converting back to plain covers the plug-in columns too.
        while store.convert_fields(false, 10).unwrap() > 0 {}
        assert_eq!(store.sealed_field_counts().unwrap().sealed, 0);
        assert_eq!(
            store.plugin_storage_get(&id(), "k").unwrap(),
            Some(json!("Meeting with Sam"))
        );
    }

    #[test]
    fn logs_are_bounded_and_paged() {
        let store = Store::open_in_memory().unwrap();
        install(&store, &manifest(&json!([])), &[]);
        let text = LocalizedMessage::new("plugin-stopped-cpu-budget").arg("plugin", "follow-up");
        let first = store
            .append_plugin_log(&PluginLogLine {
                plugin_id: &id(),
                at_ms: 1_790_000_000_123,
                level: PluginLogLevel::Error,
                source: PluginLogSource::Host,
                message: "stopped: CPU budget",
                localized: Some(&text),
            })
            .unwrap();
        assert_eq!(first.at, "2026-09-21T14:13:20.123Z");
        assert_eq!(first.localized.as_deref(), Some(&text));
        for i in 0..MAX_LOG_LINES + 5 {
            store
                .append_plugin_log(&PluginLogLine {
                    plugin_id: &id(),
                    at_ms: 1_790_000_000_000,
                    level: PluginLogLevel::Info,
                    source: PluginLogSource::Plugin,
                    message: &format!("line {i}"),
                    localized: None,
                })
                .unwrap();
        }
        let all = store.plugin_logs(Some(&id()), None, 10_000).unwrap();
        assert_eq!(all.len(), MAX_LOG_LINES as usize);
        assert_eq!(
            all.last().unwrap().message,
            format!("line {}", MAX_LOG_LINES + 4)
        );
        let tail = store
            .plugin_logs(None, Some(all[all.len() - 3].seq), 10)
            .unwrap();
        assert_eq!(tail.len(), 2);
        assert!(tail[0].seq < tail[1].seq);
        let newest = store.plugin_logs(Some(&id()), None, 2).unwrap();
        assert_eq!(newest, all[all.len() - 2..]);

        let long = "é".repeat(MAX_LOG_MESSAGE);
        let cut_line = store
            .append_plugin_log(&PluginLogLine {
                plugin_id: &id(),
                at_ms: 0,
                level: PluginLogLevel::Warn,
                source: PluginLogSource::Api,
                message: &long,
                localized: None,
            })
            .unwrap();
        assert!(cut_line.message.len() <= MAX_LOG_MESSAGE);
        store.clear_plugin_logs(&id()).unwrap();
        assert!(store.plugin_logs(Some(&id()), None, 10).unwrap().is_empty());
        // Removing the plug-in removes its log.
        store
            .append_plugin_log(&PluginLogLine {
                plugin_id: &id(),
                at_ms: 0,
                level: PluginLogLevel::Debug,
                source: PluginLogSource::Plugin,
                message: "x",
                localized: None,
            })
            .unwrap();
        store.remove_plugin(&id()).unwrap();
        assert!(store.plugin_logs(None, None, 10).unwrap().is_empty());
    }
}
