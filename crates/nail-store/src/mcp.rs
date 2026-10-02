//! MCP clients, grants and the audit log (schema v4, ADR 0022).
//!
//! Tokens are never stored: [`hash_mcp_token`] (SHA-256, lowercase hex) is
//! what [`Store::create_mcp_client`] takes and what
//! [`Store::mcp_client_by_token`] looks up. Tokens are random and long, so a
//! plain hash is enough (no password hashing needed). Client ids are
//! `mcp-<n>`.

use nail_api::mcp::{
    McpAuditEntry, McpCallKind, McpClient, McpGrant, McpOutcome, McpRateLimits, McpScope,
};
use nail_api::{AccountId, FolderId, McpClientId};
use rusqlite::{OptionalExtension, Row, Transaction, params};
use sha2::{Digest, Sha256};

use crate::error::{Error, Result};
use crate::store::Store;

/// The stored form of a bearer token: lowercase hex SHA-256.
pub fn hash_mcp_token(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

fn client_id(n: i64) -> McpClientId {
    McpClientId(format!("mcp-{n}"))
}

fn parse_client_id(id: &McpClientId) -> Result<i64> {
    id.as_str()
        .strip_prefix("mcp-")
        .and_then(|n| n.parse().ok())
        .ok_or_else(|| Error::not_found("MCP client", id))
}

fn enum_str<T: serde::Serialize>(value: T) -> Result<String> {
    match serde_json::to_value(value)? {
        serde_json::Value::String(s) => Ok(s),
        other => Err(Error::InvalidData(format!("not a string enum: {other}"))),
    }
}

fn enum_from<T: serde::de::DeserializeOwned>(idx: usize, s: String) -> rusqlite::Result<T> {
    serde_json::from_value(serde_json::Value::String(s)).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(idx, rusqlite::types::Type::Text, Box::new(e))
    })
}

const RFC3339: &str = "'%Y-%m-%dT%H:%M:%SZ'";

fn client_cols() -> String {
    format!(
        "id, name, strftime({RFC3339}, created_at, 'unixepoch'),
         strftime({RFC3339}, last_used_at, 'unixepoch'), token_hint,
         all_accounts, skip_draft_confirmation, strftime({RFC3339}, revoked_at, 'unixepoch'),
         rate_limits"
    )
}

fn client_from_row(row: &Row<'_>) -> rusqlite::Result<McpClient> {
    Ok(McpClient {
        id: client_id(row.get(0)?),
        name: row.get(1)?,
        created_at: row.get(2)?,
        last_used_at: row.get(3)?,
        token_hint: row.get(4)?,
        grant: McpGrant {
            all_accounts: row.get(5)?,
            skip_draft_confirmation: row.get(6)?,
            ..McpGrant::default()
        },
        revoked_at: row.get(7)?,
        // NULL (clients from before M4a) or unreadable JSON = the defaults.
        rate_limits: row
            .get::<_, Option<String>>(8)?
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default(),
    })
}

fn load_grant(tx: &Transaction<'_>, n: i64, grant: &mut McpGrant) -> Result<()> {
    let mut stmt = tx.prepare_cached(
        "SELECT kind, value FROM mcp_grants WHERE client_id = ?1 ORDER BY kind, value",
    )?;
    let rows = stmt
        .query_map([n], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for (kind, value) in rows {
        match kind.as_str() {
            "scope" => grant.scopes.push(enum_from(1, value)?),
            "account" => grant.accounts.push(AccountId(value)),
            "exclude-folder" => grant.excluded_folders.push(FolderId(value)),
            _ => {}
        }
    }
    // Scopes in their canonical order.
    grant
        .scopes
        .sort_by_key(|s| McpScope::ALL.iter().position(|x| x == s));
    Ok(())
}

fn write_grant(tx: &Transaction<'_>, n: i64, grant: &McpGrant) -> Result<()> {
    tx.execute(
        "UPDATE mcp_clients SET all_accounts = ?2, skip_draft_confirmation = ?3 WHERE id = ?1",
        params![n, grant.all_accounts, grant.skip_draft_confirmation],
    )?;
    tx.execute("DELETE FROM mcp_grants WHERE client_id = ?1", [n])?;
    let mut stmt = tx.prepare_cached(
        "INSERT OR IGNORE INTO mcp_grants (client_id, kind, value) VALUES (?1, ?2, ?3)",
    )?;
    for scope in &grant.scopes {
        stmt.execute(params![n, "scope", enum_str(scope)?])?;
    }
    for account in &grant.accounts {
        stmt.execute(params![n, "account", account.as_str()])?;
    }
    for folder in &grant.excluded_folders {
        stmt.execute(params![n, "exclude-folder", folder.as_str()])?;
    }
    Ok(())
}

fn client_in_tx(tx: &Transaction<'_>, n: i64) -> Result<McpClient> {
    let mut client = tx
        .query_row(
            &format!("SELECT {} FROM mcp_clients WHERE id = ?1", client_cols()),
            [n],
            client_from_row,
        )
        .optional()?
        .ok_or_else(|| Error::not_found("MCP client", client_id(n)))?;
    load_grant(tx, n, &mut client.grant)?;
    Ok(client)
}

/// A call to write into the audit log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpAuditRecord {
    /// The client, `None` for an unknown token.
    pub client_id: Option<McpClientId>,
    /// The client's name (or what the caller claimed).
    pub client_name: String,
    /// Time (Unix seconds).
    pub at: i64,
    /// Kind of call.
    pub kind: McpCallKind,
    /// Tool, prompt or resource URI.
    pub name: String,
    /// Outcome.
    pub outcome: McpOutcome,
    /// Diagnostic detail.
    pub detail: Option<String>,
}

