//! `users.history.list`: incremental changes since a `historyId`, folded
//! into added / changed / removed messages. A `404` means the start id is
//! older than Gmail keeps history (about a week, sometimes less): the
//! caller resyncs fully ([`nail_api::engine::DeltaOutcome::CursorExpired`]).

use std::collections::{HashMap, HashSet};

use reqwest::Method;

use crate::client::GmailClient;
use crate::error::GmailError;
use crate::wire::{History, ListHistoryResponse};

/// Records per history page (Gmail's maximum).
pub const HISTORY_PAGE_SIZE: u32 = 500;

/// One page of `history.list`, or the news that the start id expired.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HistoryPage {
    /// The page.
    Page(ListHistoryResponse),
    /// `404`: the start id is too old; resync fully.
    Expired,
}

impl GmailClient {
    /// `history.list` from `start_history_id` (all history types).
    pub async fn list_history(
        &self,
        start_history_id: &str,
        page_token: Option<&str>,
        max_results: u32,
    ) -> Result<HistoryPage, GmailError> {
        let mut query = vec![
            ("startHistoryId", start_history_id.to_owned()),
            (
                "maxResults",
                max_results.clamp(1, HISTORY_PAGE_SIZE).to_string(),
            ),
        ];
        for kind in [
            "messageAdded",
            "messageDeleted",
            "labelAdded",
            "labelRemoved",
        ] {
            query.push(("historyTypes", kind.to_owned()));
        }
        if let Some(token) = page_token {
            query.push(("pageToken", token.to_owned()));
        }
        match self
            .json(
                Method::GET,
                &["history"],
                &query,
                None::<&()>,
                true,
                "users.history.list",
            )
            .await
        {
            Ok(page) => Ok(HistoryPage::Page(page)),
            Err(GmailError::NotFound(_)) => Ok(HistoryPage::Expired),
            Err(e) => Err(e),
        }
    }
}

/// History records folded into their net effect.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HistoryFold {
    /// New messages, in the order they arrived (not deleted again).
    pub added: Vec<String>,
    /// Known messages whose labels changed: their labels after the last
    /// change, or `None` when the records did not carry them.
    pub changed: Vec<(String, Option<Vec<String>>)>,
    /// Messages deleted permanently.
    pub removed: Vec<String>,
    /// Thread ids seen in the records.
    pub threads: HashMap<String, String>,
}

/// Folds history records (oldest first) into their net effect: a message
/// added and deleted in the same window is only removed; label changes of
/// a new message do not make it "changed" (it is fetched anyway).
pub fn fold_history(records: &[History]) -> HistoryFold {
    let mut added: Vec<String> = Vec::new();
    let mut added_set: HashSet<String> = HashSet::new();
    let mut removed: Vec<String> = Vec::new();
    let mut removed_set: HashSet<String> = HashSet::new();
    let mut changed_order: Vec<String> = Vec::new();
    let mut changed: HashMap<String, Option<Vec<String>>> = HashMap::new();
    let mut threads = HashMap::new();
    for record in records {
        for entry in &record.messages_added {
            let m = &entry.message;
            if m.id.is_empty() {
                continue;
            }
            if !m.thread_id.is_empty() {
                threads.insert(m.id.clone(), m.thread_id.clone());
            }
            if removed_set.remove(&m.id) {
                removed.retain(|r| r != &m.id);
            }
            changed.remove(&m.id);
            if added_set.insert(m.id.clone()) {
                added.push(m.id.clone());
            }
        }
        let label_entries = record
            .labels_added
            .iter()
            .map(|e| (e, true))
            .chain(record.labels_removed.iter().map(|e| (e, false)));
        for (entry, adding) in label_entries {
            let m = &entry.message;
            if m.id.is_empty() || removed_set.contains(&m.id) || added_set.contains(&m.id) {
                continue;
            }
            if !m.thread_id.is_empty() {
                threads.insert(m.id.clone(), m.thread_id.clone());
            }
            let labels = if m.label_ids.is_empty() {
                // Records without the message's labels: apply the change to
                // what an earlier record said, else fetch.
                changed.get(&m.id).cloned().flatten().map(|mut known| {
                    if adding {
                        for l in &entry.label_ids {
                            if !known.contains(l) {
                                known.push(l.clone());
                            }
                        }
                    } else {
                        known.retain(|l| !entry.label_ids.contains(l));
                    }
                    known
                })
            } else {
                Some(m.label_ids.clone())
            };
            if !changed.contains_key(&m.id) {
                changed_order.push(m.id.clone());
            }
            changed.insert(m.id.clone(), labels);
        }
        for entry in &record.messages_deleted {
            let id = &entry.message.id;
            if id.is_empty() {
                continue;
            }
            if added_set.remove(id) {
                added.retain(|a| a != id);
            }
            changed.remove(id);
            if removed_set.insert(id.clone()) {
                removed.push(id.clone());
            }
        }
    }
    let changed = changed_order
        .into_iter()
        .filter_map(|id| changed.remove(&id).map(|labels| (id, labels)))
        .collect();
    HistoryFold {
        added,
        changed,
        removed,
        threads,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::ListHistoryResponse;

    fn recorded() -> ListHistoryResponse {
        serde_json::from_str(include_str!("../fixtures/responses/history-list.json")).unwrap()
    }

    #[test]
    fn folds_a_recorded_history_page() {
        let page = recorded();
        assert_eq!(page.history_id, "4300");
        let fold = fold_history(&page.history);
        // 18c1 was added and starred; 18c2 was added then deleted.
        assert_eq!(fold.added, ["18c1000000000001"]);
        assert_eq!(fold.removed, ["18c2000000000002", "18b0000000000009"]);
        assert_eq!(
            fold.changed,
            [(
                "18c0a1b2c3d4e5f6".to_owned(),
                Some(vec!["CATEGORY_UPDATES".to_owned(), "Label_7".to_owned()])
            )]
        );
        assert_eq!(fold.threads["18c1000000000001"], "18c0a1b2c3d4e5f6");
    }

    #[test]
    fn label_changes_without_labels_build_on_earlier_records() {
        let records: Vec<History> = serde_json::from_value(serde_json::json!([
            {"id": "10", "labelsRemoved": [{"message": {"id": "m", "labelIds": ["INBOX", "UNREAD"]}, "labelIds": ["STARRED"]}]},
            {"id": "11", "labelsRemoved": [{"message": {"id": "m"}, "labelIds": ["UNREAD"]}]},
            {"id": "12", "labelsAdded": [{"message": {"id": "n"}, "labelIds": ["STARRED"]}]},
        ]))
        .unwrap();
        let fold = fold_history(&records);
        assert_eq!(
            fold.changed,
            [
                ("m".to_owned(), Some(vec!["INBOX".to_owned()])),
                ("n".to_owned(), None)
            ]
        );
        assert!(fold.added.is_empty() && fold.removed.is_empty());
    }

    #[test]
    fn delete_then_nothing_else() {
        let records: Vec<History> = serde_json::from_value(serde_json::json!([
            {"id": "1", "labelsAdded": [{"message": {"id": "m", "labelIds": ["TRASH"]}, "labelIds": ["TRASH"]}]},
            {"id": "2", "messagesDeleted": [{"message": {"id": "m"}}]},
            {"id": "3", "labelsAdded": [{"message": {"id": "m", "labelIds": ["X"]}, "labelIds": ["X"]}]},
        ]))
        .unwrap();
        let fold = fold_history(&records);
        assert_eq!(fold.removed, ["m"]);
        assert!(fold.changed.is_empty());
    }
}
