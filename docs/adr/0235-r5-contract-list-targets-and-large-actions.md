# 0235 — R5 contract: select all, list targets and large mail actions

Status: accepted (2026-11-22)

## Context

A maintainer report on 0.8.9: acting on many selected conversations often
failed with "Too many conversations at once; select fewer", although the
conversations held far fewer than 10 000 messages; and `Ctrl+A` did nothing
in the message list. The maintainer expects `Ctrl+A` to select really
everything in the list, including rows that are not loaded yet.

What the code did (verified for this ADR):

- `MAX_MAIL_ACTION_TARGETS` was 500 targets per request (a target is a list
  row: a conversation or a message) and `MAX_MAIL_ACTION_MESSAGES` 10 000
  messages (`crates/nail-api/src/mail_actions.rs`, ADR 0164).
- `ApplyMailAction` was split by the UI into requests of 500 targets
  (`applyInChunks`, `features/mail-actions/run.ts`), which lost the single
  undo, but the confirmations (`ResolveMailTargets` before Delete
  permanently and before a move into Drafts, Trash or Junk:
  `features/mail-actions/move.ts`, `widgets/DeletePermanently.tsx`) sent
  every selected row in one request: more than 500 rows failed with
  `mail-action-issue-too-many-targets` — the maintainer's message.
- The message list pages its rows (`ListThreads`, `ListMessages`, `Search`;
  a filter chip is a search, `widgets/listFilter.ts`), so the UI knows only
  the loaded rows and cannot name the others.
- The web socket accepts request frames of at most 1 MiB
  (`nail-server` `ws.rs`); the desktop's IPC has no such limit.

## Decision

### A list target (contract)

- `MessageTarget::List(ListTarget { list: MailList, except:
  Vec<MessageTarget>, as_of: Option<String> })`, JSON `{"kind": "list",
  "list": {…}, "except": […], "as_of": "…" | null}`.
- `MailList` is the very query the message list pages with
  (`#[serde(tag = "source")]`):
  - `conversations { folder_id }` — `ListThreads` rows (real or virtual
    folder, e.g. the unified inbox);
  - `messages { folder_id }` — `ListMessages` rows (a list without
    conversations);
  - `search { query: SearchQuery, conversations }` — the hits of the search
    the list runs (the typed search plus the filter chips' terms);
    `offset`, `limit` and `sort` are ignored; `conversations` = each hit
    stands for its conversation without a folder (a list with
    conversations), else for the hit message. Hits found only by the
    server search (`from_server`, headers-only messages searched on the
    IMAP server on the first page) are not part of the list: the core
    resolves the local index only.
- Each row stands for the target the UI sends for that row today
  (`rowTarget`): a conversation row a conversation target with the listed
  folder (none in a search), a message row a message target. The action
  reaches the union of the rows' messages (each row by the action's reach,
  ADR 0164) minus the messages the `except` targets resolve to; a
  conversation row counts in `conversations` when one of its messages is
  left. `except` holds the rows deselected after "select all" as the
  targets they stand for; a list there is refused (`nested-list`).
- `MailList::folder_id` (the list's folder, a search's folder scope) is the
  rule the remote UI checks against the grant; a remote session's rows
  resolve within its grant only (`within`, like conversation targets).
- `explicit_target_count` counts the rows a request names one by one
  (message and conversation targets, a list's exceptions included; a list
  itself counts nothing); `has_list`, `has_nested_list`. The UI's ports
  (`explicitTargetCount` in the mock, `listFolderOf` in
  `actions/mailTargets.ts`) are pinned by the `mail-actions` ts-vectors.

### Counts and the confirmation

- `MailTargetResolution` gains `messages: u32` (the true count) and
  `as_of: Option<String>`. With a list among the targets `message_ids` is
  empty (a whole list can be tens of thousands of ids) and `as_of` is a
  mark of the core's mail at that moment; without a list, `message_ids` is
  complete as before and `as_of` is `null`.
- The UI shows the confirmation with `messages` / `conversations` and then
  applies the same targets with `as_of` set to the mark: only messages the
  core held when it counted are acted on, so mail that arrived meanwhile is
  never touched unasked (the R4 rule `runOptionsOf` follows for explicit
  ids). `null` = the list as it is when the request runs. The mark is
  opaque to clients; how the core makes it (for example the highest
  message row id) is the core package's choice.
