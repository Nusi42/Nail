//! Folder management (ADR 0196): `CreateFolder`, `RenameFolder`,
//! `MoveFolder`, `DeleteFolder`.
//!
//! A request is checked against the store (names, system folders, the
//! target place) and queued as a [`FolderOp`]; the answer is `Queued`.
//! [`replay_folder_ops`] runs first in every replay of the account (all
//! engines go through [`crate::replay::replay_account`]): the executor
//! changes the folder on the server ([`OpExecutor::folder_change`]), then
//! the store follows — a create adds the row, a rename or move re-paths the
//! folder and its subfolders in place (ids, messages and sync state stay),
//! a permanent delete removes them with their messages. Exchange engines
//! read the folder tree again instead. Paths in `accounts.kdl` (sync
//! policies, virtual-account mappings) and `rules.kdl` follow a rename or
//! move. Nothing changes locally before the server confirmed, so a sync in
//! between never undoes a pending change.

use nail_api::engine::EngineError;
use nail_api::folders::FolderIssue;
use nail_api::rules::{Condition, FolderRef, RuleAction, RuleScope};
use nail_api::{
    AccountId, AccountKind, ApiError, ErrorCode, Event, Folder, FolderId, FolderRole,
    LocalizedMessage, OpId, OpState, OpStatus, Request, Response,
};
use nail_store::{FolderOp, FolderOpOutcome, QueuedFolderOp, Store, folder_op_id};

use crate::error::{CoreError, Result};
use crate::replay::OpExecutor;
use crate::service::Service;

/// Attempts before a retryable failure fails the change.
const MAX_ATTEMPTS: u32 = 5;

/// A queued change, resolved against the store for the executor.
#[derive(Debug, Clone)]
pub(crate) enum FolderChange {
    /// Create `name` below `parent`.
    Create {
        /// The account.
        account: AccountId,
        /// The parent (`None` = top level).
        parent: Option<Folder>,
        /// The display name.
        name: String,
    },
    /// Give `folder` this parent and name.
    Update {
        /// The folder as it is now.
        folder: Folder,
        /// The new parent (`None` = top level).
        parent: Option<Folder>,
        /// The new display name.
        name: String,
    },
    /// Delete `folder` and `subfolders` (deepest first) for good.
    Delete {
        /// The folder.
        folder: Folder,
        /// Its subfolders, deepest first.
        subfolders: Vec<Folder>,
    },
}

/// What an executor did, for the store to follow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FolderApplied {
    /// The folder is at this server path now (create, rename, move): the
    /// core updates the store.
    Path {
        /// The server path.
        path: String,
        /// The hierarchy separator it was built with.
        delimiter: Option<String>,
    },
    /// Deleted: the core removes the folders and their messages.
    Deleted,
    /// The engine read the folder tree again; the store is current.
    Synced,
}

/// Which way an account's folders are managed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Manager {
    /// IMAP (`CREATE` / `RENAME` / `DELETE`).
    Imap,
    /// Folders only Nail keeps (POP3, local accounts).
    Local,
    /// Exchange through Graph.
    Graph,
}

