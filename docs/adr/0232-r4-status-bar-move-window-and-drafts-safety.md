# 0232 — R4: status bar tracker, Move window, drafts safety

Status: accepted (2026-11-21)

## Context

ADR 0230 (the R4 contract) set the UI's part of the bulk-action fixes:
the status bar follows the core's queue summary, lists are refetched on
`Resync`, the Move window moves only where the user chose, and only real
drafts continue in the composer. ADR 0231 records the core's part. This
ADR records how the UI packages built theirs and the core change the
drafts safety needed.

## Decision

### Status bar (`ui/src/state/opsSummary.ts`)

- One tracker reads the summary with `GetOpsSummary` on start, after a
  reconnect and on `Resync`, and applies every `OpsChanged`. An answer
  older than a summary that already arrived as an event is dropped.
- **Re-check, not a timeout:** while changes wait and no summary arrived
  for `OPS_RECHECK_MS` (2.5 s), the tracker asks the core again, until the
  core reports nothing waiting. It never clears the count without the core
  saying so, which is what ADR 0230 rejected.
- `uiStore.pendingOps` keeps its name (tests of Gmail, Exchange, mail
  actions and triage read it) but now holds only the op ids queued since
  the core's last summary; any summary clears it, and the count is
  `opsWaiting(summary) + pendingOps.length`. `opsWaiting` ports
  `OpsSummary::waiting` and is pinned by the `ops` test vectors
  (`crates/nail-api/fixtures/ts-vectors/ops.json`).
- `uiStore` loses `failedOp` and `opFinished` and gains `opsSummary`,
  `setOpsSummary`, `resyncAt` and `resynced`. A failure shows the newest
  failure's localized text (else its error class) and, with several, how
  many (`status-ops-failed-many`). When `failed` grows, the lists are
  fetched again. `OpStatus` now only refetches the lists when a folder
  change fails.
- On `Resync`, `App.tsx` re-reads the summary and refetches folders,
  lists, conversations, searches and the open message
  (`queryKeys.allMessageBodies`), then records `resyncAt`. `MessageList`
  drops a selected message it no longer shows once the list has fetched
  again after that time (`refetchedSince` in `actions/mailTargets.ts`).

### Move window (`ui/src/widgets/FolderPicker.tsx`)

- It renders its own combobox and listbox instead of the shared
  `FolderList`, which the folder dialogs keep using. Arrow keys or the mouse
  move the highlight (`aria-selected`, `aria-activedescendant`,
  `data-state="active"`); Enter takes only the highlighted option, or the
  single folder whose name equals the typed text. Otherwise Enter does
  nothing and the status line (`folder-picker.notice`, `role=status`)
  says how to choose.
- `matchRank` is the one match rule (exact name, name prefix, name
  contains, path contains); the shared pickers' `filterFolders` uses it
  too. The folder the targets are in is left out (`sourceFolderOf`).
- Moving into Drafts, Trash or Junk asks first inside the window: a
  `mail-confirm` group labelled by its question, with the count from
  `ResolveMailTargets`; Cancel has the focus, and Move moves exactly the
  counted messages. Which roles ask is one table over every folder role
  (`ASKS_BEFORE_MOVE` in `features/mail-actions/move.ts`).

### Drafts safety

- UI: `isDraftMessage` checks only the `\Draft` flag. A message in Drafts
  without it opens in the reader and offers the new action
  `mail.move-out-of-drafts` (reader toolbar, reader and message menus,
  palette), which moves it to `{ role: "inbox" }`.
- Core: `EditDraftMessage` on a message without `\Draft` (an MCP or remote
  client can still send it) copies the message into a new draft **without**
  a server copy (`draft_edit.rs`): the next autosave adds Nail's own copy
  next to it, and sending or discarding the draft never replaces or
  deletes the original. Nail's own draft copies and drafts of other clients
  carry `\Draft` and keep the old behaviour. The mock does the same.

## Consequences

- The status bar no longer depends on any single event arriving.
- The Move window's look has its own parts (`folder-picker.item`,
  `folder-picker.notice`); the visual baseline of the window changed.
- Continuing the same non-draft twice makes two drafts (nothing links them
  to the message); the UI no longer offers it, so only protocol clients can
  do that.
