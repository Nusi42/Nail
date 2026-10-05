# ADR 0149 (P2 WP204 e2e-seed): richer demo conversations, the P2 e2e spec and screens

Status: accepted · 2026-10-05 · P2 WP204 (package of the P2 wave; the integrator renumbers)

## Context

P2 (conversations, contract ADR 0148) turns the list into conversation rows
and the reader into a card feed. The demo corpus had one real conversation
(Project Nightingale, 7 messages across INBOX and Sent) and a lone sent
message ("Dinner on Saturday?"): too little to show — or test — long
conversations, folds, several participants, an unread message in the middle
or conversations the user started. WP204 owns the demo corpus
(`crates/nail-testkit`), the P2 Playwright spec, the conversation screens of
the visual harness and the user docs.

## Decisions

### The demo conversations

The new threads live beside the demo corpus, in
`crates/nail-testkit/conversations/<folder>/*.eml` (+ `flags.txt`, the corpus
layout), embedded by `build.rs` like the corpus:

| Conversation | INBOX | Sent | Archive | People | Unread |
| --- | --- | --- | --- | --- | --- |
| Lab move: timeline and packing | 9 | 3 | — | 5 | the 8th (Marie Curie), the newest is read |
| Project Nightingale kickoff (corpus, unchanged) | 4 | 3 | — | 3 | the newest |
| Reading group: October book | 4 | 1 | — | 4 (one non-ASCII name) | — |
| Dinner on Saturday? (started by the user in the corpus's Sent) | 2 | 2 | — | 2 | the newest |
| Talk proposal: Compilers for everyone | — | 1 | 2 | 2 | — |

- **Separate from the corpus, on purpose.** A first version added the files
  to `corpus/`: every seed picked them up, but 54 Rust tests outside this
  package broke — `nail-core` tests that wait for demo-multi's work inbox to
  hold exactly 15 messages (`engines.rs`, `m1b.rs`, `security.rs`) and then
  count further (`17` in the virtual account, `16` after a delivery, …),
  `nail-proto-imap`'s Sent fetch (4 messages, UIDs to 5) and `nail-mime`'s
  per-message snapshots. The corpus stays the small fixture those tests
  count on (`demo_corpus()`, `MailStore::demo()` unchanged);
  `demo_corpus_with_conversations()` / `MailStore::demo_with_conversations()`
  add the conversations after the corpus's messages per folder (corpus UIDs
  unchanged), `conversation_corpus()` returns them alone.
- **The seeds of `nail serve` show them** — `--seed demo`, `demo-imap`,
  `demo-multi` (the e2e suite, the visual harness, the maintainer's demo) — through
  a `DemoMail` choice in `nail-core/src/demo.rs` (`seed_demo_with`,
  `start_demo_imap_with`, `start_demo_multi_with`; the old functions keep the
  corpus) and `nail-cli/src/serve.rs` passing `DemoMail::WithConversations`.
  Those two files are outside this package: the exact change is handed to
  the integrator (it was applied and tested locally: `nail-core` demo tests,
  clippy, the e2e runs below).
- Replies quote their parent ("On …, X wrote:" + `>` lines), so the reader's
  quote folding has real input; no links. With the files in `corpus/`,
  `nail-mime`'s corpus tests (parsing, mutation fuzzing, no false link
  warnings) passed over them.
- Dates stay before 28 Sep 2026: the newest rows of every inbox are
  unchanged (specs and screens that look at the first rows keep working);
  the new conversations sort into the middle of the list.
- `nail_testkit::DEMO_CONVERSATIONS` describes them (per-folder counts,
  unread, participants) and `DEMO_USER_EMAIL` names the user; the crate's
  tests check the table against the corpus (references resolve, replies in
  date order, Sent holds only the user's mail, the unread message of the long
  conversation is the 8th), so core tests (WP201) and the e2e suite can rely
  on it.
- Counts with the conversations: INBOX 29, Sent 10, Archive 6 messages; the
  demo inbox lists 14 conversations (was 11), demo Sent 5 (was 2), demo-multi's
  unified inbox 17 (was 14), its work inbox 15 (was 12). Every spec that
  counted rows is adjusted (a11y, a11y-windows, accounts, composer, exchange,
  gmail, m2c, mail, mailops, p1-windows, p2-quickwin, rules, scheduling,
  search-threads, security, send, sync); they pass with the seed wiring
  applied and fail without it (the integrator applies both together).

### The P2 spec (`e2e/tests/p2.spec.ts`)

Written against the contract and the plan (ADR 0148; part names, catalog
texts, ARIA roles), on `--seed demo-multi`:

- conversation rows (participants with `me` / `unread` states, count pill,
  "Sender: preview", no count for one message, Compact = two lines without
  preview), the Sent folder's conversations;
- the card feed: `role="feed"` with `article`s (`aria-setsize` /
  `aria-posinset`), oldest first, the newest unread card open with its
  `message-view`, read cards collapsed (`aria-expanded`), the middle folded
  into "N older messages", Expand all / Collapse all, own replies from Sent
  `from-me` with "me", a conversation started by the user;
- keys j / k, Enter / o, `;` / `Shift+;`, `r` on the focused card;
- Design B: a user `layout.kdl` with `threads="expandable"` — the chevron
  (`message-list.row.expander`, "Show the conversation's messages"), child
  rows (`message-list.child`, states), a child shows that one message,
  ← / → close and open;
- the responsive standard on the feed at 1600 × 1000 and 1024 × 700;
- screenshots for the maintainer's review in Frost (light), Graphite and Nocturne (dark)
  plus the expandable list: `<build dir>/screens/p2/`.

Most of it can only pass after WP201 (counts across Sent, "me"), WP202
(rows, Compact, Design B) and WP203 (the feed, folds, keys) are integrated;
the spec is the integration's acceptance test, not a mirror of one package's
DOM. Where the contract leaves a choice open (how many cards stay visible
around the fold, the order of child rows) the spec checks the invariant
(shown + folded = 12; the unread child found by its text), not one layout.

### Visual screens

Two screens in `e2e/visual/screens.ts`: `conversation` (the Lab move
conversation selected: its row and the feed with the unread card open) and
`conversation-expanded` (Expand all). Opening the conversation marks Marie's
message read on the worker's shared server, so `markUnread` in
`support.ts` now takes search queries and resets that message too
(`CONVERSATION_UNREAD`). Their baselines need the integrated feed and are
written by the integrator (`npm run visual:update`); the existing screens'
baselines change with the seed (three more rows, two more unread messages)
and are regenerated together with WP202 / WP203's list and reader changes.

## Consequences

- Until the integrator applies the seed wiring (`DemoMail` in `demo.rs`,
  `serve.rs`), the adjusted row counts of the other specs fail on this
  package's branch (the seeds still show 14 / 11 / 12 rows).
- The visual baselines of the existing mail screens change with the seed
  (three more rows, two more unread messages) and are regenerated by the
  integrator together with WP202 / WP203's list and reader changes; the two
  conversation screens get their first baselines then.
- `CLAUDE.md`, `docs/dev-setup.md` and the M5d notes say "22 screens"; with
  the two conversation screens it is 24, and the `nail-testkit` line of
  `CLAUDE.md` gains `conversations/` (integrator).
