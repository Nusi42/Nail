# ADR 0169: P3 integration — conversation actions, conversation speed, card height

Status: accepted · 2026-10-05 · P3 integration (integrates ADRs 0165 actions core, 0166 thread-activity speed,
0167 actions UI, 0168 card height on the contract of ADR 0164)

## Context

The maintainer decided that actions on a conversation row behave like Gmail: until P3, Archive, Delete
and the other row actions reached only the conversation's newest message (ADRs 0150, 0153). The four P3
packages were built in parallel against the contract and the UI mock: WP-actions-core made `ApplyMailAction`
real (every stored copy, one batch, undo per actor, batched engine writes, MCP and plug-in conversation
targets), WP-perf maintained `thread_activity` by triggers and moved `ListThreads` onto it, WP-actions-ui wired
every row, toolbar, key and menu action to conversation targets with the undo toast, and WP-card-height sized
HTML mail frames to their content.

## Decisions

- **Numbering.** The package ADRs are 0165 (actions core), 0166 (thread-activity speed), 0167 (actions UI) and
  0168 (card height); this one is 0169. The schema stays at v18 (WP-perf's triggers and rebuild):
  WP-actions-core needed no migration, so the contract's v19 slot is unused.
- **Labels and categories act on the conversation.** WP-actions-ui left the Gmail labels window
  (`gmail.apply-labels`, `l`) and the Exchange categories window on the selected message (`UpdateFlags` on one
  id). Gmail labels a whole conversation, so both windows now remember the targets when they open
  (`features/mail-actions/tags.ts`, like the move window) and apply the change as one `ApplyMailAction`
  `tags` batch: the selected conversation, the picked rows or a card's message. They get the undo toast like
  every other action. The check boxes still show the selected message's labels.
- **No misleading keys on a card's menu.** The card menu listed the global keys (`E`, `Del`, `V`, `S`) next to
  "Archive this message" and the like, but those keys act on the whole conversation. A card's menu now shows no
  key for the conversation-wide actions (`CONVERSATION_KEY_ACTIONS` in `features/menus/resolve.ts`); Reply,
  Reply all and Forward keep theirs (their keys answer the card one is on).
- **Plug-in SDK.** `nail.mail.update` / `delete` take `MessageId | MessageId[] | PluginMailTargets`
  (`{ messageIds?, conversations?: { threadId, folderId? }[] }`); the test host resolves conversations to the
  thread's messages (in `folderId` when given). The API reference is regenerated.
- **Frame origin.** `frameHeightOf` accepts `event.origin === "null"`: the remote frame's document runs on the
  sandbox origin without `allow-same-origin`, so it posts from an opaque origin; the sender is identified by
  its frame's `contentWindow` (ADR 0168; noted in ADR 0164).

## Consequences

- One undo covers every message an action touched, for the actor that ran it (`user`, `remote:<session>`,
  `mcp:<client>`, `plugin:<id>`); batches expire after an hour.
- Gmail labels and Exchange categories set from Nail reach the user's Sent replies of a conversation too
  (tags are a "whole" reach).
- The visual `confirmation` screen moves the sample to Trash first (Delete permanently asks only there) and
  undoes the move in the new `Screen.close` hook; its baselines and the compact `menu` baselines (card More
  button, shorter plain-text card) are updated.
- Deferred: Gmail `threads.modify` and batched `messages.trash`; lists whose Sent folders a remote grant
  narrowed page at P2 speed; the composer preview frame keeps its fixed height.
