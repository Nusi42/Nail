# ADR 0164: P3 contract — Gmail-like conversation actions, conversation speed, card height

Status: accepted · 2026-10-05 · P3 contract (wave P3; plan section 10, "Wave P3")

## Context

P2 made a list row a conversation (ADR 0148: the thread's messages in the
listed folder plus the user's own replies from the accounts' Sent folders) and
the reader a card feed (ADR 0151). Actions on a row still act on one message:
the newest one the user did not write (ADRs 0150, 0153). Decision:
actions on a conversation row must behave like Gmail.

Two P2 follow-ups join the wave: `ListThreads` costs 33 ms p95 per page on the
standard corpus (3 ms for a message page) because every page groups the whole
folder by thread (ADR 0154 deferred a per-folder thread-activity table), and
HTML cards in the feed keep a 40vh minimum because a sandboxed frame without
scripts cannot report its height (ADR 0151).

This contract fixes the semantics, the protocol, the schema, the frame-height
design with its security argument, and the slots of four parallel packages.

## Decisions

### 1. Gmail-like conversation actions

#### Targets and the request

- `nail_api::mail_actions` (new module). A **target** is a message or a
  conversation as a row shows it:
  `MessageTarget::Message { message_id }` |
  `MessageTarget::Conversation(ConversationRef { thread_id, folder_id })`
  (JSON `{"kind": "message", …}` / `{"kind": "conversation", "thread_id",
  "folder_id"}`). `folder_id` is the folder the row is listed in — real, or
  virtual (the unified inbox: every member folder, per account). `null` = shown
  without a folder (search hits): the thread in every folder but Drafts, Trash
  and Junk, as `GetConversation` without a folder. A `single-<id>` thread id (a
  draft row) is that one message.
- One action per request: `MailAction` = `archive` | `trash` |
  `delete-permanently` | `move {target}` | `mark-read {read}` |
  `flag {flagged}` | `tags {add, remove}` | `snooze {until, wake_folder_id}` |
  `junk {junk}` — absolute values, never toggles.
- `Request::ApplyMailAction { targets, action }` →
  `Response::MailActionQueued { result: MailActionResult { batch_id, op_ids,
  kind, message_ids, conversations, undo_until } }`.
  `Request::ResolveMailTargets { targets, action }` →
  `Response::MailTargets { resolution: { message_ids, account_ids,
  conversations } }` (changes nothing; confirmations show the count).
  `Request::UndoMailAction { batch_id }` → `Response::Queued`.
- Limits: `MAX_MAIL_ACTION_TARGETS` = 500 targets, `MAX_MAIL_ACTION_MESSAGES` =
  10,000 resolved messages. Errors carry `localized` ids
  `mail-action-issue-*` (`MailActionIssue`: `no-targets`, `too-many-targets`,
  `too-many-messages`, `no-messages`, `nothing-to-change`, `not-in-trash`,
  `undo-expired`, `undone-already`, `not-undoable`); labels of what an action
  did are `mail-action-kind-*` (`MailActionKind`), in `core-mail-actions.ftl`.
- The per-message requests (`UpdateFlags`, `MoveMessages`, `DeleteMessages`,
  `MarkJunk`, `SnoozeMessages`) stay unchanged and without undo; MCP, plug-ins
  and older callers keep working.

#### Which messages an action reaches (`MailAction::reach`)

