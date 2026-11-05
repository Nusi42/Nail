# ADR 0148: P2 contract — conversations, the quick win and the package slots

Status: accepted · 2026-10-05 · P2 WP200 (contract + quick win)

## Context

Review finding at the M5 human gate: the threading of mail was too cramped. The
maintainer approved the proposal "Conversations that breathe" (plan section 10, "Wave
P2 (conversations)"; memory `p2-threading-decision`): the quick win (Design C)
first, then Design A (one list row per conversation, the reader as a card feed),
then Design B (opt-in rows that expand into their messages). The maintainer's
defaults for the open questions: oldest first; conversations in every folder
except Drafts and Outbox; the user's own replies from Sent appear in the
conversation, marked `from-me`.

This stage ships the quick win so an improvement can be reviewed early, and the
contract the four parallel packages build against.

## Decisions

### What a conversation is

- A **conversation listed in a folder** is the thread's messages in that folder
  (or in the member folders of a virtual folder) **plus the user's own messages
  of the thread from the Sent folders of the same accounts**. The Sent messages
  carry `from_me`. Nothing else joins: an archived copy in another folder is that
  folder's business, so the count pill and the cards always agree.
- **Drafts never join**; listing Drafts shows every draft as a conversation of
  its own. The Outbox is no folder (its messages are never listed), so nothing
  is said about it beyond that.
- **Without a folder** (search hits, MCP, the reader while searching) the
  conversation is the thread's messages in every folder but Drafts, Trash and
  Junk.
- Copies with the same Message-ID count once (as before: the oldest copy, the
  others in `duplicates`).
- **"me"** is resolved by the core from the identities of the messages'
  accounts (`accounts.kdl`; compared case-insensitively). A message is
  `from_me` when it sits in a Sent folder or its sender is one of its account's
  identities.
- Rows are ordered by the newest message of the conversation (the user's own
  replies count as activity); the row's `date`, `preview`, `latest_message_id`
  and `latest_from` describe that message.

### Protocol (`nail-api`)

- `ThreadSummary.participants` is now `Vec<ThreadParticipant>`:
  `{name, email, me, unread}` — the address flattened beside the two flags, so
  readers that expect an `Address` keep working (the MCP thread resource gains
  the two fields). `unread` = the person sent an unread message of the
  conversation.
- `ThreadSummary.latest_from: Option<ThreadParticipant>` — the newest message's
  sender ("Sender: preview"). `message_count` counts the conversation as defined
  above; `unread_count` its unread messages.
- `ThreadSummary.latest_message: Box<MessageSummary>` — the newest message itself.
  Choosing a conversation row selects it, and the UI's actions (flags, junk,
  "Create rule from message", Exchange / Gmail actions) look the selected
  message up in the caches (`findSummary`): before this field a thread row gave
  them nothing until the reader's conversation request answered, and an action
  menu opened right after the click lacked those actions. `patchSummaries`
  patches it optimistically like the other caches. Actions on a conversation
  row act on this message (as before P2); conversation-wide actions are WP202's
  call.
- `Request::GetConversation { thread_id, folder_id: Option<FolderId> }` →
  `Response::Conversation { thread, messages: Vec<ConversationMessage> }`,
  oldest first. `ConversationMessage` is a `MessageSummary` flattened beside
  `from_me`, so the UI's caches, optimistic patches and `findSummary` treat it
  as a summary. Remote sessions: `read` scope; a given folder must be granted;
  messages outside the grant are dropped (none left = `forbidden`).
