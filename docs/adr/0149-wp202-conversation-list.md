# ADR 0149 (WP202): the conversation list — Design A rows, Design B children

Status: accepted · 2026-10-05 · P2 WP202 (list). The integrator renumbers.

## Context

ADR 0148 settled what a conversation is and gave the list `threads="off" |
"collapsed" | "expandable"`, the template field `{participants}` with the
filters `names(n)` / `if-many`, and the part names. WP202 builds the rows the
maintainer approved (plan, "Wave P2"): Design A as the default and Design B as the
opt-in.

## Decisions

### Design A rows (the default layout)

- Wide windows, three lines: `{participants | names(3)}  {count | if-many}
  {date | relative}` / `{subject}` / `{latest_from | names(1) | then(":")}{preview}`.
  Narrow windows keep two lines (people + count + date, subject).
- **New template filter `then("text")`**: writes `text` after a non-empty
  value, inside the field's own element (`message-list.row.suffix`), so "Grace
  Hopper:" stays one piece and a message without a sender shows no stray
  colon. Its argument is the first quoted-text argument of the template
  language (the table in `ui/src/template/template.ts`).
- **New template field `latest_from`** on message rows too (the message's own
  sender), so one set of templates serves conversation rows and search hits.
- Unread senders are bold on line 1 only; "Sender:" on line 3 keeps the
  normal weight. The people and the sender are flex boxes whose names
  ellipsize one by one (a single ellipsis on the whole line made the names
  and separators overlap in the responsive check).
- **Compact = two lines**: each line carries `data-fields` (the field parts
  of its template); the skin hides lines with `preview` in Compact. This also
  hides a user's own preview line in Compact, which is what Compact means.
- A conversation row's tag line shows the newest message's labels and
  invitation / warning marks (from `latest_message`).
- **Actions on a conversation row act on its newest message** (the contract's
  default, kept): Delete / Archive / Move on a row therefore move one message.
  Conversation-wide actions would need core requests (all messages of a
  conversation in a folder) and are left for a later wave; noted for the
  integrator.

### Design B (`threads="expandable"`)

- The list becomes a **tree** (`role="tree"`, rows `treeitem` with
  `aria-level`; conversation rows of more than one message carry
  `aria-expanded`; children `aria-level=2` with `aria-posinset` /
  `aria-setsize`). The other modes stay a `listbox` of `option`s.
- The chevron (`message-list.row.expander`, a third grid column, turned down
  while open, invisible for one message) is pointer-only and `aria-hidden`:
  the tree item says `aria-expanded` and the keyboard opens / closes it.
- Children (`message-list.child` with `.rail`, `.from`, `.preview`, `.date`;
  states `selected`, `unread`, `flagged`, `from-me`) come from
  `GetConversation` for the listed folder (one query per open conversation),
  **oldest first** like the reader, one line each, the rail under the
  conversation's avatar. "me" for the user's own messages.
- **The reader shows a chosen child alone**: choosing a child selects its
  message and sets the selected thread to `null`, so the reader's
  conversation strip / card feed disappears; choosing the conversation row
  selects its newest message with the thread, as in Design A.
- Open conversations are remembered per UI store and list id, so a list that
  remounts (breakpoint switch, view change) keeps them open; another folder
  starts closed.

### Selection model and keys

- A conversation row and its newest child select the **same message**, so the
  list keeps the chosen row's key and falls back to the first row of the
  selected message when the selection changed elsewhere (the reader's j / k, a
  delete). `UiState.messageOrder` holds each message once, in row order.
- **j / k stay `mail.next` / `mail.previous`** (the `list` context bindings
  other packages' tests rely on). An expandable list catches the keys bound to
  those two actions in the `list` context before the global dispatcher and
  steps through its own rows (children included); other lists keep the
  built-in behaviour.
- **→ / ←** are new actions `list.expand` / `list.collapse` (feature
  `conversation-list`, bound in `keys context="list"`, labelled and with
  icons in the layout's `actions`): → opens a conversation, then moves to its
  first child; ← moves from a child to its conversation, then closes it
  (WAI-ARIA tree keys). Closing a conversation under a selected child selects
  the conversation. They do nothing outside an expandable list.

## Consequences

- The default list's rows change (people instead of the newest sender, the
  "Sender:" prefix); visual baselines of the mail screens follow.
- Right-click on a child row needs `"message-list.child": "message-list.row"`
  in `ui/src/features/menus/targets.ts` (not this package's file; requested
  from the integrator).
- `userDocs/en/templates-and-parts.md` needs the `latest_from` field, the
  `then("…")` filter and the Design B parts (WP204 owns the user docs).
- E2E: `e2e/tests/p2-list.spec.ts` (Design A rows + Compact, Design B open /
  child / keys, the responsive standard at every size in Frost, Graphite
  Compact, Nocturne, Frost Compact and High contrast).