| Action | Reach | Messages of a conversation target |
| --- | --- | --- |
| Archive, Move, Snooze, Mark junk / not junk | `listed` | its messages in the listed folder (virtual: in each member folder; no folder: everywhere but Sent, Drafts, Trash, Junk) — the user's replies in Sent stay |
| Delete (to Trash) | `whole` | every message the row counts: the listed ones plus the user's replies from Sent; never drafts |
| Mark read / unread, Tags (labels, categories) | `whole` | every message the row counts |
| Flag | `newest` | the newest message the user did not write, else the newest (`latest_message_id`, Gmail's star) |
| Unflag | `whole` | every message the row counts (clears every flag) |
| Delete permanently | `listed` | its messages in the listed folder; **every** resolved message must be in Trash or Junk, else the request fails (`not-in-trash`) |

- "Every message" means every stored copy (duplicates with the same
  Message-ID included), not only the copy the feed shows.
- Archive skips messages already in their account's Archive. Trash on messages
  already in Trash deletes them for good (as `DeleteMessages`); such a batch is
  not undoable — the UI uses Delete permanently with a confirmation there.
- **Toggles in the UI:** "toggle read" on a row = mark read when any message
  of the conversation is unread, else mark unread; "toggle flag" = unflag when
  the row is flagged (`ThreadSummary.flagged`), else flag.
- **Why the star is not on the user's own reply:** Gmail stars the newest
  message; Nail stars the newest one the user did not write (the row's
  selection since ADR 0153), so the flag stays in the listed folder (an Inbox
  "Flagged" filter, IMAP flags per folder).
- A conversation that gets new mail shows in the Inbox again (existing
  behaviour of the conversation list; unchanged).
- **UI scope:** the reader's toolbar, the list row, the row's context menu and
  the list / reader keys act on the **selected conversation** (the active
  folder as `folder_id`; no folder while searching). A card's own actions
  (header buttons, `context:thread-view.message`) act on **that message**.
  A list with `threads="off"` and a Design B child row act on the message.

#### One batch, one undo

- The core applies the change locally at once and queues it per account
  through the operation queue in one store transaction, recording each queued
  operation with its inverse in `mail_batches` / `mail_batch_ops` (schema v17,
  below) — the pattern of a rule run (`rule_run_actions`).
- `UndoMailAction` queues the inverses newest first in one transaction and
  marks the batch `undone`: every message back to the folder it was in (Trash,
  Archive, Junk, Snoozed → back; Snooze also cancels its wake timer; Junk also
  untrains), flags and tags as they were. An inverse that no longer applies
  (moved or deleted since) is skipped. Delete permanently is never undoable
  (`batch_id: null`).
- Batches are kept `MAIL_UNDO_SECS` (1 hour), at most `MAIL_UNDO_KEEP` (50);
  older ones answer `not_found` with `undo-expired`, a second undo `conflict`
  with `undone-already`. The undo is the actor's: a batch records its actor
  (`user`, `remote:<session>`, `mcp:<client>`, `plugin:<id>`); a remote
  session undoes only its own batches.
- The UI shows an undo toast with the action's label and the counts
  (`mail-actions.ftl`) and binds `mail.undo` (`z`) to the last batch.

#### Engines

The queue stays per message and account; replay groups what one server call
can do:

- **IMAP:** one `UID MOVE` / `UID STORE` per folder with a UID set (fall back
  to `COPY` + `STORE \Deleted` + `UID EXPUNGE` without MOVE), chunked.
- **POP3 / local folders:** local only (as today).
- **Gmail API:** `messages.batchModify` per label change (≤ 1,000 ids); when
  a batch covers every message of a Gmail thread in the account, `threads.modify`
  / `threads.trash`. Labels = tags; Archive = remove `INBOX`.
- **Exchange Graph / EWS:** per-item moves and updates in `$batch` (20 per
  call) / one `MoveItem` / `UpdateItem` with many item ids; categories = tags.
  Conversation-level APIs (EWS `ApplyConversationAction`) are not used: they
  act on the server's conversation, which differs from Nail's (folder + Sent).

#### MCP and plug-ins

- `update_messages` / `delete_messages` take `conversations:
  [{thread_id, folder_id?}]` beside `message_ids` (`McpConversationArg`; at
  least one of the two). They plan through `ActorWrites` as today: the
  confirmation summary names the action and the **message count** of the
  resolved conversations ("Archive 3 conversations (12 messages)"); destructive
  kinds (Trash, permanent delete) always ask; `kind` per `MailAction::action_kind`.
  The audit lists the resolved message count, never content.
- Plug-ins: `nail.mail.update` / `nail.mail.delete` accept
  `{ conversations: [...] }` the same way (plug-in API types and prelude in
  WP-actions-core).
- Until WP-actions-core, the MCP tools refuse non-empty `conversations`
  ("not supported yet") rather than ignoring them.

#### Remote sessions

- `ApplyMailAction` needs `organize` (`permanent-delete` for Delete
  permanently), `ResolveMailTargets` `read`, `UndoMailAction` `organize`.
- Message targets must be granted; a conversation target's folder must be
  granted; move / wake folders must be granted.
- **Decision on narrowed grants:** a conversation target resolves **within
  the grant** (the folder filter of `ListThreads` / `GetConversation`): the
  session acts on exactly the conversation it sees, and nothing outside the
  grant is touched. The request is **not** refused when the full conversation
  has messages outside the grant: the session's row was already narrowed
  (P2), so acting on the visible part is not a silent split from the user's
  point of view, and a refusal (or a "skipped" count) would reveal that hidden
  messages exist and make every conversation with a reply in a non-granted
  Sent folder unactionable. The answer's counts cover the visible part only.

### 2. Thread-activity table (conversation speed)

- **Schema v17** (this contract): `thread_activity (folder_id, thread_id,
  folder_last, last_date, message_count)`, primary key (folder, thread),
  `WITHOUT ROWID`; index `thread_activity_order (folder_id, last_date DESC,
  thread_id DESC)` and `thread_activity_thread (thread_id)`. `last_date` =
  MAX(the thread's newest `date_sort` in the folder, its newest in the Sent
  folders of the folder's account) — the row order of P2. Drafts folders have
  no rows. The migration backfills it from `messages`.
- **Query shape** (`nail_store::thread::activity`): one folder = `PAGE_SQL`
  (`WHERE folder_id = ?1 ORDER BY last_date DESC, thread_id DESC LIMIT
  OFFSET`, an index range, no temp B-tree; asserted by an `EXPLAIN` test) and
  `COUNT_SQL`; a virtual folder = `PAGE_SQL` per member with `LIMIT offset +
  limit`, merged in Rust; the page's rows are then read as today
  (`listed_sql`).
- **Maintenance (WP-perf, schema v18):** the table must equal the v17
  backfill query after every write — insert, delete, move, date change,
  re-threading (`thread_id` updates from JWZ merges), and a folder's role
  change. Recommended: SQLite triggers on `messages` and `folders.role`, so
  no write path in `nail-store` changes (WP-actions-core edits those); a
  property test compares the table with the backfill query after random
  operations.
- **Budget:** `thread-page-p95-ms` ≤ 10 ms on the standard corpus
  (`e2e/perf/budgets.json`, from 50 ms; WP-perf changes it with the
  implementation). `initial-sync-secs` must stay within its budget with the
  maintenance in place.

### 3. Card height: frames that report their height

Requirement: no mail script runs, anywhere; ADRs 0038 / 0041 / 0048 stay
intact (Trusted Types for `srcdoc`, remote content only through the signed
proxy, mail of the remote UI on the sandbox origin with an opaque origin).

- **Local frames** (`srcdoc`; desktop app and `nail serve` without remote
  login): `sandbox="allow-same-origin"` — never with `allow-scripts`. The
  reader reads `contentDocument.documentElement.scrollHeight` after `load`,
  after each image's `load` / `error` (capture listeners of the parent on the
  frame document) and when the frame's width changes, and sets the frame's
  height (clamped, `MAX_FRAME_HEIGHT`).
- **Remote frames** (`render_url` on the sandbox origin): the sandbox route's
  document carries one fixed script, pinned by its hash in the response's CSP
  (`script-src 'sha256-…'`), that posts `{type: "nail-frame-height", height}`
  to the app origin named by the `parent` query parameter (accepted only when
  it is one of the route's `frame-ancestors`), on load, on image loads and on
  resize. The CSP's `sandbox` becomes `allow-scripts allow-popups
  allow-popups-to-escape-sandbox` — still **without** `allow-same-origin` —
  and the iframe's attribute `sandbox="allow-scripts"`. The reader accepts the
  message only from its own frame's window and the sandbox origin
  (`ui/src/theme/frameHeight.ts`, `frameHeightOf`).
- Constants: `nail_api::remote_content::{FRAME_HEIGHT_MESSAGE,
  RENDER_PARENT_PARAM, MAX_FRAME_HEIGHT}` and their mirrors in
  `frameHeight.ts` (`SRCDOC_SANDBOX`, `REMOTE_SANDBOX`).
- The 40vh minimum of HTML cards (`threadViewSkin.ts`) and the
  `max(…, 50vh)` reader frame (`skin.ts`) go; the feed and the single-message
  reader scroll as a whole. Until a frame is measured it keeps a small
  minimum; a document taller than `MAX_FRAME_HEIGHT` scrolls inside. Wide mail
  keeps its horizontal scrollbar (maintainer decision).

**Security argument.**

- *No mail script runs.* Local: the sandbox lacks `allow-scripts`, so the
  browser runs no script, event handler or `javascript:` URL in the document,
  whatever its origin; the reader document's own CSP has `default-src 'none'`
  without `script-src`, and the page's CSP (inherited by `srcdoc`) allows only
  `'self'` scripts; the sanitizer removes scripts and handlers first. Remote:
  scripts are allowed only by hash, so only the fixed height script runs —
  inline handlers, `javascript:` URLs and any script the sanitizer missed are
  blocked (no `'unsafe-inline'`, no `'unsafe-hashes'`).
- *What `allow-same-origin` changes locally.* The document gets the app's
  origin instead of an opaque one. Without scripts it cannot use that origin
  (DOM, storage, cookies, IPC need script). Its requests are limited by the
  intersection of the page's CSP and the reader document's CSP to `data:` and
  the signed proxy route (the sanitizer drops relative URLs, so `/api/…`
  cannot be named); proxy requests now count as same-origin, which the proxy
  accepted already. Forms (no `allow-forms`, `form-action 'none'`), top
  navigation, popups and modals stay blocked by the sandbox. Trusted Types:
  `srcdoc` is still set through the `nail-reader` policy, and the reader never
  writes into `contentDocument` (it only reads sizes and adds listeners).
  The Tauri IPC is unreachable without script. The remote UI never uses this
  path (it always frames `render_url`); if a remote session ever lacks a
  `render_url`, the document's loads carry the session cookie only to the
  proxy, which is GET-only and token-signed.
- *What `allow-scripts` changes remotely.* The fixed script runs in an opaque
  origin on the sandbox host: `document.cookie` and storage throw, it can
  reach no API (the sandbox origin serves only `/render/` and the proxy), and
  the only thing it sends is one number to the parent (`postMessage` to the
  validated parent origin). The parent ignores every other message and clamps
  the height. Combining `allow-scripts` with `allow-same-origin` is forbidden
  on both paths (a test asserts the attribute values; the sandbox route's
  tests assert its CSP).

## Store and crate slots

Schema **v17** is this contract's (the two table groups above). Package
changes need new migrations: **v18 = WP-perf** (thread-activity maintenance),
**v19 = WP-actions-core** if it needs one; the integrator renumbers if one of
them is unused. `crates/nail-store/src/thread/activity.rs` (WP-perf) and
`crates/nail-store/src/mail_batches.rs` (WP-actions-core) are the empty slots.

The core answers the three requests with a **stub** until WP-actions-core
(`crates/nail-core/src/mail_actions.rs`): targets resolve per `reach` from
`GetConversation`'s messages (one copy per Message-ID) and are forwarded to the
per-message writes; no batch (`batch_id: null`), `UndoMailAction` answers
`unsupported`. Remote sessions already resolve within the grant. The UI mock
core implements the whole contract including undo
(`ui/src/api/mockMailActions.ts`).

| Package | Owns |
| --- | --- |
| WP-actions-core | `crates/nail-core/src/mail_actions.rs` (replaces the stub: every copy, one batch, undo, actor), the per-message write paths it needs in `nail-core/src/{mail,junk,snooze,gmail,replay,imap_engine}.rs` and `exchange/`, `remote_access.rs` (undo by the session's own batches), `crates/nail-store/src/{mail_batches.rs,queue.rs,mail.rs}` and a conversation-target resolution query (new file under `nail-store/src/thread/`, not `activity.rs` / `conversation.rs`), engine batching in `nail-proto-imap`, `nail-proto-gmail`, `nail-proto-graph`, `nail-proto-ews`; `crates/nail-mcp` (conversation targets in the write plans, confirmation summaries with counts) and `crates/nail-plugins` + `packages/plugin-api` (`conversations` in `nail.mail.update` / `delete`); `crates/nail-core/tests/mail_actions.rs`; `locales/en/core-mail-actions.ftl` additions; schema v19 if needed |
| WP-perf | `crates/nail-store/src/thread/activity.rs`, `thread/conversation.rs` (`PAGE_SQL` / `COUNT_SQL` / `list_conversations` on the table), schema v18 (maintenance), `crates/nail-core/src/conversations.rs` (`list_conversations` only), `crates/nail-bench`, `e2e/perf/budgets.json` (`thread-page-p95-ms` ≤ 10) |
| WP-actions-ui | `ui/src/actions/`, `ui/src/state/mailOps.ts`, `ui/src/widgets/MessageList*.tsx` (row actions and selection scope), the card **header actions** in `ui/src/widgets/ThreadViewFeed.tsx`, `ui/src/widgets/DeletePermanently.tsx`, `ui/src/features/menus/`, `ui/src/features/mail-actions/` (undo toast, `mail.undo`, confirmations), `ui/src/api/mockMailActions.ts` (if the mock needs more), `locales/{en,de}/mail-actions.ftl`, the `keys` / `menu` entries of `defaults/layout.kdl` for `mail.undo`, `e2e/tests/p3-actions.spec.ts`, `userDocs/en/mail.md` (conversation actions) |
| WP-card-height | `ui/src/theme/frameHeight.ts`, `ui/src/widgets/MessageView.tsx` (`Body`: the frames and their sizing), `ui/src/theme/reader.ts`, `trustedHtml.ts`, the body-height rules in `ui/src/widgets/threadViewSkin.ts` and `ui/src/theme/skin.ts`, `crates/nail-server/src/sandbox.rs` (+ `headers.rs` if needed; the hashed height script, `parent` check), `e2e/tests/p3-card-height.spec.ts` (local, remote UI on the sandbox origin, desktop smoke) |

Shared files: `crates/nail-api` changes only by agreement with this contract
(record them in the package ADR); generated bindings (`npm run gen:api`) and
`crates/nail-api/fixtures/default-layout.json` are regenerated by the
integrator after merging; visual baselines are updated only for the screens a
package's change touches.

## Consequences

- One request carries every conversation action; the UI's optimistic updates
  use the answer's `message_ids`, and the undo toast its `batch_id`.
- A remote session's narrowed conversation is acted on as it is seen.
- `ListThreads` becomes an index range per folder once WP-perf lands; the
  table adds a small cost to every message write.
- Local mail frames gain the app's origin (scripts stay off); remote mail
  frames gain exactly one hash-pinned script in an opaque origin.