- Remote sessions: `ListThreads` answers are not narrowed per message today
  (a granted folder's rows). Once the Sent join exists, WP201 narrows each row
  to what the grant covers (a Sent folder outside the grant contributes
  nothing, `latest_message` / `latest_from` / participants recomputed or the
  row dropped), like `Response::Thread` / `Conversation`.
- `GetThread` stays as it was (every stored copy, all folders), for plug-ins,
  MCP and the print view.
- No store migration in the contract. WP-core may add `v16` (e.g. an index for
  the Sent join) if its queries need one.

**Stubs:** the store fills `participants[].unread` and `latest_from`; `me` stays
`false` and `message_count` covers the listed folder only until WP-core
(`crates/nail-core/src/conversations.rs` answers `GetConversation` with
`GetThread`'s messages, none `from_me`). The UI mock core implements the whole
contract (`ui/src/api/mockMail.ts`), and `addMockConversation`
(`ui/src/api/mockConversations.ts`) adds a six-message conversation across Inbox
and Sent plus a draft that never joins it.

### Layout, templates, parts

- `message-list threads=` takes `off` | `collapsed` | `expandable` (`expanded`,
  the pre-P2 spelling, still parses and means `expandable`). The default layout
  uses `collapsed` in both breakpoints. Until WP-list builds Design B,
  `expandable` behaves like `collapsed`.
- Template field `{participants}` (a message row: its sender) and two filters:
  `names(n)` — at most `n` names, with more the first `n - 1` and the newest,
  "me" for the user (Fluent `conversations-me`), parts
  `message-list.row.participants` > `.participant-group` > `.participant`
  (states `me`, `unread`) and `.separator` (`conversations-names-separator` /
  `-gap`); `if-many` — a number only when it is more than 1 (the count pill
  `message-list.row.count`, empty and hidden for one message). The plan's
  `names N` is written `names(N)`, like `truncate(n)`.
- Part names reserved for the packages: `message-list.row.participants`,
  `.count`, `.expander`; `message-list.child` (+ `.from`, `.preview`, `.date`,
  `.rail`; states `selected`, `unread`); `thread-view.card` (states `expanded`,
  `unread`, `selected`, `from-me`) with `.header`, `.avatar`, `.recipients`,
  `.actions`; `thread-view.fold`; `thread-view.participants`. `thread-view.item`
  (and its sub-parts) stays an alias for one release after the card feed lands:
  `ui/src/theme/partAliases.ts` (`PART_ALIASES`, empty in the contract) makes the
  theme compiler emit every part rule that names an old part also for the new
  one (selector and ancestors); WP203 adds `"thread-view.item":
  "thread-view.card"` with the card feed, the next wave removes it.
- Styling moved out of `defaults/theme.kdl`: its `thread-view.*` rules (a grey
  band, `max-height: 40vh`, own scroll) are gone; the default look is in the UI
  skins, on tokens: `ui/src/widgets/threadViewSkin.ts` (`THREAD_VIEW_SKIN_CSS`)
  and `ui/src/widgets/messageListSkin.ts` (`MESSAGE_LIST_SKIN_CSS`), emitted
  right after the shared skin, before every theme's part rules. The strip's
  avatars use `--nail-skin-avatar-sm` = `size.avatar` × 28 / 34 (28 px
  Comfortable, 23 px Compact) and the avatar palette. No new colour pairs: the
  strip and the pill use `fg` / `fg-muted` / `accent` on `bg` / `bg-subtle` and
  `selection-fg` on `selection`, all in `tests/theme_contrast.rs` already.
- Fluent: `locales/{en,de}/conversations.ftl` (shared words: "me", separators,
  counts `conversations-count` / `-earlier` / `-older`, expand / collapse, the
  three list modes), `conversation-list.ftl` (WP-list) and
  `conversation-reader.ftl` (WP-reader); `de` empty; all embedded in
  `nail-config/src/l10n.rs`.

### The quick win (Design C, shipped here)

- The list shows conversations (`threads="collapsed"`): row 1 = the newest
  sender (`from` of a thread row is now `latest_from`), the count pill
  (`{count | if-many}`), the date.
- The reader's strip (`ThreadView.tsx`) asks for `GetConversation` with the
  active folder (no folder while searching), shows the newest three messages and
  folds the older ones into "N earlier messages" (`thread-view.fold`, a button;
  the selected message is never folded away), 28 px avatars
  (`thread-view.item.avatar`), body-size text, rows padded by `space.row`, no
  band, no scroll area of its own, the strip aligned with the reader's column.
  The user's own messages say "me" and carry the state `from-me`.

## Slots of the parallel packages

| Package | Owns |
| --- | --- |
| WP201 core | `crates/nail-store` (thread queries, the Sent join, participants, optional migration `v16` + its tests), `crates/nail-core/src/conversations.rs` (+ `list_threads` / `get_thread` in `mail.rs`, identities for "me"), new `crates/nail-core/tests/conversations.rs`, `crates/nail-mcp` if a conversation tool or resource changes; `crates/nail-api` only for fixes agreed with this contract (record them in the package ADR) |
| WP202 list | `ui/src/widgets/MessageList.tsx`, `messageListMeasure*.ts`, `messageListSkin.ts`, `ui/src/template/` (filters), `ui/src/features/conversation-list/`, `locales/{en,de}/conversation-list.ftl`; in `defaults/layout.kdl` the `message-list` nodes (Design A rows) and `keys context="list"`; in `crates/nail-config/src/widgets/core.rs` the `message-list` entry |
| WP203 reader | `ui/src/widgets/ThreadView.tsx` (and new `ThreadView*.tsx`), `threadViewSkin.ts`, the alias entry in `ui/src/theme/partAliases.ts`, a `thread-view.card` entry in `ui/src/features/menus/targets.ts` (right-click menu `context:thread-view.message`), the changes `MessageView.tsx` needs to render inside a card, `ui/src/features/conversation-reader/` (actions, the reader-stack migration offer for user layouts), `locales/{en,de}/conversation-reader.ftl`; in `defaults/layout.kdl` the reader stacks and `keys context="reader"`; in `widgets/core.rs` the `thread-view` entry |
| WP204 e2e | `e2e/tests/p2.spec.ts`, conversation screens in `e2e/visual/` (+ baselines), richer threads in the demo seeds (`crates/nail-testkit` corpus / demo seed) with the row counts other specs expect, `userDocs/en/mail.md` and the conversation sections of the reference docs |

Shared files: `crates/nail-api/fixtures/default-layout.json` and the
`nail-config` golden files follow `defaults/layout.kdl` — WP202 and WP203 each
regenerate them (`NAIL_UPDATE_SNAPSHOTS=1 cargo test -p nail-config --test
defaults`, `NAIL_UPDATE_GOLDEN=1 cargo test -p nail-config --test layout_edit
--test write_back`); the integrator regenerates them after merging instead of
resolving conflicts by hand. Visual baselines likewise: each package updates only
the screens its change touches.

## Consequences

- Every list in the default layout shows conversations; e2e specs that counted
  message rows now count conversation rows, and specs that act on an older
  message of a conversation choose it through the reader's strip.
- Visual baselines of the mail screens changed (count pill, strip) — regenerated
  here for the intended change only.
- Older cores answer `GetConversation` with `unsupported`/an unknown request; the
  UI here always talks to a core of the same version, so no fallback is kept.