/// The request handler.
pub(crate) async fn handle(service: &Service, request: Request) -> Result<Response> {
    let (account, op) = match request {
        Request::CreateFolder {
            account_id,
            parent_id,
            name,
        } => {
            let manager = manager(service, &account_id).await?;
            let account = account_id.clone();
            let op = service
                .store()
                .call(move |s| Ok(plan_create(s, manager, &account, parent_id.as_ref(), &name)))
                .await??;
            (account_id, op)
        }
        Request::RenameFolder { folder_id, name } => {
            plan_for(service, &folder_id, move |s, m, f| {
                plan_rename(s, m, f, &name)
            })
            .await?
        }
        Request::MoveFolder {
            folder_id,
            parent_id,
        } => {
            plan_for(service, &folder_id, move |s, m, f| {
                plan_move(s, m, f, parent_id.as_ref())
            })
            .await?
        }
        Request::DeleteFolder {
            folder_id,
            permanent,
        } => {
            plan_for(service, &folder_id, move |s, m, f| {
                plan_delete(s, m, f, permanent)
            })
            .await?
        }
        other => {
            return Err(ApiError::internal(format!(
                "{} is not a folder request",
                other.wire_type()
            ))
            .into());
        }
    };
    let Some(op) = op else {
        // Nothing to change (the same name and place).
        return Ok(Response::Queued { op_ids: Vec::new() });
    };
    let id = {
        let account = account.clone();
        service
            .store()
            .call(move |s| s.enqueue_folder_op(&account, &op))
            .await?
    };
    let op_id = folder_op_id(id);
    broadcast_status(service, &op_id, &account, OpState::Pending, None);
    service.request_replay(&account);
    Ok(Response::Queued {
        op_ids: vec![op_id],
    })
}

/// Plans a change of an existing folder: its account, and the operation
/// (`None` = nothing to change).
async fn plan_for(
    service: &Service,
    id: &FolderId,
    plan: impl FnOnce(&Store, Manager, &Folder) -> Result<Option<FolderOp>> + Send + 'static,
) -> Result<(AccountId, Option<FolderOp>)> {
    let id = id.clone();
    let folder = service.store().call(move |s| s.get_folder(&id)).await?;
    let manager = manager(service, &folder.account_id).await?;
    let account = folder.account_id.clone();
    let op = service
        .store()
        .call(move |s| Ok(plan(s, manager, &folder)))
        .await??;
    Ok((account, op))
}

/// `invalid_request` with the issue's text.
fn refuse(issue: FolderIssue, detail: impl Into<String>) -> CoreError {
    ApiError::invalid_localized(detail, issue.message_id()).into()
}

/// How the account's folders are managed, or why they cannot be.
async fn manager(service: &Service, account: &AccountId) -> Result<Manager> {
    let id = account.clone();
    let kind = service
        .store()
        .call(move |s| s.get_account(&id))
        .await?
        .kind;
    match kind {
        AccountKind::Imap => Ok(Manager::Imap),
        AccountKind::Pop3 | AccountKind::Local => Ok(Manager::Local),
        AccountKind::Gmail => Err(refuse(
            FolderIssue::GmailLabels,
            format!("account {account} is a Gmail account: use labels"),
        )),
        AccountKind::Exchange => {
            let config = service.account_config(account)?;
            let graph = matches!(
                &config.incoming,
                nail_api::IncomingServer::Exchange { options, .. }
                    if options.engine == nail_api::ExchangeEngine::Graph
            );
            if graph {
                Ok(Manager::Graph)
            } else {
                Err(refuse(
                    FolderIssue::Unsupported,
                    format!("account {account} uses EWS, which cannot change folders yet"),
                ))
            }
        }
        AccountKind::Virtual => Err(refuse(
            FolderIssue::Unsupported,
            format!("account {account} is a virtual account"),
        )),
    }
}

/// Whether a folder may not be renamed, moved or deleted.
fn is_system(s: &Store, manager: Manager, folder: &Folder) -> nail_store::Result<bool> {
    if folder.role != FolderRole::Other || crate::junk::is_snoozed_folder(s, folder) {
        return Ok(true);
    }
    if manager == Manager::Graph {
        // Mailbox roots and well-known folders.
        return Ok(s
            .exchange_folder(&folder.id)?
            .is_none_or(|r| r.well_known.is_some()));
    }
    Ok(false)
}

