//! Gmail labels ↔ Nail folders, flags, tags and categories (ADR 0065):
//! the folder follows from the labels ([`GmailFolder::for_labels`]),
//! `UNREAD` / `STARRED` / `DRAFT` are flags, user labels are tags, the
//! `CATEGORY_*` label is the message's [`GmailCategory`].

use nail_api::domain::Flags;
use nail_api::engine::{RemoteFolder, RemoteMessage, RemoteMessageChange};
use nail_api::gmail::{GmailCategory, GmailFolder, GmailMessageInfo, system};

use crate::labels::LabelTable;
use crate::wire::Message;

/// The folders of a Gmail account, in display order. Their names are the
/// stable paths (`INBOX`, `ARCHIVE`, …): the UI shows the folders by their
/// role in the user's language.
pub fn remote_folders() -> Vec<RemoteFolder> {
    GmailFolder::ALL
        .into_iter()
        .map(|folder| RemoteFolder {
            remote_id: folder.path().to_owned(),
            name: folder.path().to_owned(),
            role: folder.role(),
            parent: None,
        })
        .collect()
}

fn has(labels: &[String], id: &str) -> bool {
    labels.iter().any(|l| l == id)
}

/// The system flags of a message with `labels`. Gmail has no `\Answered`.
pub fn flags_for_labels(labels: &[String]) -> Flags {
    Flags {
        seen: !has(labels, system::UNREAD),
        flagged: has(labels, system::STARRED),
        answered: false,
        draft: has(labels, system::DRAFT),
    }
}

/// Whether a message is a chat transcript (no MIME; not synced).
pub fn is_chat(labels: &[String]) -> bool {
    has(labels, system::CHAT)
}

/// The Gmail data of a message.
pub fn message_info(id: &str, thread_id: &str, labels: &[String]) -> GmailMessageInfo {
    GmailMessageInfo {
        gmail_id: id.to_owned(),
        thread_id: if thread_id.is_empty() {
            id.to_owned()
        } else {
            thread_id.to_owned()
        },
        label_ids: labels.to_vec(),
        category: GmailCategory::for_labels(labels),
    }
}

/// A listed message (`format=minimal` or `raw`) as an API engine reports
/// it.
pub fn remote_message(message: &Message, labels: &LabelTable) -> RemoteMessage {
    let info = message_info(&message.id, &message.thread_id, &message.label_ids);
    RemoteMessage {
        remote_id: message.id.clone(),
        thread_id: Some(info.thread_id.clone()),
        folder: GmailFolder::for_labels(&message.label_ids)
            .path()
            .to_owned(),
        flags: flags_for_labels(&message.label_ids),
        tags: labels.tags_for(&message.label_ids),
        internal_date: message.internal_date_secs(),
        size: message.size_estimate,
        raw: message.raw_bytes().ok().flatten(),
        gmail: Some(info),
        change_key: None,
        exchange: None,
    }
}

/// A label change of a known message: everything that follows from its
/// labels now.
pub fn remote_change(
    id: &str,
    thread_id: &str,
    label_ids: &[String],
    labels: &LabelTable,
) -> RemoteMessageChange {
    RemoteMessageChange {
        remote_id: id.to_owned(),
        folder: Some(GmailFolder::for_labels(label_ids).path().to_owned()),
        flags: Some(flags_for_labels(label_ids)),
        tags: Some(labels.tags_for(label_ids)),
        gmail: Some(message_info(id, thread_id, label_ids)),
        change_key: None,
        exchange: None,
    }
}

/// The labels a message stored into `folder` with `flags` gets
/// (`messages.insert`).
pub fn labels_for_insert(folder: GmailFolder, flags: Flags) -> Vec<String> {
    let mut out: Vec<String> = folder.label().into_iter().map(str::to_owned).collect();
    if !flags.seen {
        out.push(system::UNREAD.to_owned());
    }
    if flags.flagged {
        out.push(system::STARRED.to_owned());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::labels::label_info;
    use crate::wire::ListLabelsResponse;
    use nail_api::domain::FolderRole;

    fn table() -> LabelTable {
        let page: ListLabelsResponse =
            serde_json::from_str(include_str!("../fixtures/responses/labels-list.json")).unwrap();
        LabelTable::new(page.labels.iter().map(label_info))
    }

    fn ids(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn folders_have_roles() {
        let folders = remote_folders();
        assert_eq!(folders.len(), 6);
        assert_eq!(folders[0].remote_id, "INBOX");
        assert_eq!(folders[0].role, FolderRole::Inbox);
        assert!(
            folders
                .iter()
                .any(|f| f.remote_id == "ARCHIVE" && f.role == FolderRole::Archive)
        );
        assert!(
            folders
                .iter()
                .any(|f| f.remote_id == "SPAM" && f.role == FolderRole::Junk)
        );
    }

    #[test]
    fn labels_become_folder_flags_tags_and_category() {
        let message: Message = serde_json::from_value(serde_json::json!({
            "id": "m1", "threadId": "t1",
            "labelIds": ["STARRED", "CATEGORY_SOCIAL", "INBOX", "Label_7", "IMPORTANT"],
            "internalDate": "1790856000000", "sizeEstimate": 77
        }))
        .unwrap();
        let remote = remote_message(&message, &table());
        assert_eq!(remote.remote_id, "m1");
        assert_eq!(remote.thread_id.as_deref(), Some("t1"));
        assert_eq!(remote.folder, "INBOX");
        assert!(remote.flags.seen && remote.flags.flagged && !remote.flags.draft);
        assert_eq!(remote.tags, ["Work/Projects"]);
        assert_eq!(remote.internal_date, Some(1_790_856_000));
        assert_eq!(remote.size, Some(77));
        assert_eq!(remote.raw, None);
        let info = remote.gmail.unwrap();
        assert_eq!(info.category, Some(GmailCategory::Social));
        assert!(info.label_ids.contains(&"IMPORTANT".to_owned()));
    }

    #[test]
    fn changes_and_threads() {
        let change = remote_change("m", "", &ids(&["UNREAD", "TRASH", "Label_12"]), &table());
        assert_eq!(change.folder.as_deref(), Some("TRASH"));
        assert!(!change.flags.unwrap().seen);
        assert_eq!(change.tags.unwrap(), ["Receipts"]);
        // No thread id: the message is its own thread.
        assert_eq!(change.gmail.unwrap().thread_id, "m");
        let archived = remote_change("m", "t", &[], &table());
        assert_eq!(archived.folder.as_deref(), Some("ARCHIVE"));
        assert!(archived.flags.unwrap().seen);
        assert!(is_chat(&ids(&["CHAT"])));
        assert!(!is_chat(&ids(&["INBOX"])));
        let draft = flags_for_labels(&ids(&["DRAFT"]));
        assert!(draft.draft);
    }

    #[test]
    fn insert_labels() {
        assert_eq!(
            labels_for_insert(
                GmailFolder::Inbox,
                Flags {
                    seen: false,
                    flagged: true,
                    ..Flags::default()
                }
            ),
            ["INBOX", "UNREAD", "STARRED"]
        );
        assert_eq!(
            labels_for_insert(
                GmailFolder::Archive,
                Flags {
                    seen: true,
                    ..Flags::default()
                }
            )
            .len(),
            0
        );
    }
}
