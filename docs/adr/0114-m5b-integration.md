# ADR 0114: M5b integration — the rail in the default layout, migration of user layouts, contract follow-ups

Status: accepted · 2026-10-03 · M5b WP165

## Context

WP161–WP164 built the shell (rail, view host, PIM views; ADR 0110), the mail
view (grouped folder pane, list header, ranked action bar with "More"; ADR
0111), the settings page (ADR 0112) and the calendar redesign (ADR 0113)
against the contract (ADR 0109), each in its own slot. The default
`layout.kdl` still had the M5a windows, so nothing of it showed in the app.
WP165 puts it together: the default layout, the contract changes the packages
asked for, migration of existing user layouts, the scenarios and the docs.

## Decisions

### The default layout

- `window "main"` = `stack` { `config-errors`, `split "horizontal"
  id="shell" sizes="auto 1fr" resizable=#false` { `nav-rail`, `view-host` },
  `status-bar`, `mcp-context-indicator` } — one window for every width; the
  status bar and config errors span all views.
- The M5a three-pane content moved to `window "mail"` (both breakpoints) and
  `view "mail" window="mail"`. Its sidebar is the grouped `folder-tree`
  (`sections="favorites accounts smart-folders"`, `favorites="all-inboxes
  flagged snoozed drafts"`, `collapsible`, `account-colors`) followed by the
  Gmail / Exchange widgets; the list pane starts with `list-header`. The
  separate `search-bar`, `account-switcher` and `smart-folders` widgets left
  the default (the list header searches, the account sections replace the
  switcher, smart folders are a pane section); they stay available for
  user layouts.
- **No main toolbar.** The proposal's rail replaced the navigation buttons and
  the folder pane's **New message** the compose button; the remaining actions
  (check for mail, outbox, filter rules, snoozed, add account) are in the
  command palette (Ctrl+K), their keys, the folder pane (Snoozed favorite,
  the account heading's menu) and Settings. `toolbar "main"` is still defined
  (ranked) so user layouts that place it keep it.
- The `confirmation-indicator` left the main window: the rail's
  confirmations badge shows the same count.
- The reader bar and the two message context menus are the proposal's
  (`ui/src/widgets/proposalToolbars.ts` mirrors them): Reply (`rank=1`, the
  primary look), Reply all, Forward labelled; Archive, Delete, Move, Flag,
  Snooze as icons; "More" with the headings Message / Turn into;
  `hide-disabled=#true`.
- The calendar window offers Day, Week, Month, Year, Agenda, Free time (no
  work week in the switch); its toolbar is ranked with "More" (print, import,
  sources, scheduling settings).
- The settings window is `settings-page id="settings-page"` (the tabs are
  gone; the former tab ids stay `settings.section` deep links via
  `features/settings/formerTabs.json`). New section `calendar` ("Calendar &
  contacts", `@settings.section.calendar` in `core-shell.ftl`) with the
  `pim-settings` widget, registered in `nail-config` `widgets/pim.rs`.
- `pim.calendar.open`, `pim.contacts.open`, `pim.tasks.open` and
  `settings.open` became aliases of `view.show "<view>"` (with a view host,
  `window.open` would open a dialog copy of the view's window).
- New context menu `context:folder-tree.account` (the DOM part
  `folder-tree.account` maps to it in `features/menus/targets.ts`).
- The search hint key stays `/`: Ctrl+K is the command palette (the plan's
  "Ctrl+K hint" predates that binding); the list header shows whatever key
  the layout binds to `search.focus`.

### Migration of user layouts

A pre-M5b `layout.kdl` keeps working unchanged: it inherits the views, rail
and settings page (ADR 0109) while its own `window "main"` still shows; view
shortcuts and the palette open the views as windows (the contract's
fallback). Once, the app offers the new default
(`features/customization/LayoutMigration.tsx`, an app effect): when the
resolved main window has views but no `view-host` and the UI state has no
`customize.layout-offer`, a small region "A new layout with a navigation
rail" offers **Review the new layout** (opens the merge dialog prefilled
with the shipped `defaults/layout.kdl` and compared at once; the user takes
over what they want, usually `window "main"`) or **Not now**. Both answers
are remembered in the UI state. Nothing is written until the user applies
the merge (which backs the file up as before). `nail-config`
`tests/m5b_migration.rs` checks both paths: an old file resolves with the
inherited views, and taking only its `main` window from the default yields
the rail while keeping the user's own items.

### Contract follow-ups (package requests)

- `ThemeInfo.preview` (optional map of the theme's own resolved
  `bg-subtle`, `bg`, `accent`, `fg`; `PREVIEW_TOKENS`), filled by
  `ThemeCatalog` for usable themes; the UI's `previewColors` lays it over the
  base palette, so a user theme with its own accent previews with it (the M5a
  theme-gallery point). Editing a user theme now also sends `ThemeChanged`
  (its preview changed).
- `folder-tree compose=` (Bool, default `#true`).
- i18n: the browser's languages are negotiated only against locales whose
  catalogs hold a message (plus English and an explicit override), so an
  empty `locales/de` no longer makes dates German next to English text
  (`availableLocales`, `createLocalizer(…, override)`).
- The High contrast / forced-colours fill of a selected segmented option moved
  from the settings skin to the shared skin and covers `aria-pressed` (the
  calendar's view switch) as well as `aria-checked`.
- `defaults/theme.kdl`: the M3a part rules that `features/pim/skin.ts` now
  styles were removed (they came later in the cascade and overrode it); the
  composer's message text uses the reader face (monospace stays for source
  code); Sign / Encrypt / Technology sit on one centred line.

### Found while integrating

- A split pane sized `auto` now takes its content's width (`0 0 auto`, was
  `1 1 auto`): the rail grew to half the window and squeezed the mail panes
  to their minimum widths (`layout/engine.ts` `flexForSize`). A split without
  `sizes` still shares the space.
- The folder pane's account sections carry the account kind and connection
  state (`data-account-kind`, `data-status`) and name a state other than
  online in their heading ("Disabled", "Offline", "Sign-in required"): the
  default no longer shows the account switcher with its status dots.
- The settings page's search landmark is named ("Search settings"): a
  section's widgets bring search landmarks of their own (axe
  `landmark-unique`). The page's configuration error list is labelled apart
  from the Configuration errors window its toolbar opens.