/// Checks a name for a place; the trimmed name.
fn check_name(
    s: &Store,
    manager: Manager,
    account: &AccountId,
    parent: Option<&Folder>,
    name: &str,
    except: Option<&FolderId>,
) -> Result<String> {
    if let Some(issue) = FolderIssue::of_name(name) {
        return Err(refuse(issue, format!("{name:?} is not a folder name")));
    }
    let name = name.trim().to_owned();
    if manager == Manager::Imap || manager == Manager::Local {
        let delimiter = delimiter_of(s, manager, account, parent)?;
        if delimiter.as_deref().is_some_and(|d| name.contains(d)) {
            return Err(refuse(
                FolderIssue::InvalidName,
                format!("{name:?} contains the folder separator"),
            ));
        }
    }
    let taken = s.list_folders(account)?.iter().any(|f| {
        f.parent_id.as_ref() == parent.map(|p| &p.id)
            && f.name.eq_ignore_ascii_case(&name)
            && Some(&f.id) != except
    });
    if taken {
        return Err(refuse(
            FolderIssue::NameTaken,
            format!("a folder named {name:?} exists there"),
        ));
    }
    Ok(name)
}

/// The hierarchy separator for children of `parent`.
fn delimiter_of(
    s: &Store,
    manager: Manager,
    account: &AccountId,
    parent: Option<&Folder>,
) -> nail_store::Result<Option<String>> {
    if manager == Manager::Local {
        return Ok(Some(LOCAL_DELIMITER.to_owned()));
    }
    s.folder_delimiter(account, parent.map(|p| &p.id))
}

/// The separator of folders only Nail keeps.
pub(crate) const LOCAL_DELIMITER: &str = "/";

fn parent_in(s: &Store, account: &AccountId, parent: Option<&FolderId>) -> Result<Option<Folder>> {
    let Some(id) = parent else {
        return Ok(None);
    };
    let parent = s.get_folder(id)?;
    if &parent.account_id != account {
        return Err(refuse(
            FolderIssue::OtherAccount,
            format!("folder {id} belongs to another account"),
        ));
    }
    Ok(Some(parent))
}

/// Where "the top level" is on an IMAP server whose folders all live
/// below INBOX (a personal namespace `INBOX.`, as Courier keeps it):
/// INBOX. `None` = the real top level.
fn personal_root(s: &Store, manager: Manager, account: &AccountId) -> Result<Option<Folder>> {
    if manager != Manager::Imap {
        return Ok(None);
    }
    let Some(delimiter) = s.folder_delimiter(account, None)? else {
        return Ok(None);
    };
    let folders = s.list_folders(account)?;
    let Some(inbox) = folders
        .iter()
        .find(|f| f.path.eq_ignore_ascii_case("INBOX"))
    else {
        return Ok(None);
    };
    let prefix = format!("{}{delimiter}", inbox.path);
    let mut others = folders.iter().filter(|f| f.id != inbox.id).peekable();
    let below = others.peek().is_some() && others.all(|f| f.path.starts_with(&prefix));
    Ok(below.then(|| inbox.clone()))
}

fn plan_create(
    s: &Store,
    manager: Manager,
    account: &AccountId,
    parent: Option<&FolderId>,
    name: &str,
) -> Result<Option<FolderOp>> {
    let parent = match parent_in(s, account, parent)? {
        None => personal_root(s, manager, account)?,
        some => some,
    };
    let name = check_name(s, manager, account, parent.as_ref(), name, None)?;
    Ok(Some(FolderOp::Create {
        parent_id: parent.map(|p| p.id),
        name,
    }))
}

fn plan_rename(
    s: &Store,
    manager: Manager,
    folder: &Folder,
    name: &str,
) -> Result<Option<FolderOp>> {
    refuse_system(s, manager, folder)?;
    let parent = parent_in(s, &folder.account_id, folder.parent_id.as_ref())?;
    if name.trim() == folder.name {
        return Ok(None);
    }
    let name = check_name(
        s,
        manager,
        &folder.account_id,
        parent.as_ref(),
        name,
        Some(&folder.id),
    )?;
    Ok(Some(FolderOp::Update {
        folder_id: folder.id.clone(),
        name,
        parent_id: folder.parent_id.clone(),
    }))
}

