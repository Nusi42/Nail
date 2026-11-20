//! Values for the shared pickers (R2, ADR 0222): the tags in use and the
//! newest stored raw messages (whose `List-Id`s and header names the core
//! reads).

use rusqlite::params;

use crate::addresses::MAX_PREFIX;
use crate::blob::BlobHash;
use crate::error::Result;
use crate::store::Store;

impl Store {
    /// Up to `limit` tags that start with `prefix` (case-insensitive; empty
    /// = every tag), with how many messages carry each; most used first.
    pub fn suggest_tags(&self, prefix: &str, limit: u32) -> Result<Vec<(String, u32)>> {
        let prefix: String = prefix.trim().chars().take(MAX_PREFIX).collect();
        if limit == 0 {
            return Ok(Vec::new());
        }
        let pattern = format!("{}%", crate::util::escape_like(&prefix));
        self.with_conn(|conn| {
            let mut stmt = conn.prepare_cached(
                "SELECT tag, COUNT(*) AS uses FROM message_tags
                 WHERE tag LIKE ?1 ESCAPE '\\'
                 GROUP BY tag ORDER BY uses DESC, tag LIMIT ?2",
            )?;
            let rows = stmt.query_map(params![pattern, limit], |r| Ok((r.get(0)?, r.get(1)?)))?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
    }

    /// The blob hashes of the `limit` newest messages whose raw form is
    /// stored, newest first.
    pub fn recent_blob_hashes(&self, limit: u32) -> Result<Vec<BlobHash>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare_cached(
                "SELECT blob_hash FROM messages WHERE blob_hash IS NOT NULL
                 ORDER BY date_sort DESC, id DESC LIMIT ?1",
            )?;
            let rows = stmt
                .query_map([limit], |r| r.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            rows.iter().map(|h| BlobHash::parse(h)).collect()
        })
    }
}

#[cfg(test)]
mod tests {
    use nail_api::testing;
    use nail_api::{AccountKind, FolderRole, MessageSummary};

    use crate::test_store::{add, store_with};

    #[test]
    fn tags_are_ranked_by_use_and_filtered_by_prefix() {
        let store = store_with(
            &testing::account("a1", AccountKind::Imap),
            &[testing::folder("inbox", "a1", "INBOX", FolderRole::Inbox)],
        );
        for (id, tags) in [
            ("m1", vec!["work", "Project"]),
            ("m2", vec!["project"]),
            ("m3", vec!["project", "home"]),
        ] {
            add(
                &store,
                MessageSummary {
                    tags: tags.into_iter().map(Into::into).collect(),
                    ..testing::summary(id, "a1", "inbox")
                },
            );
        }
        let all = store.suggest_tags("", 10).unwrap();
        assert_eq!(all[0], ("project".to_owned(), 2));
        assert_eq!(all.len(), 4);
        let p = store.suggest_tags("PRO", 10).unwrap();
        assert_eq!(
            p.iter().map(|(t, _)| t.as_str()).collect::<Vec<_>>(),
            ["project", "Project"]
        );
        assert_eq!(store.suggest_tags("x", 10).unwrap(), Vec::new());
        assert_eq!(store.suggest_tags("", 0).unwrap(), Vec::new());
        assert_eq!(store.recent_blob_hashes(5).unwrap(), Vec::new());
    }
}
