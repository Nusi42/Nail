# ADR 0153: P2 integration — conversations end to end

Status: accepted · 2026-10-05 · P2 WP205 (integrates ADRs 0149 core, 0150 list, 0151 reader, 0152 demo
conversations + e2e on the contract of ADR 0148)

## Context

The four P2 packages were built in parallel against the contract and the UI mock: WP201 made the core
answer `ListThreads` / `GetConversation` / `GetThread` with real conversations (the Sent join, "me",
`single-<id>` draft rows, remote narrowing, store v16), WP202 the Design A rows and the opt-in Design B
children, WP203 the card-feed reader with its migration offer, WP204 the demo conversations, `p2.spec.ts`
and the docs. Run together on the real core, a few seams showed.

## Decisions

- **Numbering.** The package ADRs are 0149 (core), 0150 (list), 0151 (reader) and 0152 (demo
  conversations, e2e); this one is 0153.
- **What a row acts on.** With the Sent join a conversation's newest message is often the user's own
  reply in Sent. Choosing an Inbox row selected that reply, so Archive, Delete, Move or a Gmail label went
  to the Sent copy (the Gmail e2e label test caught it). `ThreadSummary.latest_message` (and
  `latest_message_id`) is now the newest message the user did not write, else the newest; the row's
  date, preview and "Sender:" still show the newest activity ("me: …"). The store, the core's remote
  narrowing (`summarize_conversation`) and the UI mock follow the same rule.
- **A card's own subject.** Cards hide the message's subject under the feed's heading. An encrypted
  message with protected headers is stored with the placeholder subject "...", so its decrypted subject
  never showed. A card's `message-view` now shows `message-view.header.subject` when the message's
  subject, without `Re:` / `Fwd:` prefixes and list tags, differs from the conversation's
  (`cardShowsSubject`; `ReaderCard.subject` carries the heading).
- **j / k after reading older cards.** Moving through the feed selects messages that have no list row
  (older ones, the user's replies from Sent). The list now highlights the conversation's row for them
  (`selectedIndexOf` falls back to `selectedThreadId`), and `mail.next` / `mail.previous` continue from
  that row (`UiState.messageThreads`, set with `messageOrder`) instead of starting again at the top.
- **Design B's context menu.** `message-list.child` maps to the list row's menu (`DOM_PARTS`).
- **Reminder toasts** moved from the bottom left to the bottom right (`defaults/theme.kdl`,
  `features/pim/parts.ts`): at the bottom left they covered the navigation rail's Settings button (the
  three Exchange e2e failures that also happened on `main`).
- **"me" in narrow rows.** When the names of a row do not fit, "me" keeps its width (its
  `message-list.row.participant-group` carries the state `me`); the other names shrink alike.
- **Seeds.** `nail serve`'s seeds show the demo conversations (`nail_core::demo::DemoMail::WithConversations`,
  WP204's wiring patch); core tests keep the plain corpus.
- **E2E.** The reader's subject is the feed's `thread-view.subject` heading, or the message's own heading
  when a message shows alone: specs wait on `READER_SUBJECT` (`e2e/support/views.ts`).
  `p2-quickwin.spec.ts` now covers the strip through a strip-era user layout plus the one-time offer
  (both answers); `search-threads.spec.ts` reads the conversation in the feed. `p2.spec.ts` follows what
  the packages built: the feed opens the unread cards and the newest (Gmail-like, ADR 0151), Compact hides
  the preview line instead of removing it, the Design B chevron is pointer-only (`aria-hidden`), j / k
  focus the cards' header buttons, and Playwright types ":" for Shift+; (its "Shift+;" keeps the key ";").
  `p1-mail.spec.ts` checks that the page never scrolls (the feed does; a card's frame keeps 40 % of the
  window's height). `vault-recovery.spec.ts` shows the Mail view after the restart (the last view is
  remembered).

## Deferred

- Conversation-wide actions (archive / delete a whole conversation) need a core request; rows act on one
  message (ADR 0150).
- `nail-bench` measures `ListMessages` pages only; a `ListThreads` page metric is still to add (measured
  by hand: about 20–25 ms for a 100-row Inbox page on the 100k corpus, ADR 0149).
- Card frames of short HTML mail keep a minimum height (a sandboxed frame cannot report its height).
- `GetThread` with a `single-<id>` (printing a draft row) answers `not_found`, like the mock.
