# ADR 0151 (WP203): the reader as a card feed

Status: accepted · 2026-10-05 · P2 WP203 (reader; the integrator renumbers)

## Context

P2 Design A (plan section 10, "Wave P2"; contract ADR 0148) turns the reader
into a card feed: the conversation's subject and participants on top, the
messages as cards oldest → newest without indentation, open cards reusing
`message-view` with their own security badges, remote-content banner and
actions, older read ones collapsed to one line, long collapsed runs folded
into "N older messages", `role="feed"` / `article`, keys j / k, Enter / o,
; / Shift+;, n / p, r / a / f. Layouts of the user's keep the strip-era
reader stack and get an offer to migrate.

## Decisions

### One widget, two modes

- `thread-view mode="cards"` is the card feed (`ui/src/widgets/ThreadViewFeed.tsx`);
  `mode="strip"` — the default when the option is missing — is the quick-win
  strip above a separate `message-view`, so every `layout.kdl` written before
  P2 keeps working unchanged. The default layout's reader stacks are now
  `toolbar` + `thread-view id="thread" mode="cards"` (`sizes="auto 1fr"`), in
  both breakpoints.
- New options (nail-config `widgets/core.rs`): `mode` (`strip` | `cards`),
  `card` (words: which of `security-message security-warnings remote-content
  junk-verdict sync-message-body message-view` an open card shows, in that
  order by default; `CARD_WIDGETS` in `ThreadViewModel.ts` and the Rust list
  are checked against each other), `headers` (the card's `message-view`
  header fields, default none — the card's header shows sender, recipients
  and date) and `mark-read`.

### Per-card widgets through a scoped UI state

The reader's per-message widgets (also other packages': security, remote
content, junk, sync, scheduling's invitation card, plug-in banners) read the
selected message from the UI state. Inside a card they get a view of the UI
store whose `selectedMessageId` is the card's message
(`ThreadViewScope.tsx`, `scopedUiStore` + `CardScope`); every other field and
every write is the app's store. No widget outside this package changed.
Actions run through the app's dispatcher and act on the app's selection, so
**focus = selection**: pointer-down or focus inside a card selects its
message (not on the fold at the card's top, which stands for other
messages). The reader's toolbar, r / a / f, right-click menus and widget
buttons inside a card therefore act on that card. `MessageView` inside a card
(`useReaderCard()`) renders a `div` without its own label and without the
subject heading (the feed's header has it), and folds quoted lines of plain
text bodies (`message-view.quote-block` / `.quote-toggle` / `.quote`,
`.text-block`) — the reply structure without indentation.

### Which cards are open

- Open at first sight: every unread message, the newest message and the
  message the list selected. Decided once per message and remembered per
  conversation (`feedStore` in `ThreadViewModel.ts`), so reading a card (which
  marks it read) never collapses it. The feed scrolls to the selected message
  when the list chose an older one, else to the newest unread, else the
  newest.
- Folds: a run of more than four collapsed cards in the middle (never the
  first or the newest card) folds into "N older messages"; the collapsed card
  right before the next open one stays visible as context. A selected card
  inside the folded part keeps the run open. The fold button sits at the top
  of that context card's `article` (a feed owns only articles).
- While the conversation loads the reader is empty (no flash of the message
  alone); without a thread, on an error, or for a message outside the
  conversation the widgets show the selected message alone
  (`data-mode="single"`), as the reader did before.

### Accessibility and keys

- `section` (region "Conversation") > header (`h1` subject, participants with
  "me" / unread states, count, Expand all / Collapse all) > `role="feed"`
  labelled by the subject > `article` per card with `aria-setsize` /
  `aria-posinset` (positions count folded cards too), `aria-labelledby` its
  header button, `aria-current` on the selected card. A card's header is a
  button with `aria-expanded`; Enter / Space on it toggles natively. An open
  card has a group of icon buttons Reply / Reply all / Forward (labels from
  the layout's actions).
- Actions (`ui/src/features/conversation-reader/actions.ts`), bound in
  `keys context="reader"`: `conversation.next-card` / `previous-card` (j / k:
  card headers and folds in feed order; without a card feed they step the
  list like `mail.next` / `mail.previous`), `conversation.next-message` /
  `previous-message` (n / p: every message, opening a fold on the way and
  focusing the card once it shows), `conversation.toggle-card` (o),
  `conversation.expand-all` (;) and `conversation.collapse-all` (:). The key
  matcher drops Shift for symbols, so the plan's Shift+; is bound as `:`
  (US layout; on a German layout `:` is Shift+. and `;` Shift+,). Enter is not
  bound: it would also catch Enter on the buttons inside a card. r / a / f
  stay the global reply keys and act on the focused card through the
  selection. Each action has a default `action` entry with a Lucide icon.

### Parts and styling

`thread-view[data-mode]`, `.header`, `.subject`, `.participants`,
`.participant` (states `me`, `unread`), `.participants.separator`, `.count`,
`.expand-all`, `.feed`, `.card` (states `expanded`, `unread`, `selected`,
`flagged`, `from-me`), `.card.header`, `.card.toggle`, `.card.avatar`,
`.card.from`, `.card.recipients`, `.card.preview`, `.card.date`,
`.card.actions`, `.card.action`, `.card.action.icon`, `.card.body`,
`.fold`. Default look in `threadViewSkin.ts` on tokens only: bordered cards
with `radius.lg`, rows padded by `space.row`, 28 px avatars, the selected card
with an accent border, the newest open card stretching to the pane's end (a
short conversation fills the reader), HTML bodies at least 40 % of the
viewport high (a sandboxed frame cannot report its height). Container queries
on the reader (`nail-reader`): below 640 px the recipients hide and paddings
shrink; below 480 px only Reply stays in a card's header (Reply all /
Forward are in the reader's bar). No new colour pairs (fg / muted / accent on
bg / bg-subtle, accent borders). `PART_ALIASES` maps `thread-view.item` →
`thread-view.card` for one release; the right-click menu of a card is
`context:thread-view.message` (`menus/targets.ts`), inside an open card's body
the `message-view` menu.

### Migration of user layouts

`features/conversation-reader/readerStack.ts` finds stacks that hold a
strip-mode `thread-view` beside a `message-view`; `ReaderOffer.tsx` (an app
effect, the M5b offer's card `layout-offer`) offers once to switch: one
`UpdateLayout` per click (one undo entry) that adds `thread-view mode="cards"`
(the strip's id and label, `card=` the stack's visible per-message widgets in
their order unless they are the default six, `mark-read=#false` carried over)
where the strip was and then removes the strip and those widgets, from the
end; stacks of a breakpoint from the last. Checked against nail-config's
editor (the result equals the shipped default stacks). Answers are remembered
under the UI-state key `conversation-reader.layout-offer`; the offer waits
while the M5b layout offer is pending.

## Consequences

- The default reader has no `message-view` node of its own any more; the
  subject is `thread-view.subject`, not `message-view.header.subject`, when a
  conversation is shown. E2E specs that look for the subject in
  `message-view.header.subject`, for `thread-view.item`, or that press j / k
  in the reader to step the list need adjusting (listed for WP204 / the
  integrator).
- The list's selection highlight follows `selectedMessageId`; moving through
  cards selects older messages (also Sent ones that are no list row), so
  WP202 should highlight the conversation row by `selectedThreadId` and step
  from it.
