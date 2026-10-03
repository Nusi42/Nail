# ADR 0111: M5b WP162 — the mail view: ranked action bar with "More", grouped folder pane, list header, reader fixes

Status: accepted · 2026-10-03 · M5b WP162 (the integrator renumbers)

## Context

ADR 0109 gave the layout model for toolbar overflow (`rank`, `display`,
`more { … }`, `hide-disabled`, `overflow`), menu headings, the folder-pane
sections, the `list-header` widget and the list filters. WP162 builds the
mail view on it as the approved proposal shows it
(a published page, not linked here "Mail, restructured"),
and fixes the M5a deferrals in the mail widgets. Only `ui/src/widgets/`,
`ui/src/theme/skin.ts` and `locales/*/app.ftl` were edited; everything else
is a request to WP165.

## Decisions

### Toolbars (`widgets/Toolbar.tsx`, `widgets/toolbarModel.ts`)

- **Display:** an action item shows icon and label unless `display` says
  `icon` (the label becomes the accessible name) or `label`. The tooltip is
  "Label (Key)" when a key is bound (`toolbar-button-tooltip`; key text from
  the context-menu key labels, `aria-keyshortcuts` for single chords); the
  binding is looked up in the toolbar's key context, then `global`.
- **Primary action:** the button with `rank=1` gets the soft-accent look
  (`[data-rank="1"]`), so "Reply" is primary in the default bar without a
  new option.
- **Fitting:** widths are measured, never estimated. When the shown items or
  the bar's width change (ResizeObserver), every item and the "More" button
  render once and are measured in a layout effect (before paint), then
  `fitToolbar` (ADR 0109, shared) decides. `overflow="more"` (default)
  bars never wrap (`flex-wrap: nowrap`); separators left at an edge vanish
  (`normalizeSeparators`).
- **"More":** a `toolbar.more` button opening the shared `ContextMenu`
  under it (keyboard: Enter / Space / Down open it with the first entry
  focused; Escape returns focus). Entries = the `more` item's own entries,
  then a separator and the actions that did not fit, in bar order (resolved
  like context menus: headings only above actions, shortcuts shown). A bar
  without a `more` item gets an automatic one only while something
  overflows.
- **hide-disabled** filters the bar and the menu (`shownToolbarItems`); the
  toolbar re-renders when the selection or the list's order changes, so
  checks that read loaded summaries (rules, labels) follow.
- **overflow="wrap"** keeps wrapping; after layout, a separator that starts a
  line is removed (`data-wrap-edge="start"` → `display: none`, never changes
  the lines above) and one ending a line is hidden but keeps its width
  (`"end"` → `visibility: hidden`, so nothing moves up) — the M5a "wrapped
  reader bar starts with a separator" deferral.
- The proposal's reader bar and the matching right-click menus are in
  `widgets/proposalToolbars.ts` (tests render them; WP165 writes them into
  `defaults/layout.kdl`).

### Folder pane (`widgets/FolderTree.tsx`, `widgets/folderPaneState.ts`)

- **Opt-in:** the grouped pane renders when the node sets any of `sections`,
  `favorites`, `collapsible`, `account-colors`; without them `folder-tree`
  stays the pre-M5b tree of the active account, so older user layouts (and
  today's tests / e2e that expect one account's folders) keep working. The
  default layout opts in (WP165).
- **New message** on top (`folder-tree.compose`, runs `mail.compose`); an
  option `compose=#false` turns it off (requested for nail-config's table).
- **Favorites** (`folder-tree.favorite`): All inboxes / Drafts / Sent show
  the top-level folder of that role of "All accounts" (selects account
  `all`); Unread / Flagged run `is:unread` / `is:flagged` over every account
  (also put into the search field); Snoozed and Outbox run `snooze.list` /
  `outbox.open`. Counts: unread for the inboxes, all drafts for Drafts.
- **Accounts:** one section per account (real, then the user's virtual
  ones; "All accounts" is Favorites), heading "Name · address", colour dot
  = `account_color_slot(index)` on the avatar palette (`data-color` =
  slot − 1). The message list's account tag now uses the same slot (it used
  a hash of the id), so a row's dot matches its section. Folder rows keep the
  part `folder-tree.item` (context menus, themes, e2e).
