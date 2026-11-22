# 0236 — R5 WT2: select all in the message list, one question per whole-list action

Status: accepted (2026-11-22)

## Context

ADR 0235 added list targets (`MessageTarget::List`): an action can name a
whole message list except some rows, and `ResolveMailTargets` answers it
with the true count and an `as_of` mark. This package builds the UI on it:
`Ctrl+A` in the message list, the "all except these" selection, and the
actions on it. The ADR left open when an action on a large selection asks
first ("destructive or above a threshold, decide and document").

## Decision

### The selection

- The list-selection store (`ui/src/actions/mailTargets.ts`) holds, besides
  the explicit selection (the selected row and the rows picked with
  Ctrl / Shift + click), `all: { list, except } | null`: the list query the
  message list pages with (`mailListOf`: a folder's conversations or
  messages, or the search including the filter chips' terms; the search
  query comes from the same `searchQueryOf` the list's `Search` uses) and
  the rows left out.
- `list.select-all` (`Ctrl+A`, `Cmd+A` on macOS, the command palette and
  the row's context menu) sets it; `list.clear-selection` (`Esc`), a plain
  click on a row, Shift + arrow keys, another folder, search or filter
  (`setList` compares the queries) end it. Under "select all",
  `Ctrl` + click leaves a row out or takes it back, `Shift` + click does the
  same for the range from the last clicked row (out when the clicked row was
  selected, back when it was left out).
- Every row not left out renders selected, also rows loaded later.
- `currentTargets` returns one list target (`listTargetOf`: the left-out
  rows as the targets they stand for) while `all` is set; a context menu on
  a left-out row acts on that row alone. Every caller of `currentTargets`
  (the built-in mail actions, move, tags, snooze, junk) therefore sends the
  list target without changes of its own.
- "All N selected" (`message-list-all-selected`) sits at the top of the
  list (`message-list.selection-summary`, `aria-live="polite"`): the core's
  count (`ResolveMailTargets` of the list target with `mark-read`, whose
  reach keeps every message of a row: `conversations` for a list of
  conversations, `messages` otherwise; `useListCount`, refetched after
  every action and when the list's total changes), the list's total minus
  the left-out rows until the core answers. The list box keeps
  `aria-multiselectable="true"`.

### One question per whole-list action

- **Every** action on a whole list asks once, whatever its size or kind,
  like Gmail's "Confirm bulk action": after `Ctrl+A` most rows were never
  seen (not loaded), so even a flag change on them should not happen
  unasked, and a threshold would make the same gesture sometimes ask and
  sometimes not. Actions on rows picked one by one keep their R4 behaviour
  (only Delete permanently and moves into Drafts, Trash or Junk ask).
- `runMailAction` does it centrally (`features/mail-actions/bulk.ts`):
  `ResolveMailTargets` with the real action, nothing when it counts no
  message, else the modal confirmation `mail-bulk-confirm` (Cancel
  focused, the button names the action: "Archive all", "Mark all as
  unread", …; the Delete permanently note for that action). Confirming
  applies the same list target with the answer's `as_of`; cancelling, Esc
  or closing the window acts on nothing. Delete permanently and Delete in
  Trash on a whole list go through this question instead of "Delete
  permanently?" (one question, not two).
- One `ApplyMailAction` per action, whatever its size: `applyInChunks` is
  gone (with it the split batches without a single undo). Progress shows
  through the queue summary in the status bar ("Saving N changes").
- `RunOptions.resolution` replaces `apply` / `conversations`: a confirmed
  explicit selection applies exactly the counted messages (as before), a
  confirmed list the list with the mark (`confirmedTargets`).
- The list updates its loaded rows at once (`loadedRowTargets`: the list
  target as its loaded rows minus the exceptions); a removing action ends
  "select all", flags and tags keep it. The toggles look at what is known:
  toggle read asks the folder's unread count (a folder list) or the loaded
  rows; toggle flag the loaded rows; mark read / unread stay offered.

## Scope decisions

- The question's button text is chosen in Fluent from the action's type and,
  for read / flag / junk, which way (`bulkLabelArgs`), so the UI holds no
  second port of `MailAction::kind`.
- The label window (`gmail.apply-labels`, `exchange.apply-categories`)
  shows the tags of the loaded targets only; for a whole list it shows none
  checked (adding works; the counts of a whole list are not fetched).
- Rules "run on selected" expands targets in the UI (`expandTargets`), which
  skips list targets: running rules on a whole list needs `RunRules` to take
  mail targets (a request to the integrator / a later package).

## Consequences

- The default layout gets the list keys (`ctrl+a`, `meta+a`, `escape` →
  `list.select-all` / `list.clear-selection`), the two actions' labels and
  icons, and `list.select-all` in `context:message-list.row`; the theme a
  style for `message-list.selection-summary` (integration).
- Tests: `actions/mailTargets.test.ts` (the model), `features/mail-actions/
  run.test.ts` (one request, the question, `as_of`, cancel),
  `features/mail-actions/selectAll.app.test.tsx` (the app on the mock core),
  `e2e/tests/r5-select-all.spec.ts` (web mode: a list larger than a page,
  a search, Esc, Undo).