- `MailActionResult.message_ids` stays complete for every action (the UI
  patches its caches; the desktop IPC and the outgoing socket frames have
  no size limit).

### No limit on the action; parts and one undo

- `MAX_MAIL_ACTION_TARGETS` is no longer a limit on what an action touches
  but a sanity cap on rows named one by one: `MAX_REQUEST_BYTES /
  MAIL_TARGET_BYTES` = 1 MiB / 256 bytes = 4 096. `MAX_REQUEST_BYTES` is the
  new single definition of the largest request frame (`nail-api`
  `protocol.rs`; the web socket's `MAX_FRAME` derives it), and a test
  serializes 4 096 conversation targets with ids of 64 bytes and checks
  they fit. Selecting more rows one by one than that is refused with
  `too-many-targets`, whose text now points to "select all".
- `MAX_MAIL_ACTION_MESSAGES` is renamed `MAIL_ACTION_PART_MESSAGES`
  (10 000): the core plans, applies locally and queues at most that many
  messages per store transaction and runs a larger action in several
  parts, all recorded in **one** batch (one `MailBatchId`, one undo, one
  toast); the issue `too-many-messages` is removed. The contract's core
  still applies an action in one transaction; the parts are the core
  package's work.
- The UI drops `applyInChunks`: one request per action, with a list target
  for "select all".

### Selection in the message list (UI package)

- `Ctrl+A` (`Cmd+A` on macOS) in the message list selects the whole list
  (the current folder, unified view, search or filter chips), loaded or not;
  the list shows "All N selected" (`message-list-all-selected`,
  `locales/en/list-selection.ftl`) with the true count from the list's
  `total` minus the deselected rows; `Ctrl` / `Shift` + click deselects
  rows; `Esc` clears; changing the folder, search or filter clears it.
  Actions on the selection send one list target.

### Contract in code

- `nail-api`: `ListTarget`, `MailList` (+ `folder_id`),
  `MessageTarget::List` / `MessageTarget::list`, `explicit_target_count`,
  `has_list`, `has_nested_list`, `MAIL_TARGET_BYTES`,
  `MAIL_ACTION_PART_MESSAGES`, `MAX_REQUEST_BYTES`,
  `MailTargetResolution::{messages, as_of}`, `MailActionIssue::NestedList`
  (`too-many-messages` removed), `SearchQuery: Hash`; TypeScript
  regenerated; Fluent `mail-action-issue-nested-list` and the new
  `too-many-targets` text.
- `nail-core`: `mail_lists.rs` (`mail_list_rows`: the rows of a list page by
  page with `list_conversations`, `list_messages` and `search_with`
  without the server part; `within`), `resolve_targets` / `resolve_row`
  with list targets and exceptions, the remote check of a list's folder and
  exceptions. Tests in `crates/nail-core/tests/mail_actions.rs`.
- Mock core (`ui/src/api/mockMailActions.ts`): list targets, exceptions,
  `messages`, `as_of` marks (an apply with a mark acts only on messages the
  mock held then), the issues; Vitest in `mockMailActions.test.ts`.
- The UI only compiles against the new kind (a list counts as unknown in
  `anyUnread` / `canMark` / `allFlagged`, `expandTargets` skips it); it
  never builds one yet.

## Scope decisions

- **One request, no chunking in the UI.** Splitting in the UI cost the
  single undo and the true count; the core splits instead.
- **No overall limit.** The maintainer decided against any cap on
  conversations or messages; very large actions take longer to replay but
  stay one batch.
- **Server-only search hits are not in a selected list.** Selecting all
  hits of a search acts on the hits in the local index; the few headers-only
  messages a server search adds to the first page are not part of it. The
  count the UI shows comes from the core.
- **`message_ids` stays in `MailActionResult`.** Responses have no frame
  limit; the UI needs the ids to update its caches.
- **No new crates.** New locale slot: `locales/*/list-selection.ftl`.

## Consequences

- MCP and plug-in mail actions keep naming messages; they may use list
  targets through the same request later.
- The release notes "Nail 0.8.10" and `CHANGELOG.md` `[Unreleased]` are
  prepared; the packages add their user-visible items.
