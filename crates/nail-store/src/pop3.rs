//! POP3 bookkeeping (schema v3): which server messages (`UIDL`s) an account
//! downloaded, when, into which local message, and whether the user deleted
//! the local copy. The core feeds this into `nail_proto_pop3::policy` to
//! decide what to download and what to delete on the server.

use std::collections::{HashMap, HashSet};

use nail_api::{AccountId, MessageId};
use rusqlite::{OptionalExtension, params};

use crate::error::Result;
use crate::store::Store;

/// One downloaded POP3 message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pop3Uid {
    /// The server's unique id (`UIDL`).
    pub uidl: String,
    /// The local message; `None` once the user deleted it.
    pub message_id: Option<MessageId>,
    /// Download time (Unix seconds).
    pub downloaded_at: i64,
    /// Whether the local copy was deleted by the user.
    pub deleted_locally: bool,
}

/// An account's POP3 bookkeeping in the shapes the policy functions take.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Pop3State {
    /// Every known `UIDL` (downloaded, whether or not still local).
    pub known: HashSet<String>,
    /// `UIDL` → download time.
    pub downloaded: HashMap<String, i64>,
    /// `UIDL`s whose local copy the user deleted.
    pub locally_deleted: HashSet<String>,
}

impl Store {
    /// All downloaded POP3 messages of an account, by `UIDL`.
    pub fn pop3_uids(&self, account: &AccountId) -> Result<Vec<Pop3Uid>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare_cached(
                "SELECT uidl, message_id, downloaded_at, deleted_locally FROM pop3_uids
                 WHERE account_id = ?1 ORDER BY uidl",
            )?;
            let rows = stmt.query_map([account.as_str()], |r| {
                Ok(Pop3Uid {
                    uidl: r.get(0)?,
                    message_id: r.get::<_, Option<String>>(1)?.map(MessageId),
                    downloaded_at: r.get(2)?,
                    deleted_locally: r.get(3)?,
                })
            })?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
    }

    /// The account's bookkeeping as sets and maps (see [`Pop3State`]).
    pub fn pop3_state(&self, account: &AccountId) -> Result<Pop3State> {
        let mut state = Pop3State::default();
        for row in self.pop3_uids(account)? {
            state.downloaded.insert(row.uidl.clone(), row.downloaded_at);
            if row.deleted_locally {
                state.locally_deleted.insert(row.uidl.clone());
            }
            state.known.insert(row.uidl);
        }
        Ok(state)
    }

    /// Records a downloaded message (replacing an earlier record).
    pub fn pop3_record(
        &self,
        account: &AccountId,
        uidl: &str,
        message: &MessageId,
        downloaded_at: i64,
    ) -> Result<()> {
        self.with_conn(|conn| {
            conn.execute(
                "INSERT INTO pop3_uids (account_id, uidl, message_id, downloaded_at)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT (account_id, uidl) DO UPDATE SET
                     message_id = excluded.message_id,
                     downloaded_at = excluded.downloaded_at,
                     deleted_locally = 0",
                params![account.as_str(), uidl, message.as_str(), downloaded_at],
            )?;
            Ok(())
        })
    }

    /// Marks the POP3 message behind a local message as deleted by the user.
    /// Returns its `UIDL`, `None` when the message did not come from POP3.
    pub fn pop3_mark_deleted(&self, message: &MessageId) -> Result<Option<String>> {
        self.with_tx(|tx| {
            let uidl: Option<String> = tx
                .query_row(
                    "SELECT uidl FROM pop3_uids WHERE message_id = ?1",
                    [message.as_str()],
                    |r| r.get(0),
                )
                .optional()?;
            if uidl.is_some() {
                tx.execute(
                    "UPDATE pop3_uids SET deleted_locally = 1, message_id = NULL
                     WHERE message_id = ?1",
                    [message.as_str()],
                )?;
            }
            Ok(uidl)
        })
    }

    /// Forgets `UIDL`s (gone from the server). Returns how many rows went.
    pub fn pop3_forget(&self, account: &AccountId, uidls: &[String]) -> Result<usize> {
        self.with_tx(|tx| {
            let mut stmt =
                tx.prepare_cached("DELETE FROM pop3_uids WHERE account_id = ?1 AND uidl = ?2")?;
            let mut removed = 0;
            for uidl in uidls {
                removed += stmt.execute(params![account.as_str(), uidl])?;
            }
            Ok(removed)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nail_api::{Account, AccountKind};

    fn store() -> Store {
        let store = Store::open_in_memory().unwrap();
        store
            .upsert_account(&Account {
                id: "home".into(),
                name: "Home".into(),
                email: "me@home.example".into(),
                kind: AccountKind::Pop3,
            })
            .unwrap();
        store
    }

    #[test]
    fn records_marks_and_forgets() {
        let store = store();
        let home = AccountId::new("home");
        store
            .pop3_record(&home, "u1", &MessageId::new("m1"), 100)
            .unwrap();
        store
            .pop3_record(&home, "u2", &MessageId::new("m2"), 200)
            .unwrap();
        let state = store.pop3_state(&home).unwrap();
        assert_eq!(state.known.len(), 2);
        assert_eq!(state.downloaded["u2"], 200);
        assert!(state.locally_deleted.is_empty());

        assert_eq!(
            store.pop3_mark_deleted(&MessageId::new("m1")).unwrap(),
            Some("u1".to_owned())
        );
        assert_eq!(store.pop3_mark_deleted(&MessageId::new("x")).unwrap(), None);
        let state = store.pop3_state(&home).unwrap();
        assert!(state.locally_deleted.contains("u1"));
        assert!(state.known.contains("u1"), "a deleted copy stays known");
        let rows = store.pop3_uids(&home).unwrap();
        assert_eq!(rows[0].message_id, None);
        assert!(rows[0].deleted_locally);

        assert_eq!(store.pop3_forget(&home, &["u1".into()]).unwrap(), 1);
        assert_eq!(store.pop3_state(&home).unwrap().known.len(), 1);
    }

    #[test]
    fn rows_go_with_the_account() {
        let store = store();
        let home = AccountId::new("home");
        store
            .pop3_record(&home, "u1", &MessageId::new("m1"), 1)
            .unwrap();
        store.delete_account(&home).unwrap();
        assert_eq!(store.pop3_uids(&home).unwrap().len(), 0);
    }
}