fn plan_move(
    s: &Store,
    manager: Manager,
    folder: &Folder,
    parent: Option<&FolderId>,
) -> Result<Option<FolderOp>> {
    refuse_system(s, manager, folder)?;
    let parent = match parent_in(s, &folder.account_id, parent)? {
        None => personal_root(s, manager, &folder.account_id)?,
        some => some,
    };
    if parent.as_ref().map(|p| &p.id) == folder.parent_id.as_ref() {
        return Ok(None);
    }
    if let Some(p) = &parent
        && (p.id == folder.id || s.folder_descendants(&folder.id)?.contains(&p.id))
    {
        return Err(refuse(
            FolderIssue::IntoItself,
            format!("folder {} cannot move into itself", folder.id),
        ));
    }
    let name = check_name(
        s,
        manager,
        &folder.account_id,
        parent.as_ref(),
        &folder.name,
        Some(&folder.id),
    )?;
    Ok(Some(FolderOp::Update {
        folder_id: folder.id.clone(),
        name,
        parent_id: parent.map(|p| p.id),
    }))
}

fn plan_delete(
    s: &Store,
    manager: Manager,
    folder: &Folder,
    permanent: bool,
) -> Result<Option<FolderOp>> {
    refuse_system(s, manager, folder)?;
    if permanent {
        return Ok(Some(FolderOp::Delete {
            folder_id: folder.id.clone(),
        }));
    }
    let folders = s.list_folders(&folder.account_id)?;
    let trash = folders.iter().find(|f| f.role == FolderRole::Trash);
    let in_trash = trash.is_some_and(|t| is_inside(&folders, folder, &t.id));
    let Some(trash) = trash.filter(|_| !in_trash) else {
        return Err(refuse(
            FolderIssue::NoTrash,
            format!("folder {} can only be deleted for good", folder.id),
        ));
    };
    // "Receipts", "Receipts (2)", … — the first name free in Trash.
    let free = |name: &str| {
        !folders
            .iter()
            .any(|f| f.parent_id.as_ref() == Some(&trash.id) && f.name.eq_ignore_ascii_case(name))
    };
    // At most one name per folder in Trash is taken, so one more is free.
    let name = (1..=folders.len() + 1)
        .map(|n| {
            if n == 1 {
                folder.name.clone()
            } else {
                format!("{} ({n})", folder.name)
            }
        })
        .find(|name| free(name))
        .unwrap_or_else(|| folder.name.clone());
    Ok(Some(FolderOp::Update {
        folder_id: folder.id.clone(),
        name,
        parent_id: Some(trash.id.clone()),
    }))
}

fn refuse_system(s: &Store, manager: Manager, folder: &Folder) -> Result<()> {
    if is_system(s, manager, folder)? {
        return Err(refuse(
            FolderIssue::SystemFolder,
            format!("folder {} is a system folder", folder.id),
        ));
    }
    Ok(())
}

/// Whether `folder` is `ancestor` or below it.
fn is_inside(folders: &[Folder], folder: &Folder, ancestor: &FolderId) -> bool {
    let mut current = Some(folder);
    while let Some(f) = current {
        if &f.id == ancestor {
            return true;
        }
        current = f
            .parent_id
            .as_ref()
            .and_then(|p| folders.iter().find(|x| &x.id == p));
    }
    false
}

fn broadcast_status(
    service: &Service,
    op_id: &OpId,
    account: &AccountId,
    state: OpState,
    error: Option<(&EngineError, Option<LocalizedMessage>)>,
) {
    let (code, message, localized) = match error {
        Some((e, localized)) => (Some(e.code), Some(e.message.clone()), localized),
        None => (None, None, None),
    };
    service.broadcast(Event::OpStatus {
        status: OpStatus {
            op_id: op_id.clone(),
            account_id: account.clone(),
            state,
            error: code,
            localized,
            message,
        },
    });
}

