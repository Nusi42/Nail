# 0220 — Live search and the overlaid query mistake

Status: accepted (2026-11-20)

## Context

ADR 0219 set the contract for wave R1: the search field searches while the
user types (`LIVE_SEARCH_PAUSE_MS`, `useDebouncedAction`), Enter at once, an
invalid query is not searched, and the query mistake no longer pushes the
content under the field down. This records how it was built.

## Decision

- **One search form.** Every mail query field is `SearchForm`
  (`ui/src/widgets/SearchBar.tsx`): the `search-bar` widget and the search of
  the list header (`list-header`). Saved searches (smart folders) save the
  active search and have no query field of their own; the other search boxes
  (settings, contacts, toolbar editor, …) filter local lists and do not use
  the query language, so they are left alone.
- **Committed searches.** A text is committed when the typing pause settles
  or on Enter (`useLiveSearch`, `ui/src/features/search/liveSearch.ts`); both
  take the same path (`planLiveSearch`): the trimmed text equal to the active
  search does nothing (no request), an emptied field ends the search, any
  other text is checked first. Nail has no query history today; when one is
  added it records committed searches only, never the intermediate texts.
- **Checked before searching.** A committed text is checked with
  `CheckQuery` (one cached definition shared with the live check,
  `checkQuery` / `useCheckQuery` in `ui/src/state/queries.ts`). An invalid
  text is not searched: the store's search stays, so the list keeps the last
  valid results, and the field shows the mistake. A core without
  `CheckQuery` (or a failed check) leaves the verdict to the search itself,
  whose `InvalidQuery` answer the list reports as before.
- **Newest wins.** A check answering after the field's text moved on is
  dropped. Search results are keyed by their text, so a late answer for an
  older text never reaches the screen; while the next text loads, the list
  keeps the previous results of the same scope (`keepsSearchResults`, TanStack
  `placeholderData`) instead of flashing "Loading".
- **The mistake overlays.** The field is marked `aria-invalid` and
  `data-state="invalid"` and shows the shared compact problem indicator
  (`ProblemIndicator` in follow mode, ADR 0221: a `circle-alert` icon). Its
  details hold the explanation (`search-bar.error`); they are
  `role="tooltip"`, the field's `aria-describedby`, always in the DOM, and
  open while the pointer is over the search form or focus is in it. They
  float absolutely under the search form (the skin's `problem.details` rules
  in `ui/src/theme/skin.ts`), so they never move the list. They take the
  pointer (review R1, WCAG 1.4.13: content shown on hover stays while the
  pointer is on it), with a strip over the gap to the field; the query line
  wraps, so a mistake late in a long query stays visible. While the field has
  the focus, a visually hidden `role="status"` region (`search-bar.status`)
  announces the mistake and its position, since a changed description of a
  focused field is not read again. Only the search field the user last
  focused commits typed text (`searchOwnerFor`), so two fields of a layout
  never race with their own scopes. The explanation
  also says whether the list still shows an earlier search
  (`search-live-kept`) or nothing was searched (`search-live-not-run`).

## Consequences

- Enter on an invalid query no longer empties the list with "The search could
  not be run"; that message remains for queries the core alone rejects.
- The field hints stay in the flow as before (an overlaid hint list would
  catch clicks meant for the rows under it while the field has focus); a
  later redesign may turn them into a proper combo-box popup.
- Themes that styled `search-bar.error` as a block keep working; its overlay
  geometry comes from the skin, which a theme can still override.
