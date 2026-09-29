//! Saved searches (schema v4): named queries shown as smart folders. Ids
//! are `search-<n>`. The query text is stored as given; the core checks it
//! parses before saving.

use nail_api::search::{SavedSearch, SearchSort};
use nail_api::{AccountId, FolderId, SavedSearchId};
use rusqlite::{OptionalExtension, Row, Transaction, params};

use crate::error::{Error, Result};
use crate::store::Store;

fn search_id(n: i64) -> SavedSearchId {
    SavedSearchId(format!("search-{n}"))
}

fn parse_search_id(id: &SavedSearchId) -> Result<i64> {
    id.as_str()
        .strip_prefix("search-")
        .and_then(|n| n.parse().ok())
        .ok_or_else(|| Error::not_found("saved search", id))
}

fn sort_str(sort: SearchSort) -> &'static str {
    match sort {
        SearchSort::Date => "date",
        SearchSort::Relevance => "relevance",
    }
}

const COLS: &str = "id, name, query, account_id, folder_id, sort, position";

fn from_row(row: &Row<'_>) -> rusqlite::Result<SavedSearch> {
    let sort: String = row.get(5)?;
    Ok(SavedSearch {
        id: Some(search_id(row.get(0)?)),
        name: row.get(1)?,
        query: row.get(2)?,
        account_id: row.get::<_, Option<String>>(3)?.map(AccountId),
        folder_id: row.get::<_, Option<String>>(4)?.map(FolderId),
        sort: if sort == "relevance" {
            SearchSort::Relevance
        } else {
            SearchSort::Date
        },
        position: row.get(6)?,
    })
}

fn get_in_tx(tx: &Transaction<'_>, n: i64) -> Result<SavedSearch> {
    tx.query_row(
        &format!("SELECT {COLS} FROM saved_searches WHERE id = ?1"),
        [n],
        from_row,
    )
    .optional()?
    .ok_or_else(|| Error::not_found("saved search", search_id(n)))
}

impl Store {
    /// Saved searches by position.
    pub fn list_saved_searches(&self) -> Result<Vec<SavedSearch>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare_cached(&format!(
                "SELECT {COLS} FROM saved_searches ORDER BY position, id"
            ))?;
            let rows = stmt.query_map([], from_row)?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
    }

    /// Creates (`id: None`, placed last) or replaces a saved search.
    /// Returns it as stored.
    pub fn save_saved_search(&self, search: &SavedSearch) -> Result<SavedSearch> {
        if search.name.trim().is_empty() {
            return Err(Error::InvalidData("a saved search needs a name".into()));
        }
        self.with_tx(|tx| {
            let n = match &search.id {
                None => {
                    tx.execute(
                        "INSERT INTO saved_searches (name, query, account_id, folder_id, sort, position)
                         VALUES (?1, ?2, ?3, ?4, ?5,
                                 (SELECT COALESCE(MAX(position) + 1, 0) FROM saved_searches))",
                        params![
                            search.name,
                            search.query,
                            search.account_id.as_ref().map(AccountId::as_str),
                            search.folder_id.as_ref().map(FolderId::as_str),
                            sort_str(search.sort)
                        ],
                    )?;
                    tx.last_insert_rowid()
                }
                Some(id) => {
                    let n = parse_search_id(id)?;
                    let changed = tx.execute(
                        "UPDATE saved_searches SET name = ?2, query = ?3, account_id = ?4,
                             folder_id = ?5, sort = ?6, position = ?7
                         WHERE id = ?1",
                        params![
                            n,
                            search.name,
                            search.query,
                            search.account_id.as_ref().map(AccountId::as_str),
                            search.folder_id.as_ref().map(FolderId::as_str),
                            sort_str(search.sort),
                            search.position
                        ],
                    )?;
                    if changed == 0 {
                        return Err(Error::not_found("saved search", id));
                    }
                    n
                }
            };
            get_in_tx(tx, n)
        })
    }

    /// Deletes a saved search. Returns whether it existed.
    pub fn delete_saved_search(&self, id: &SavedSearchId) -> Result<bool> {
        let n = parse_search_id(id)?;
        self.with_conn(|conn| {
            Ok(conn.execute("DELETE FROM saved_searches WHERE id = ?1", [n])? > 0)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn search(name: &str, query: &str) -> SavedSearch {
        SavedSearch {
            id: None,
            name: name.into(),
            query: query.into(),
            account_id: None,
            folder_id: None,
            sort: SearchSort::Date,
            position: 0,
        }
    }

    #[test]
    fn crud_and_order() {
        let store = Store::open_in_memory().unwrap();
        let unread = store
            .save_saved_search(&search("Unread", "is:unread"))
            .unwrap();
        assert_eq!(unread.id.as_ref().unwrap().as_str(), "search-1");
        assert_eq!(unread.position, 0);
        let mut flagged = store
            .save_saved_search(&SavedSearch {
                account_id: Some("work".into()),
                sort: SearchSort::Relevance,
                ..search("Flagged", "is:flagged")
            })
            .unwrap();
        assert_eq!(flagged.position, 1);
        assert_eq!(flagged.sort, SearchSort::Relevance);

        // Moving "Flagged" first.
        flagged.position = 0;
        let mut first = unread.clone();
        first.position = 1;
        store.save_saved_search(&flagged).unwrap();
        store.save_saved_search(&first).unwrap();
        let names: Vec<String> = store
            .list_saved_searches()
            .unwrap()
            .into_iter()
            .map(|s| s.name)
            .collect();
        assert_eq!(names, ["Flagged", "Unread"]);

        assert!(
            store
                .delete_saved_search(unread.id.as_ref().unwrap())
                .unwrap()
        );
        assert!(
            !store
                .delete_saved_search(unread.id.as_ref().unwrap())
                .unwrap()
        );
        assert!(store.save_saved_search(&unread).unwrap_err().is_not_found());
        assert!(matches!(
            store.save_saved_search(&search(" ", "x")),
            Err(Error::InvalidData(_))
        ));
        assert_eq!(store.list_saved_searches().unwrap().len(), 1);
    }
}