- The PIM skin resets the browser look of the day-head and task-title
  buttons (grey `ButtonFace` in dark modes failed colour contrast) and keeps
  a contact avatar's text colour in a selected row.
- The view host saves `shell.view` only when its own view changes: it used to
  write its view back whenever the stored value differed, so two sessions on
  different views (the desktop and a remote browser share the UI state)
  would have answered each other forever.
- The view key bindings come before the older `g c` / `g p` / `g t` ones, so
  the rail's tooltips name Ctrl+1 … Ctrl+4.
- Plug-in panels placed by the placement writer go into the mail view's
  window (`nail-config` `plugins.rs` `panel_window`), into `main` only for a
  pre-rail layout.

### Tests

- Playwright runs at 1280 × 1000 so the list under the list header renders
  every demo row; `e2e/support/views.ts` holds the shell helpers; the app
  reopens the last view after reloads, which tests take into account.
- `m5b.spec.ts` covers the rail, shortcuts and focus, the unread badge, the
  remembered view, the reader bar and More (keyboard, narrowing), filter
  chips, settings search and the remembered section, the calendar switch and
  a calendar's menu, dates with a German browser, the migration offer and a
  remote browser's own dark mode; `a11y.spec.ts` scans the views in all five
  themes; the visual harness gained the `more` screen.

### Package ADRs

Renumbered from the parallel stage's `0110-wp16x-*`: 0110 shell, 0111 mail
view, 0112 settings page, 0113 calendar view.

## Consequences

- The app looks like the proposal; e2e suites address views as regions
  ("<View> view"), Settings sections through the page navigation, account
  folders through their pane sections (`e2e/support/views.ts`).
- Users who keep an old layout see the offer once and can apply the new
  default later through **Customize → Apply a theme or layout** with the same
  file.
- Restoring the remembered view only happens with a view host; in the
  fallback no window opens at start (ADR 0110).
