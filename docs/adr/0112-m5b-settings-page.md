# ADR 0112: M5b WP163 — the settings page: navigation, cards, search, deep links

Status: accepted · 2026-10-03 · M5b WP163 (package ADR; the integrator may renumber)

## Context

Plan section 10 (WP163) and the approved UX proposal replace the fourteen
settings tabs by one page: grouped navigation (General, Look & feel,
Security, Extensions), a settings search, card-based rows (label and
description left, control right), and every former tab moved into a
section without losing a setting. The contract (ADR 0109) gives the
structure — `ResolvedLayout.settings` from the `settings-page` section of
`layout.kdl`, the `settings-page` widget, `Feature.settings` labels and
`layout/settingsIndex.ts` — and the slot `ui/src/features/settings/` plus
the `settings` export of every feature folder except `pim/`. M5a left two
points in this area: the Density segmented control stretched across its
card and showed its selection only in bold in High contrast, and a user
theme's gallery card showed its base palette.

## Decisions

- **Page structure.** `SettingsPage` renders the navigation (a search box,
  then one `role="group"` per settings group labelled by the group's name,
  sections as buttons with `aria-current="page"`) and the shown section: a
  breadcrumb (the group), the section title (`h2`, focus target) and the
  section's widgets. The content is a plain container, not a region: the
  widgets inside are often regions with the same name (axe
  `landmark-unique`). Group labels are not headings, so headings stay the
  section's and its widgets'.
- **Cards.** A section with several nodes is a `stack` (ADR 0109); each of
  its children sits on a `settings-page.card` (a single node: one card),
  carrying `data-widget` (kind) and `data-card-for` (node id) as anchors for
  deep links. Hidden nodes (`view.toggle`) are left out like in any stack.
  The row look is pure CSS in `skin.ts`: inside a card, `form.field` is a
  two-column grid (label and hint left, `form.input` / `form.select` right;
  text areas and anything else full width), rows are separated by
  hairlines, and the widgets' own `card` / `form.section` lose their border
  so cards never nest. No widget component changed — they keep their parts,
  and a theme can still restyle every piece.
- **Shown section** = the core's UI state `settings.section` (read with
  `useUiStateValue`, written with `useSetUiState`, optimistic), so the page
  reopens where it was left and a deep link only has to write that key; an
  unknown or missing value falls back to the first section.
- **Search.** The index is `buildSettingsIndex(layout, appRegistry.settings,
…)` (contract). The contract's `searchSettings` keeps page order; the page
  ranks the hits (`rankSettingsHits`): a section whose label holds every
  word, then one with a setting label holding every word, then by the best
  kind of entry that matched, page order among equals — otherwise
  "plug-ins" would open Desktop app (its developer setting) before Plug-ins.
  While searching, the results replace the groups: each hit's section with
  its icon, and up to four matching setting labels; matches are wrapped in
  `<mark data-part="settings-page.mark">` (`highlightText` folds like the
  index — case and diacritics — and maps back onto the original
  characters). A polite status line says how many sections match. Enter
  opens the best hit, Down moves into the results, Escape clears a
  non-empty search (and otherwise lets the window close).
- **Deep links and reveal.** Choosing a setting result (or a
  `settings.section` link naming a widget) stores a reveal request
  (`store.ts`, per UI store). The page resolves it once the section shows
  it — widgets render their rows after their data arrived, so it watches the
  body with a `MutationObserver` for up to 3 s, then falls back to the
  widget's card, else the title: the deepest element whose text equals the
  label is scrolled into view, marked with `data-settings-found` for 2.5 s
  (styled by the skin), and focus goes to its control (`label[for]`, else
  the first control of its row, else the card).
- **`settings.section "<id>"`** resolves, in order: a section id; a node id
  on the page or `page-<id>`; the id of a former settings tab
  (`formerTabs.json`: every node of M5a's settings window → the widget kind
  holding its settings); a widget kind. It writes the UI state (cache at
  once, then `SetUiState`), stores a reveal request when a widget was named,
  and shows the page through the shell's `showView("settings")` (view host,
  or the view's window without one; without a `settings` view, the
  `settings` window). `settings.search` focuses the search box the same way.
  The former tab ids live in JSON because the catalog coverage test treats
  string literals with message prefixes (`sync-`, `settings-`,
  `customize-`) as message ids.
- **No setting lost** is a test: every former tab id resolves to a section
  that shows its widget kind; while the settings window still has tabs,
  every node in it is in the checklist; every widget kind of the old tabs is
  on the page; every settings widget on the default page has registered
  labels.
- **Registrations.** Each feature's `index.ts` exports `settings`
  (`NonNullable<Feature["settings"]>`) with the Fluent ids of the labels its
  settings widgets show — the former tab label, the widget's title, its
  field and switch labels — and a `settings-keywords-<kind>` id from
  `settings.ftl` with words users search for that are not on the page
  ("spam", "out of office", "pgp", "dark mode"). Labels with placeholders
  are not used (they would render `{$min}`); a test checks every id exists in
  English and has none. The built-in widgets' labels (`account-settings`,
  `virtual-account-editor`, `toolbar`) are in `features/settings/builtins.ts`
  because `ui/src/widgets` belongs to WP162 in M5b.
- **M5a points.** The segmented control inside `appearance` and the page
  keeps its natural width (`justify-self: start; width: max-content`), a
  selected segment there gets a hairline besides its shadow, and in High contrast
  (`:root[data-nail-mode="high-contrast"]`) the selected segment is filled
  with the accent, in `accent-fg`, with an outline; under `forced-colors`
  it uses `Highlight` / `HighlightText`. These rules live in
  `SETTINGS_SKIN_CSS` because `theme/skin.ts` belongs to WP162; WP165 may
  move the generic ones there. The gallery colours of user themes need the
  core to send preview tokens (a contract change, requested from WP165; see
  Consequences).
- **Look.** Tokens only (tested): navigation on `bg-subtle` with selection
  colours for the current section, cards on `bg` with `radius-lg`, titles in
  the display face; one column below 720 px.

## Consequences

- The page works with any `settings-page` section a user writes; sections,
  order, names, icons and keywords stay in `layout.kdl`.
- WP165 swaps the settings window's tabs for `settings-page`; the former tab
  ids keep working as deep links after that, and the checklist test keeps
  checking them.
- Open: `ThemeInfo` preview tokens for user themes (`appearance/` and the
  core), a "Calendar & contacts" section in the default page (WP164 registers
  its widget's labels), per-widget duplicate titles (a widget's own heading
  repeats the section title, e.g. Appearance) — the skin could hide a
  widget's first heading on the page once every widget marks it.