- **Smart folders** (`folder-tree.smart-folder`) only when a saved search
  exists; choosing one runs it like the `smart-folders` widget.
- **Collapsing:** clicking a heading, Left / Right, Enter / Space, or
  `folders.toggle-section "<id>"`; kept in the UI state
  (`folder-pane.collapsed`; the cache changes at once, `SetUiState` after).
  A collapsed account heading shows its inbox's unread count.
- **Keyboard:** one ARIA tree, sections are expandable tree items labelled
  by their heading; roving tab stop (the last focused item, else the
  selected folder, else the first section; a collapsed section takes its
  rows' stop; while that item is not rendered the first item stands in).
  Arrows move focus only (choosing is Enter / Space / click), unlike the
  pre-M5b tree where arrows select.

### List header (`widgets/ListHeader.tsx`, `widgets/listFilter.ts`)

- The search is the `search-bar` form (`SearchForm`, shared) with the key
  that focuses it shown inside (`search-bar.shortcut`): the last binding of
  `search.focus`, a chord with a modifier preferred over a single key.
  Today's default layout binds only "/" to `search.focus` (Ctrl+K is the
  palette), so the hint reads "/" until WP165 decides.
- Title: "Search results" while searching; a unified folder of "All
  accounts" is named like its favorite ("All inboxes"), else the folder's
  name. Counts: "N unread", plus "· N accounts" for "All accounts".
- Chips All / Unread / Flagged / Attachments (`chip` with `aria-pressed`)
  set `UiState.listFilter` (also `list.filter "<filter>"`). A chip is a
  search: its term within the shown folder, or `(<typed query>) <term>`
  while searching (parentheses keep an `OR` inside). The list marks it with
  `data-filter`, shows "No messages match this filter." when empty, and
  does not report the combined query's error positions to the search field.

### Reader and list

- **HTML body:** the iframe is `sandbox=""` (opaque origin), so its content
  height cannot be read, and `allow-same-origin` is not an option (tests and
  the security model require `sandbox=""`). Instead the reader scrolls as a
  whole (`message-view` `overflow-y: auto`), the column grows with its
  content (`flex: 1 0 auto`) and the body fills the rest of the pane but is
  at least `max(16 × space.lg, 50vh)` high — never the 150px default, and
  the status bar stays in view (the stack fix of ADR 0108 stays).
- **Banners** keep the column's width formula; `scrollbar-gutter: stable
  both-edges` on the reader keeps the column centred like the banners above
  it when a scrollbar appears.
- **Attachment status:** the `role="status"` element is always there (empty
  while idle, `data-state="empty"` hides it) so "Saving…" and the outcome
  are announced; it shows an icon (check / alert) and colours success /
  failure; it stays in the text column next to the size (ADR 0108).
- **Focus ring on rows:** while the list has keyboard focus, the active row
  gets the ring instead of the whole list (`:focus-visible:has(…)`).
- **High contrast** (`:root[data-nail-mode="high-contrast"]`, any theme in
  that mode): inside selected favorites / smart folders everything uses
  `selection-fg`, hover is the soft accent, focus rings 3px, the More button
  gets the thin border.

### Dates in the UI locale (regression)

Widgets format dates only through `i18n/format.ts` with `useLocale()`; a new
test renders list rows (relative and short dates) and the reader's date with
a German browser and an English UI and expects English. The maintainer's setup —
Windows / browser German, no language chosen — still shows German dates
with English texts: `createLocalizer` negotiates the browser languages
against every catalog directory, and `locales/de/` exists (empty), so the
locale is `de` while every text falls back to English. The fix belongs in
`ui/src/i18n/` (not a WP162 file) and is requested from WP165: negotiate the
browser's languages only against locales whose catalogs define messages (an
explicit override stays honoured, which the existing tests require).

## Consequences

- WP165 must opt the default layout in (`folder-tree` options, `list-header`
  above the list, the reader bar with `rank` / `display` / `more` and
  `hide-disabled=#true`, the matching context menus), drop the separate
  `account-switcher` / `search-bar` / `smart-folders` widgets from the mail
  window, regenerate the visual baselines, and update e2e locators that
  assume one account's folder tree.
- Toolbars measure in the browser; jsdom tests stub widths
  (`getBoundingClientRect`, `clientWidth`) to exercise overflow.