/// The user-facing text of a server's refusal.
fn localized(err: &EngineError) -> Option<LocalizedMessage> {
    (err.code == ErrorCode::Conflict)
        .then(|| LocalizedMessage::new(FolderIssue::NameTaken.message_id()))
}

/// Replays the account's queued folder changes with `executor`; a
/// retryable failure keeps the rest queued.
pub(crate) async fn replay_folder_ops(
    service: &Service,
    account: &AccountId,
    executor: &dyn OpExecutor,
) -> Result<()> {
    let ops = {
        let account = account.clone();
        service
            .store()
            .call(move |s| s.pending_folder_ops(&account))
            .await?
    };
    for op in ops {
        let op_id = op.op_id();
        broadcast_status(service, &op_id, account, OpState::InFlight, None);
        let result = run_one(service, executor, &op).await;
        let (outcome, stop) = match &result {
            Ok(()) => {
                broadcast_status(service, &op_id, account, OpState::Done, None);
                (FolderOpOutcome::Done, false)
            }
            Err(err) if err.retryable && op.attempts + 1 < MAX_ATTEMPTS => {
                broadcast_status(
                    service,
                    &op_id,
                    account,
                    OpState::Pending,
                    Some((err, None)),
                );
                (FolderOpOutcome::Retry(err.message.clone()), true)
            }
            Err(err) => {
                tracing::warn!(%account, op = %op_id, error = %err, "a folder change failed");
                broadcast_status(
                    service,
                    &op_id,
                    account,
                    OpState::Failed,
                    Some((err, localized(err))),
                );
                (FolderOpOutcome::Failed(err.message.clone()), false)
            }
        };
        let row = op.id;
        service
            .store()
            .call(move |s| s.finish_folder_op(row, &outcome))
            .await?;
        if stop {
            break;
        }
    }
    Ok(())
}

/// Resolves, executes and applies one change.
async fn run_one(
    service: &Service,
    executor: &dyn OpExecutor,
    op: &QueuedFolderOp,
) -> std::result::Result<(), EngineError> {
    let change = {
        let op = op.clone();
        service
            .store()
            .call(move |s| resolve(s, &op))
            .await
            .map_err(|e| EngineError::from(CoreError::from(e)))?
    };
    let Some(change) = change else {
        // The folder is gone already.
        return Ok(());
    };
    let applied = executor.folder_change(&change).await?;
    apply(service, &change, applied)
        .await
        .map_err(EngineError::from)
}

/// The change with the folders as they are now; `None` when there is
/// nothing to do (the folder was deleted meanwhile).
fn resolve(s: &Store, op: &QueuedFolderOp) -> nail_store::Result<Option<FolderChange>> {
    let find = |id: &FolderId| -> nail_store::Result<Option<Folder>> {
        match s.get_folder(id) {
            Ok(folder) => Ok(Some(folder)),
            Err(nail_store::Error::NotFound { .. }) => Ok(None),
            Err(err) => Err(err),
        }
    };
    let parent = |id: &Option<FolderId>| -> nail_store::Result<Option<Option<Folder>>> {
        match id {
            None => Ok(Some(None)),
            Some(id) => Ok(find(id)?.map(Some)),
        }
    };
    Ok(match &op.op {
        FolderOp::Create { parent_id, name } => match parent(parent_id)? {
            Some(parent) => Some(FolderChange::Create {
                account: op.account_id.clone(),
                parent,
                name: name.clone(),
            }),
            None => {
                return Err(nail_store::Error::InvalidData(format!(
                    "the parent folder {parent_id:?} is gone"
                )));
            }
        },
        FolderOp::Update {
            folder_id,
            name,
            parent_id,
        } => match (find(folder_id)?, parent(parent_id)?) {
            (Some(folder), Some(parent)) => Some(FolderChange::Update {
                folder,
                parent,
                name: name.clone(),
            }),
            (None, _) => None,
            (Some(_), None) => {
                return Err(nail_store::Error::InvalidData(format!(
                    "the parent folder {parent_id:?} is gone"
                )));
            }
        },
        FolderOp::Delete { folder_id } => match find(folder_id)? {
            Some(folder) => {
                let mut subfolders = Vec::new();
                for id in s.folder_descendants(&folder.id)? {
                    if let Some(f) = find(&id)? {
                        subfolders.push(f);
                    }
                }
                // Deepest first: longer paths below shorter ones.
                subfolders.sort_by_key(|f| std::cmp::Reverse(f.path.len()));
                Some(FolderChange::Delete { folder, subfolders })
            }
            None => None,
        },
    })
}

