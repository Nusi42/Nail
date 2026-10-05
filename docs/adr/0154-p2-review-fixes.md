# ADR 0154: P2 review fixes — conversations that breathe, for real

Status: accepted · 2026-10-05 · P2 review fixes (on ADRs 0148–0153)

## Context

The two P2 reviews measured the integrated wave on the real core (demo-multi, every theme, 400 px to
1600 px) and on the store. They found the open card stretching, two keyboard dead ends, cut-up names in
the list, cramped Design B children, a send-as identity counted as "me", a list mode that silently
changed meaning, the conversation list's cost unmeasured, and two smaller issues (the fold drawn inside
a card, a remote narrowing that collapsed copies first).

## Decisions

- **Cards are as tall as their message.** The rule that let the last open card grow to the reader's
  height is gone, and the in-card text grid aligns its rows to the start, so "Show quoted text" sits
  right under the text (it floated about 270 px below it). A short conversation leaves the reader's
  background below the last card.
- **The fold is a line between cards.** "N older messages" is its own `article` of the feed
  (`thread-view.fold-row`, holding the `thread-view.fold` button) with dashed rules on both sides, not
  the top strip of the next card (screen readers announced it as part of that message). Opening it
  moves the focus to the first card it revealed (the focus fell to `<body>` and j / k stopped).
- **From the list into the reader.** New action `conversation.open-in-reader`, bound to `Enter` and `o`
  in `keys context="list"`: it focuses the selected card's header (else the newest open card, else the
  message shown alone). Before, a keyboard user needed 12 Tab stops to reach the first card.
- **Names: first names, then fewer names.** `names(n)` writes first names when it names several people
  (full names when two would share one; the address's local part without a name; the full name is the
  tooltip). When a name would still be cut short, the row leaves out names before the newest (the newest
  and "me" stay; `fewerNames`, measured per row and again when the line's width changes). At 1280 px
  and 1024 px rows read "Linus, Marie … Ada" / "Linus … Ada" instead of "Linu…, Ma… … Ad…".
- **Design B child rows** use body-size text and padding of five sixths of `space.row`: about 40 px in
  Comfortable and 32 px in Compact, as the proposal's density table asks (they were 27 px at 12.5 px).
- **The account chip only when the list mixes accounts.** The tag line names a row's account only when
  the loaded rows come from more than one account (the unified inbox, a search across accounts); inside
  one account's folder every row repeated it, a fourth line on every conversation row.
- **Send-as identities are not "me".** An identity with `send-as=` (ADR 0094) names a shared or
  delegate mailbox others write from too; only its `sender=` address counts as the user's. Colleagues'
  mail from such a mailbox is no longer `from_me`, so the row's actions act on it, not on an older
  message.
- **Enter and Space stay a control's own.** With `enter` bound in the list context, the key dispatcher
  (which falls back to the last context when the focus has none) swallowed Enter on buttons such as a
  notice's Dismiss. Bindings now never take Enter / Space on a button, link, menu item, tab or toggle
  (`isActivation`).
- **`threads="expanded"` keeps its pre-P2 meaning**, one row per message (like `off`); P2 had read it as a
  spelling of `expandable`, which silently turned such user layouts into the Design B tree. This
  corrects ADR 0148's note.
- **Remote narrowing before collapsing copies.** Without a folder, a narrowed remote session's
  `GetConversation` now drops the copies outside the grant before copies with the same Message-ID
  collapse (`Store::conversation_within`), so a hidden copy neither stands for a visible one nor shows
  in `duplicates`.
- **The conversation list is measured.** nail-bench measures `thread-page-p95-ms` (`ListThreads`, the
  default list since P2) on the three largest folders, with a budget of 50 ms in `e2e/perf/budgets.json`.
  Standard corpus (100,000 messages, an Inbox of 29,830 messages / 17,829 conversations, Sent 12,154):
  33 ms p95, against 3.3 ms for a `ListMessages` page. The page query alone is about 14 ms (the rest is
  the 100 rows' conversation reads); rewriting it (grouping per arm, a correlated Sent lookup) gained
  nothing measurable.

## Deferred

- A per-folder thread-activity summary kept in step with writes (or keyset paging), so a conversation
  page stops grouping the whole folder: worth it when `thread-page-p95-ms` approaches its budget or a
  single folder grows well beyond the standard corpus's 30,000 messages.
- Card frames of short HTML mail keep their minimum height; conversation-wide actions; `GetThread` with
  `single-<id>`.