impl Store {
    /// Registers a client with the hash of its token ([`hash_mcp_token`])
    /// and the token's last characters for display, at `at` (Unix
    /// seconds). Fails with [`Error::Conflict`] when an active client has
    /// the same name.
    pub fn create_mcp_client(
        &self,
        name: &str,
        token_hash: &str,
        token_hint: &str,
        grant: &McpGrant,
        at: i64,
    ) -> Result<McpClient> {
        self.with_tx(|tx| {
            let taken = tx
                .query_row(
                    "SELECT 1 FROM mcp_clients WHERE name = ?1 AND revoked_at IS NULL",
                    [name],
                    |_| Ok(()),
                )
                .optional()?
                .is_some();
            if taken {
                return Err(Error::Conflict(format!(
                    "an MCP client named {name:?} exists"
                )));
            }
            tx.execute(
                "INSERT INTO mcp_clients (name, token_hash, token_hint, created_at)
                 VALUES (?1, ?2, ?3, ?4)",
                params![name, token_hash, token_hint, at],
            )?;
            let n = tx.last_insert_rowid();
            write_grant(tx, n, grant)?;
            client_in_tx(tx, n)
        })
    }

    /// All clients (revoked ones included), oldest first.
    pub fn list_mcp_clients(&self) -> Result<Vec<McpClient>> {
        self.with_tx(|tx| {
            let ids: Vec<i64> = {
                let mut stmt = tx.prepare_cached("SELECT id FROM mcp_clients ORDER BY id")?;
                stmt.query_map([], |r| r.get(0))?
                    .collect::<rusqlite::Result<_>>()?
            };
            ids.into_iter().map(|n| client_in_tx(tx, n)).collect()
        })
    }

    /// One client.
    pub fn get_mcp_client(&self, id: &McpClientId) -> Result<McpClient> {
        let n = parse_client_id(id)?;
        self.with_tx(|tx| client_in_tx(tx, n))
    }

    /// The active (not revoked) client holding a token, by the token's hash.
    pub fn mcp_client_by_token(&self, token_hash: &str) -> Result<Option<McpClient>> {
        self.with_tx(|tx| {
            let n: Option<i64> = tx
                .query_row(
                    "SELECT id FROM mcp_clients WHERE token_hash = ?1 AND revoked_at IS NULL",
                    [token_hash],
                    |r| r.get(0),
                )
                .optional()?;
            n.map(|n| client_in_tx(tx, n)).transpose()
        })
    }

    /// Replaces a client's grant. Fails with [`Error::Conflict`] for a
    /// revoked client.
    pub fn update_mcp_grant(&self, id: &McpClientId, grant: &McpGrant) -> Result<McpClient> {
        let n = parse_client_id(id)?;
        self.with_tx(|tx| {
            let client = client_in_tx(tx, n)?;
            if client.revoked_at.is_some() {
                return Err(Error::Conflict(format!("MCP client {id} is revoked")));
            }
            write_grant(tx, n, grant)?;
            client_in_tx(tx, n)
        })
    }