/// The server path of a new or moved folder (`parent` `None` = top level).
pub(crate) fn server_path(
    imap: bool,
    parent: Option<&Folder>,
    delimiter: Option<&str>,
    name: &str,
) -> std::result::Result<String, EngineError> {
    if imap {
        nail_proto_imap::folder_path(parent.map(|p| p.path.as_str()), delimiter, name)
            .map_err(EngineError::from)
    } else {
        Ok(match parent {
            Some(p) => format!("{}{LOCAL_DELIMITER}{name}", p.path),
            None => name.to_owned(),
        })
    }
}

/// The store (and the config) after an executed change.
async fn apply(service: &Service, change: &FolderChange, applied: FolderApplied) -> Result<()> {
    let renamed = match change {
        FolderChange::Update { folder, .. } => Some(folder.clone()),
        FolderChange::Create { .. } | FolderChange::Delete { .. } => None,
    };
    let changed = {
        let change = change.clone();
        let applied = applied.clone();
        let blobs = service.blobs().clone();
        let index = service.search_index();
        service
            .store()
            .call(move |s| apply_to_store(s, &blobs, &index, &change, &applied))
            .await?
    };
    if let Some(before) = renamed {
        let id = before.id.clone();
        let new = service.store().call(move |s| s.get_folder(&id)).await?.path;
        if new != before.path {
            rewrite_config(service, &before.account_id, &before.path, &new).await;
        }
    }
    service.broadcast(Event::MailChanged {
        folder_ids: changed,
    });
    Ok(())
}

/// Applies an executed change to the store; the folders that changed.
fn apply_to_store(
    s: &Store,
    blobs: &nail_store::BlobStore,
    index: &nail_search::SearchIndex,
    change: &FolderChange,
    applied: &FolderApplied,
) -> nail_store::Result<Vec<FolderId>> {
    match (change, applied) {
        (_, FolderApplied::Synced) => Ok(Vec::new()),
        (
            FolderChange::Create {
                account,
                parent,
                name,
            },
            FolderApplied::Path { path, delimiter },
        ) => {
            let id = crate::ids::unique_folder_id(s, account, path)?;
            s.upsert_folder(&Folder {
                id: id.clone(),
                account_id: account.clone(),
                parent_id: parent.as_ref().map(|p| p.id.clone()),
                name: name.clone(),
                path: path.clone(),
                role: FolderRole::Other,
                unread: 0,
                total: 0,
            })?;
            s.set_folder_delimiter(&id, delimiter.as_deref())?;
            Ok(vec![id])
        }
        (
            FolderChange::Update {
                folder,
                parent,
                name,
            },
            FolderApplied::Path { path, .. },
        ) => {
            s.move_folder_tree(&folder.id, parent.as_ref().map(|p| &p.id), name, path)?;
            let mut changed = vec![folder.id.clone()];
            changed.extend(s.folder_descendants(&folder.id)?);
            Ok(changed)
        }
        (FolderChange::Delete { folder, subfolders }, FolderApplied::Deleted) => {
            let mut changed = Vec::new();
            for f in subfolders.iter().chain(std::iter::once(folder)) {
                if crate::sync::remove_local_folder(s, blobs, index, &f.id)? {
                    changed.push(f.id.clone());
                }
            }
            Ok(changed)
        }
        _ => Err(nail_store::Error::InvalidData(
            "the executor answered a folder change it was not asked".into(),
        )),
    }
}

