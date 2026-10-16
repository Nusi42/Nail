# 0196 — Folder management

Status: accepted (2026-10-16)

## Context

Nail could not create, rename, move or delete folders. The protocol clients
could (IMAP `CREATE` / `RENAME` / `DELETE`, Graph `mailFolders`), but no
request or menu used them; the core only created a missing Snoozed folder.
IMAP folder ids and message ids were derived from the server path
(`ids::folder_id`, `ids::message_id`), so a rename would have looked like a
deleted folder plus a new one: every message downloaded again with new ids,
sync state, per-folder settings and pending operations lost. Gmail's
folders are labels (managed already, ADR 0065), and the EWS client has no
folder operations.

## Decision

- **API** (`nail_api::folders`): `CreateFolder { account_id, parent_id,
  name }`, `RenameFolder { folder_id, name }`, `MoveFolder { folder_id,
  parent_id }`, `DeleteFolder { folder_id, permanent }`, each answered with
  `Queued { op_ids }`; refusals are `invalid_request` with a
  `FolderIssue` (`folder-issue-*`, `core-folders.ftl`): missing name, too
  long, invalid (separator, control characters), name taken among the
  siblings, system folder (any role but Other, the Snoozed folder,
  Exchange well-known folders and mailbox roots), into itself, other
  account, no hierarchy, no Trash, Gmail (labels), unsupported (EWS,
  virtual accounts). Remote sessions need `organize` (`permanent-delete`
  for a permanent delete) and granted folders.
- **Queued, server first.** Changes go into `folder_ops` (schema v20) and
  are replayed by the account's engine before its message operations
  (`replay_account` → `folders::replay_folder_ops`, so a move into a new
  folder works). The store changes only after the server confirmed: a
  sync in between can never undo a pending change, and a refused change
  needs no rollback. Offline, changes wait; retryable failures are retried
  up to five times. Progress is `OpStatus` with `folder-<n>` op ids; a
  server conflict is reported as `folder-issue-name-taken`.
- **Ids stay.** A rename or move updates the folder's and its subfolders'
  paths in place (`Store::move_folder_tree`); ids, messages, UIDs and sync
  state stay. IMAP folder mirroring now matches folders by path (not by
  derived id) and records the hierarchy delimiter (`folders.delimiter`);
  a new path gets `ids::unique_folder_id` (the derived id unless a renamed
  folder still holds it), and new message ids skip ids already taken.
  Exchange ids come from the server's folder ids already; after a change
  the engine reads the mailbox's folder tree again.
- **Paths in the config follow** a rename or move: sync policies and
  virtual-account folder mappings in `accounts.kdl`, folder references in
  `rules.kdl` (rules without an account only when no other account has a
  folder at the old path).
- **Delete** moves the folder (with its subfolders) into the account's
  Trash (`Name (2)` when taken there). In Trash, or without one, only a
  permanent delete is possible: the UI asks first; IMAP deletes the
  subfolders deepest first.
- **IMAP details:** paths are built with the server's delimiter and
  modified UTF-7 (`nail_proto_imap::folder_path`); new and renamed folders
  are subscribed; when every folder lives below `INBOX<delimiter>` (a
  personal namespace like Courier's), "the top level" is below INBOX.
- **Engines:** IMAP and local folders (POP3, local accounts: the store
  only) in `replay.rs`; Exchange through `ExchangeMailEngine::folder_write`
  (Graph: create, rename, move — new `POST …/move` — and delete; EWS
  answers `unsupported` for now).
- **UI** (`ui/src/features/folders/`): actions `folder.new`,
  `folder.rename`, `folder.move`, `folder.delete` in the folder pane's
  menus (account heading: New folder); off for Gmail and virtual accounts
  and, except New, for system folders. Windows `folder-name`,
  `folder-move` (the shared `FolderList`, also used by the message move
  picker) and the confirmation `folder-delete`; they wait for the outcome
  (`state/queuedOps.ts` `runQueued`) and show the core's reason or that
  the change waits for the server.

## Consequences

- EWS accounts (Exchange on premises) cannot change folders yet: the EWS
  client needs CreateFolder / UpdateFolder / MoveFolder / DeleteFolder.
- Folder changes are not undoable (they are confirmed by the server and
  can be reversed by another change).
- MCP tools and plug-ins have no folder management yet.