    /// Replaces a client's rate limits (M4a). Fails with
    /// [`Error::InvalidData`] for limits with issues
    /// ([`McpRateLimits::issues`]) and [`Error::Conflict`] for a revoked
    /// client.
    pub fn set_mcp_rate_limits(
        &self,
        id: &McpClientId,
        limits: &McpRateLimits,
    ) -> Result<McpClient> {
        if !limits.issues().is_empty() {
            return Err(Error::InvalidData(format!(
                "invalid rate limits {limits:?}"
            )));
        }
        let n = parse_client_id(id)?;
        self.with_tx(|tx| {
            let client = client_in_tx(tx, n)?;
            if client.revoked_at.is_some() {
                return Err(Error::Conflict(format!("MCP client {id} is revoked")));
            }
            tx.execute(
                "UPDATE mcp_clients SET rate_limits = ?2 WHERE id = ?1",
                params![n, serde_json::to_string(limits)?],
            )?;
            client_in_tx(tx, n)
        })
    }

    /// The result a client's earlier write call with this idempotency key
    /// returned, if it is younger than `since` (Unix seconds): `(tool,
    /// result)`.
    pub fn mcp_idempotent_result(
        &self,
        id: &McpClientId,
        key: &str,
        since: i64,
    ) -> Result<Option<(String, serde_json::Value)>> {
        let n = parse_client_id(id)?;
        self.with_conn(|conn| {
            let row: Option<(String, String)> = conn
                .query_row(
                    "SELECT tool, nail_open('mcp_idempotency.result', result) FROM mcp_idempotency
                     WHERE client_id = ?1 AND key = ?2 AND created_at >= ?3",
                    params![n, key, since],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?;
            row.map(|(tool, json)| Ok((tool, serde_json::from_str(&json)?)))
                .transpose()
        })
    }

    /// Remembers a write call's result under its idempotency key at `at`
    /// (Unix seconds), replacing an expired one; drops entries older than
    /// `expire_before`.
    pub fn store_mcp_idempotent_result(
        &self,
        id: &McpClientId,
        key: &str,
        tool: &str,
        result: &serde_json::Value,
        at: i64,
        expire_before: i64,
    ) -> Result<()> {
        let n = parse_client_id(id)?;
        self.with_tx(|tx| {
            tx.execute(
                "DELETE FROM mcp_idempotency WHERE created_at < ?1",
                [expire_before],
            )?;
            tx.execute(
                "INSERT INTO mcp_idempotency (client_id, key, tool, result, created_at)
                 VALUES (?1, ?2, ?3, nail_seal('mcp_idempotency.result', ?4), ?5)
                 ON CONFLICT (client_id, key) DO UPDATE SET tool = excluded.tool,
                     result = excluded.result, created_at = excluded.created_at",
                params![n, key, tool, serde_json::to_string(result)?, at],
            )?;
            Ok(())
        })
    }

    /// Records that a client made a call at `at` (Unix seconds).
    pub fn touch_mcp_client(&self, id: &McpClientId, at: i64) -> Result<()> {
        let n = parse_client_id(id)?;
        self.with_conn(|conn| {
            conn.execute(
                "UPDATE mcp_clients SET last_used_at = ?2 WHERE id = ?1",
                params![n, at],
            )?;
            Ok(())
        })
    }

    /// Revokes a client at `at` (Unix seconds): its token no longer works
    /// and its grant is cleared; the row and its audit entries stay.
    /// Revoking twice is a no-op.
    pub fn revoke_mcp_client(&self, id: &McpClientId, at: i64) -> Result<McpClient> {
        let n = parse_client_id(id)?;
        self.with_tx(|tx| {
            let client = client_in_tx(tx, n)?;
            if client.revoked_at.is_none() {
                tx.execute(
                    "UPDATE mcp_clients SET revoked_at = ?2 WHERE id = ?1",
                    params![n, at],
                )?;
                write_grant(tx, n, &McpGrant::default())?;
            }
            client_in_tx(tx, n)
        })
    }

    /// Appends an audit entry; returns its id.
    pub fn append_mcp_audit(&self, record: &McpAuditRecord) -> Result<u64> {
        let client = record.client_id.as_ref().map(parse_client_id).transpose()?;
        let kind = enum_str(record.kind)?;
        let outcome = enum_str(record.outcome)?;
        self.with_conn(|conn| {
            conn.execute(
                "INSERT INTO mcp_audit (client_id, client_name, at, kind, name, outcome, detail)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    client,
                    record.client_name,
                    record.at,
                    kind,
                    record.name,
                    outcome,
                    record.detail
                ],
            )?;
            Ok(u64::try_from(conn.last_insert_rowid()).unwrap_or_default())
        })
    }