/// Rewrites folder paths in `accounts.kdl` (sync policies, virtual-account
/// mappings) and `rules.kdl` after `old` became `new` (and the same for
/// the subfolders). Failures are logged: the folder change itself stands.
async fn rewrite_config(service: &Service, account: &AccountId, old: &str, new: &str) {
    let renamed = |path: &str| -> Option<String> {
        if path == old {
            return Some(new.to_owned());
        }
        ["/", ".", "\\"].iter().find_map(|d| {
            path.strip_prefix(&format!("{old}{d}"))
                .map(|rest| format!("{new}{d}{rest}"))
        })
    };
    let config = service.inner.accounts.config();
    if let Some(account_config) = config.account(account) {
        let mut edited = account_config.clone();
        let mut changed = false;
        for rule in &mut edited.sync.folders {
            if let nail_api::accounts::FolderMatch::Path(p) = &mut rule.folder
                && let Some(n) = renamed(p)
            {
                *p = n;
                changed = true;
            }
        }
        if changed && let Err(err) = service.save_account(edited, &[], false).await {
            tracing::warn!(%account, %err, "cannot rewrite sync policies after a folder rename");
        }
    }
    for virtual_account in &config.virtual_accounts {
        let mut edited = virtual_account.clone();
        let mut changed = false;
        for source in edited
            .folder_mappings
            .iter_mut()
            .flat_map(|m| m.sources.iter_mut())
            .filter(|s| &s.account_id == account)
        {
            if let Some(n) = renamed(&source.path) {
                source.path = n;
                changed = true;
            }
        }
        if changed && let Err(err) = service.save_virtual_account(edited, false) {
            tracing::warn!(%account, %err, "cannot rewrite folder mappings after a folder rename");
        }
    }
    // Rules without an account apply to the message's account: rewrite
    // them only when no other account has a folder at the old path.
    let others = {
        let (account, old) = (account.clone(), old.to_owned());
        service
            .store()
            .call(move |s| {
                Ok(s.list_accounts()?
                    .iter()
                    .filter(|a| a.id != account)
                    .any(|a| s.find_folder_by_path(&a.id, &old).ok().flatten().is_some()))
            })
            .await
            .unwrap_or(true)
    };
    let mut fix = |folder: &mut FolderRef| -> bool {
        if let FolderRef::Path { path, account_id } = folder
            && (account_id.as_ref() == Some(account) || (account_id.is_none() && !others))
            && let Some(n) = renamed(path)
        {
            *path = n;
            return true;
        }
        false
    };
    let mut edited = Vec::new();
    for mut rule in service.inner.rules.rules() {
        let mut changed = false;
        for scope in &mut rule.scope {
            if let RuleScope::Folder { folder } = scope {
                changed |= fix(folder);
            }
        }
        changed |= fix_conditions(&mut rule.conditions, &mut fix);
        for action in &mut rule.actions {
            if let RuleAction::Move { folder } | RuleAction::Copy { folder } = action {
                changed |= fix(folder);
            }
        }
        if changed {
            edited.push(rule);
        }
    }
    for rule in edited {
        let id = rule.id.clone();
        if let Err(err) = service.save_rule_in_place(rule) {
            tracing::warn!(%id, %err, "cannot rewrite a rule after a folder rename");
        }
    }
}

fn fix_conditions(condition: &mut Condition, fix: &mut impl FnMut(&mut FolderRef) -> bool) -> bool {
    match condition {
        Condition::All { conditions } | Condition::Any { conditions } => {
            let mut changed = false;
            for c in conditions {
                changed |= fix_conditions(c, fix);
            }
            changed
        }
        Condition::Not { condition } => fix_conditions(condition, fix),
        Condition::Folder { folder } => fix(folder),
        _ => false,
    }
}
