//! Collected addresses (schema v4): every address the user sent to or
//! received from, ranked by use, for the composer's autocomplete.

use nail_api::Address;
use rusqlite::params;

use crate::error::Result;
use crate::store::Store;

/// Longest prefix [`Store::suggest_addresses`] accepts; longer input is cut.
const MAX_PREFIX: usize = 256;

impl Store {
    /// Records addresses as used at `at` (Unix seconds): new ones are
    /// added, known ones count one more use. A non-empty display name
    /// replaces the stored one. Empty addresses are skipped.
    pub fn record_addresses(&self, addresses: &[Address], at: i64) -> Result<()> {
        self.with_tx(|tx| {
            let mut stmt = tx.prepare_cached(
                "INSERT INTO collected_addresses (email, name, use_count, last_used)
                 VALUES (?1, ?2, 1, ?3)
                 ON CONFLICT (email) DO UPDATE SET
                     name = COALESCE(excluded.name, name),
                     use_count = use_count + 1,
                     last_used = MAX(last_used, excluded.last_used)",
            )?;
            for address in addresses {
                let email = address.email.trim();
                if email.is_empty() {
                    continue;
                }
                let name = address
                    .name
                    .as_deref()
                    .map(str::trim)
                    .filter(|n| !n.is_empty());
                stmt.execute(params![email, name, at])?;
            }
            Ok(())
        })
    }

    /// Up to `limit` addresses whose address or display name (or a word of
    /// it) starts with `prefix`, case-insensitive; most used first, then
    /// most recent.
    pub fn suggest_addresses(&self, prefix: &str, limit: u32) -> Result<Vec<Address>> {
        let prefix: String = prefix.trim().chars().take(MAX_PREFIX).collect();
        if prefix.is_empty() || limit == 0 {
            return Ok(Vec::new());
        }
        let pattern = format!("{}%", crate::util::escape_like(&prefix));
        let word = format!("% {}%", crate::util::escape_like(&prefix));
        self.with_conn(|conn| {
            let mut stmt = conn.prepare_cached(
                "SELECT email, name FROM collected_addresses
                 WHERE email LIKE ?1 ESCAPE '\\' OR name LIKE ?1 ESCAPE '\\'
                    OR name LIKE ?2 ESCAPE '\\'
                 ORDER BY use_count DESC, last_used DESC, email
                 LIMIT ?3",
            )?;
            let rows = stmt.query_map(params![pattern, word, limit], |r| {
                Ok(Address {
                    email: r.get(0)?,
                    name: r.get(1)?,
                })
            })?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn addr(name: Option<&str>, email: &str) -> Address {
        Address {
            name: name.map(str::to_owned),
            email: email.into(),
        }
    }

    #[test]
    fn records_and_ranks() {
        let store = Store::open_in_memory().unwrap();
        store
            .record_addresses(
                &[
                    addr(Some("Bob Builder"), "bob@example.org"),
                    addr(None, "bella@example.org"),
                    addr(None, " "),
                ],
                10,
            )
            .unwrap();
        store
            .record_addresses(&[addr(None, "BOB@example.org")], 20)
            .unwrap();
        let hits = store.suggest_addresses("b", 10).unwrap();
        assert_eq!(
            hits,
            [
                addr(Some("Bob Builder"), "bob@example.org"),
                addr(None, "bella@example.org")
            ]
        );
        // A word of the name matches too; the limit applies.
        assert_eq!(
            store.suggest_addresses("build", 10).unwrap()[0].email,
            "bob@example.org"
        );
        assert_eq!(store.suggest_addresses("B", 1).unwrap().len(), 1);
        assert_eq!(store.suggest_addresses("", 10).unwrap().len(), 0);
        assert_eq!(store.suggest_addresses("z", 10).unwrap().len(), 0);
    }

    #[test]
    fn like_wildcards_are_literal() {
        let store = Store::open_in_memory().unwrap();
        store
            .record_addresses(
                &[addr(None, "a_b@example.org"), addr(None, "axb@example.org")],
                1,
            )
            .unwrap();
        let hits = store.suggest_addresses("a_", 10).unwrap();
        assert_eq!(hits, [addr(None, "a_b@example.org")]);
        assert_eq!(store.suggest_addresses("%", 10).unwrap().len(), 0);
    }
}