    /// A page of the audit log, newest first: entries of one client (or
    /// all) with an id below `before` (or the newest).
    pub fn list_mcp_audit(
        &self,
        client: Option<&McpClientId>,
        before: Option<u64>,
        limit: u32,
    ) -> Result<Vec<McpAuditEntry>> {
        let client = client.map(parse_client_id).transpose()?;
        let before = before.map(|b| i64::try_from(b).unwrap_or(i64::MAX));
        self.with_conn(|conn| {
            let mut stmt = conn.prepare_cached(&format!(
                "SELECT id, client_id, client_name, strftime({RFC3339}, at, 'unixepoch'),
                        kind, name, outcome, detail
                 FROM mcp_audit
                 WHERE (?1 IS NULL OR client_id = ?1) AND (?2 IS NULL OR id < ?2)
                 ORDER BY id DESC LIMIT ?3"
            ))?;
            let rows = stmt.query_map(params![client, before, limit], |r| {
                Ok(McpAuditEntry {
                    id: r.get::<_, i64>(0)?.try_into().unwrap_or_default(),
                    client_id: r.get::<_, Option<i64>>(1)?.map(client_id),
                    client_name: r.get(2)?,
                    at: r.get(3)?,
                    kind: enum_from(4, r.get(4)?)?,
                    name: r.get(5)?,
                    outcome: enum_from(6, r.get(6)?)?,
                    detail: r.get(7)?,
                    localized: None,
                })
            })?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
    }

    /// Deletes audit entries older than `before` (Unix seconds). Returns
    /// how many were deleted.
    pub fn purge_mcp_audit(&self, before: i64) -> Result<usize> {
        self.with_conn(|conn| Ok(conn.execute("DELETE FROM mcp_audit WHERE at < ?1", [before])?))
    }
}

impl Store {
    /// The MCP server settings (`MCP_SETTINGS_KEY` in `app_settings`;
    /// defaults when never saved): the fixed port the server starts on
    /// (M2c WP53, `nail_mcp::configured_port`).
    pub fn mcp_settings(&self) -> Result<nail_api::mcp::McpSettings> {
        Ok(self.setting(crate::MCP_SETTINGS_KEY)?.unwrap_or_default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grant() -> McpGrant {
        McpGrant {
            scopes: vec![McpScope::Context, McpScope::Read],
            all_accounts: false,
            accounts: vec!["work".into()],
            excluded_folders: vec!["work/secret".into()],
            skip_draft_confirmation: true,
        }
    }

    #[test]
    fn clients_store_only_token_hashes() {
        let store = Store::open_in_memory().unwrap();
        let hash = hash_mcp_token("tok-123");
        assert_eq!(hash.len(), 64);
        assert_ne!(hash, hash_mcp_token("tok-124"));
        let client = store
            .create_mcp_client("claude", &hash, "-123", &McpGrant::default(), 0)
            .unwrap();
        assert_eq!(client.id.as_str(), "mcp-1");
        assert_eq!(client.created_at, "1970-01-01T00:00:00Z");
        assert!(client.grant.is_empty());
        assert_eq!(client.last_used_at, None);
        let stored: i64 = store
            .with_conn(|c| {
                Ok(c.query_row(
                    "SELECT COUNT(*) FROM mcp_clients WHERE token_hash LIKE '%tok%'",
                    [],
                    |r| r.get(0),
                )?)
            })
            .unwrap();
        assert_eq!(stored, 0);
        assert!(matches!(
            store.create_mcp_client("claude", "other", "x", &McpGrant::default(), 1),
            Err(Error::Conflict(_))
        ));

        let found = store.mcp_client_by_token(&hash).unwrap().unwrap();
        assert_eq!(found.id, client.id);
        assert!(store.mcp_client_by_token("nope").unwrap().is_none());

        let updated = store.update_mcp_grant(&client.id, &grant()).unwrap();
        assert_eq!(updated.grant.scopes, [McpScope::Read, McpScope::Context]);
        assert_eq!(updated.grant.accounts, [AccountId::from("work")]);
        assert!(updated.grant.skip_draft_confirmation);
        store.touch_mcp_client(&client.id, 60).unwrap();
        assert_eq!(
            store
                .get_mcp_client(&client.id)
                .unwrap()
                .last_used_at
                .as_deref(),
            Some("1970-01-01T00:01:00Z")
        );

        let revoked = store.revoke_mcp_client(&client.id, 120).unwrap();
        assert!(revoked.revoked_at.is_some());
        assert!(revoked.grant.is_empty());
        assert!(store.mcp_client_by_token(&hash).unwrap().is_none());
        assert!(matches!(
            store.update_mcp_grant(&client.id, &grant()),
            Err(Error::Conflict(_))
        ));
        // The name is free again; the revoked client stays listed.
        store
            .create_mcp_client("claude", "h2", "x", &McpGrant::default(), 130)
            .unwrap();
        assert_eq!(store.list_mcp_clients().unwrap().len(), 2);
        assert!(
            store
                .get_mcp_client(&"mcp-9".into())
                .unwrap_err()
                .is_not_found()
        );
    }

    #[test]
    fn audit_log_pages_newest_first() {
        let store = Store::open_in_memory().unwrap();
        let client = store
            .create_mcp_client("a", "h", "x", &McpGrant::default(), 0)
            .unwrap();
        for i in 0..5 {
            store
                .append_mcp_audit(&McpAuditRecord {
                    client_id: (i % 2 == 0).then(|| client.id.clone()),
                    client_name: "a".into(),
                    at: i,
                    kind: McpCallKind::Tool,
                    name: format!("tool-{i}"),
                    outcome: if i == 4 {
                        McpOutcome::Denied
                    } else {
                        McpOutcome::Allowed
                    },
                    detail: (i == 4).then(|| "missing scope read".into()),
                })
                .unwrap();
        }
        let page = store.list_mcp_audit(None, None, 2).unwrap();
        assert_eq!(
            page.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(),
            ["tool-4", "tool-3"]
        );
        assert_eq!(page[0].outcome, McpOutcome::Denied);
        assert_eq!(page[0].client_id.as_ref(), Some(&client.id));
        assert_eq!(page[1].client_id, None);
        let next = store.list_mcp_audit(None, Some(page[1].id), 10).unwrap();
        assert_eq!(next.len(), 3);
        let own = store.list_mcp_audit(Some(&client.id), None, 10).unwrap();
        assert_eq!(own.len(), 3);
        assert_eq!(store.purge_mcp_audit(2).unwrap(), 2);
        assert_eq!(store.list_mcp_audit(None, None, 10).unwrap().len(), 3);
    }

    #[test]
    fn rate_limits_and_idempotency_keys() {
        let store = Store::open_in_memory().unwrap();
        let client = store
            .create_mcp_client("agent", &hash_mcp_token("t"), "t", &grant(), 100)
            .unwrap();
        assert_eq!(client.rate_limits, McpRateLimits::default());
        let limits = McpRateLimits {
            requests_per_minute: Some(60),
            writes_per_minute: None,
            sends_per_hour: Some(2),
        };
        let saved = store.set_mcp_rate_limits(&client.id, &limits).unwrap();
        assert_eq!(saved.rate_limits, limits);
        assert_eq!(
            store.get_mcp_client(&client.id).unwrap().rate_limits,
            limits
        );
        assert!(matches!(
            store.set_mcp_rate_limits(
                &client.id,
                &McpRateLimits {
                    requests_per_minute: Some(0),
                    ..limits
                }
            ),
            Err(Error::InvalidData(_))
        ));

        assert_eq!(
            store.mcp_idempotent_result(&client.id, "k1", 0).unwrap(),
            None
        );
        let result = serde_json::json!({"status": "done", "op_ids": ["op-1"]});
        store
            .store_mcp_idempotent_result(&client.id, "k1", "update_messages", &result, 1000, 0)
            .unwrap();
        assert_eq!(
            store.mcp_idempotent_result(&client.id, "k1", 900).unwrap(),
            Some(("update_messages".to_owned(), result.clone()))
        );
        // Too old for the caller's window.
        assert_eq!(
            store.mcp_idempotent_result(&client.id, "k1", 1001).unwrap(),
            None
        );
        // Storing later expires the old entry.
        store
            .store_mcp_idempotent_result(&client.id, "k2", "send_message", &result, 5000, 2000)
            .unwrap();
        assert_eq!(
            store.mcp_idempotent_result(&client.id, "k1", 0).unwrap(),
            None
        );
        assert!(
            store
                .mcp_idempotent_result(&client.id, "k2", 0)
                .unwrap()
                .is_some()
        );

        store.revoke_mcp_client(&client.id, 6000).unwrap();
        assert!(matches!(
            store.set_mcp_rate_limits(&client.id, &limits),
            Err(Error::Conflict(_))
        ));
    }
}
