//! Schema v20 (ADR 0196): queued folder changes and in-place folder moves.
#![cfg(test)]

#[expect(dead_code, reason = "each test crate uses part of the shared fixtures")]
mod common;

use common::seeded_store;
use nail_api::{AccountId, FolderId, FolderRole, testing};
use nail_store::{FolderOp, FolderOpOutcome, Store};

fn add(store: &Store, id: &str, path: &str, parent: Option<&str>) -> FolderId {
    let mut folder = testing::folder(id, "a1", path, FolderRole::Other);
    folder.parent_id = parent.map(FolderId::from);
    store.upsert_folder(&folder).unwrap();
    folder.id
}

#[test]
fn folder_ops_queue_and_finish() {
    let store = seeded_store();
    let account: AccountId = "a1".into();
    let create = FolderOp::Create {
        parent_id: None,
        name: "Receipts".into(),
    };
    let id = store.enqueue_folder_op(&account, &create).unwrap();
    assert!(store.has_pending_folder_ops(&account).unwrap());
    let pending = store.pending_folder_ops(&account).unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].op, create);
    assert_eq!(pending[0].op_id().as_str(), format!("folder-{id}"));
    store
        .finish_folder_op(id, &FolderOpOutcome::Retry("offline".into()))
        .unwrap();
    assert_eq!(store.pending_folder_ops(&account).unwrap()[0].attempts, 1);
    store.finish_folder_op(id, &FolderOpOutcome::Done).unwrap();
    assert!(!store.has_pending_folder_ops(&account).unwrap());
}

#[test]
fn a_tree_moves_in_place_and_keeps_its_ids() {
    let store = seeded_store();
    let account: AccountId = "a1".into();
    let projects = add(&store, "projects", "Projects", None);
    let acme = add(&store, "acme", "Projects/Acme", Some("projects"));
    let old = add(&store, "old", "Projects/Acme/2025", Some("acme"));
    let archive = add(&store, "archive", "Archive", None);
    store.set_folder_delimiter(&acme, Some("/")).unwrap();
    assert_eq!(
        store
            .folder_delimiter(&account, Some(&acme))
            .unwrap()
            .as_deref(),
        Some("/")
    );
    assert_eq!(
        store
            .folder_delimiter(&account, Some(&projects))
            .unwrap()
            .as_deref(),
        Some("/"),
        "falls back to the account's separator"
    );
    store
        .move_folder_tree(&acme, Some(&archive), "Acme Corp", "Archive/Acme Corp")
        .unwrap();
    let moved = store.get_folder(&acme).unwrap();
    assert_eq!(moved.path, "Archive/Acme Corp");
    assert_eq!(moved.name, "Acme Corp");
    assert_eq!(moved.parent_id, Some(archive.clone()));
    assert_eq!(
        store.get_folder(&old).unwrap().path,
        "Archive/Acme Corp/2025"
    );
    assert_eq!(store.folder_descendants(&archive).unwrap().len(), 2);
    // Not into itself, not onto a taken path.
    assert!(store.move_folder_tree(&acme, Some(&old), "x", "y").is_err());
    assert!(
        store
            .move_folder_tree(&projects, None, "Archive", "Archive")
            .is_err()
    );
}
