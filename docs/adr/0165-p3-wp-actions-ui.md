# ADR 0165 (P3 WP-actions-ui): conversation actions in the UI

Status: accepted · 2026-10-05 · P3 WP-actions-ui (contract ADR 0164; the
integrator renumbers)

## Context

ADR 0164 makes every row action Gmail-like: the reader's toolbar, the list
row, the row's menu and the keys act on the selected conversation through
`ApplyMailAction`; a card's own actions act on its message; one batch, one
undo. This package wires the UI to it against the contract's mock core.

## Decisions

- **Targets** (`ui/src/actions/mailTargets.ts`). The message list publishes
  how its rows map to targets (`ListScope`: conversations or messages, the
  listed folder; `null` while searching) and the rows picked with Ctrl /
  Shift in a small store per UI store. `currentTargets(ui)` answers, in
  order: the open context menu's target (a card or something inside one →
  that message; a Design B child row → its message; a row among the picked
  ones → all picked rows; another row → its conversation), else the picked
  rows, else the selection — `{thread, listed folder}` when the list shows
  conversations and a thread is selected, else the message. The reader
  selecting an older card keeps the selected thread, so the toolbar still
  acts on the conversation. Without a list (pop-outs, tests) actions act on
  messages.
- **One runner** (`features/mail-actions/run.ts`): every built-in write
  (`mail.archive`, `mail.delete`, `mail.toggle-read`, `mail.mark-read`,
  `mail.mark-unread`, `mail.toggle-flag`, `mail.move`, `mail.move-to`) and the
  triage ones (`junk.mark`, `junk.not-junk`, the snooze dialog) send one
  `ApplyMailAction`. Message targets use it too, so every action has the
  same undo. The runner updates the caches at once: rows that leave the
  listed folder go (`patchThreads` / `patchSummaries`), the selection moves
  to the next row (`nextSelection` over `messageOrder`), a card's own
  removal selects the next card of the conversation (the conversation row
  goes only when no card is left); flags / read state patch the row and the
  messages the answer names. While searching only Trash and Delete
  permanently take hits out. A refusal shows the core's `localized` text in
  the toast and refetches.
- **Toggles**: toggle read = mark read when any target is unread (the
  conversation row's `unread_count`), else unread; toggle flag = unflag when
  every target is flagged, else flag. `mail.mark-read` / `mail.mark-unread`
  are enabled only when they would change something (hidden in the card menu
  otherwise).
- **Delete in Trash** asks like Delete permanently. `askDeletePermanently`
  resolves the targets first (`ResolveMailTargets`) and the question names
  the count ("Delete this conversation's 12 messages permanently?"); a
  refusal (`not-in-trash`) shows in the toast instead of a question.
- **Undo toast** (`features/mail-actions/Toast.tsx`, an effect): "Conversation
  archived", "3 conversations moved to Trash", "Message moved to Trash"
  (`mail-actions-done-<kind>` with the conversation and message counts), Undo
  while the answer has a `batch_id`, a dismiss button; 8 s (4 s for "Action
  undone"), held while hovered or focused; `role="status"` (`alert` for
  errors). It is a manual popover (top layer, above every window, also modal
  confirmations); environments without popovers render it in place. Its
  look is `MAIL_ACTIONS_SKIN_CSS` (`features/mail-actions/skin.ts`, compiled
  after the other skins by a one-line addition to `theme/compile.ts`). `mail.undo`
  (`z`) undoes the last batch until `undo_until`; undo restores the
  selection the action had when it came from the list.
- **Card actions**: each open card's header gets a **More** button
  (`data-action="more"`) that opens `context:thread-view.message` for that
  card (keyboard-opened when activated by key); that menu now offers Mark
  this message unread / read, Flag, Archive / Move / Delete this message and
  Mark as junk, all on the card's message. Right-clicks inside a card's body
  act on the card's message too. The junk verdict's **Not junk** passes its
  message id (`junk.not-junk "<id>"`), so it acts on that message inside a
  card.
- **Multi-select**: Ctrl / Cmd + click toggles a row, Shift + click and
  Shift + ↑ / ↓ pick a range from the last chosen row; picked rows show as
  selected (`aria-selected`, `data-state~="selected"`), the list is
  `aria-multiselectable`. Picks are dropped on a plain click, another folder
  or mode, and after a removal; rows that disappear are pruned.
- **Move window**: `mail.move` remembers its targets when it opens
  (`features/mail-actions/move.ts`); the folder picker moves those.
- **Default layout** (the contract gives this package `defaults/layout.kdl`'s
  keys / menu entries): Gmail's keys `#` (delete; before `delete`, so the
  tooltips keep naming Del), `Shift+I` / `Shift+U`, `!` (junk), `l` (Gmail
  labels), `z` (`mail.undo`); action metadata for the three new actions; the
  card menu above. `crates/nail-api/fixtures/default-layout.json` is
  regenerated accordingly (`NAIL_UPDATE_SNAPSHOTS=1 cargo test -p nail-config
  --test defaults`).

## Consequences

- The per-message requests are no longer sent by the UI's actions
  (`UpdateFlags` remains for "opening marks read"); older tests expecting
  them were updated.
- Until WP-actions-core replaces the core stub, the real core answers
  `batch_id: null`: the toast shows no Undo and `z` does nothing there; the
  mock core and the unit tests cover undo, `e2e/tests/p3-actions.spec.ts`'s
  undo step needs WP-actions-core.
- The Gmail labels dialog (`gmail.apply-labels`, `l`) and Exchange
  categories still act on the selected message (their dialogs are other
  packages'); a follow-up moves them to `ApplyMailAction` `tags` with the
  conversation target.
