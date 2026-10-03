# ADR 0109: M5b contract — views and the navigation rail, toolbar overflow, folder-pane sections, the settings page, UI state, per-session Follow Windows, slots

Status: accepted · 2026-10-03 · M5b WP160

## Context

Wave M5b (plan section 10) builds the shell of the approved UX proposal
(a published page, not linked here): Mail, Calendar,
Contacts, Tasks and Settings as views behind a navigation rail, a grouped
folder pane, a list header, one ranked reader action bar with "More",
Settings as a searchable page, and a redesigned calendar. Four UI packages
build in parallel against this ADR — WP161 (rail, view switching, PIM views),
WP162 (mail widgets), WP163 (settings page), WP164 (calendar view) — and WP165
integrates (the new default `layout.kdl`, migration, scenarios, docs).

Binding scope decisions (plan section 10): every structure stays expressible
in `layout.kdl` / `theme.kdl`; components emit `data-part`s; icons are
Lucide. M5a left these open points for M5b: toolbar overflow (the High
contrast main toolbar wraps at 1280 px; a wrapped reader bar can start with
a separator), a menu header entry (`MenuItem::Header`, already styled as
`context-menu.header`), and the `MessageSummary` attachment / invitation /
warning fields for the tag line (store migration — not part of this
contract, see Consequences).

All four parallel packages are UI-only, so everything Rust needs —
`nail-api`, `nail-config`, the core and the default layout's new sections —
is done here, and the core features (UI state, per-session Follow Windows)
are served already.

## Decisions

### Views and the rail (layout model, `nail_api::views`)

- `ResolvedLayout.views: IndexMap<id, ViewDef {label, icon, window, badge}>`
  (definition order), `default_view`, `rail: Vec<RailItem>`
  (`view` / `action {action, args, label, icon, badge}` / `spacer` /
  `separator`), `start_view()` = `default_view`, else the first view.
- A view **shows a layout window**: `ViewDef.window` names a `window
  "<name>"` (with its breakpoints). The `view-host` widget renders the active
  view's window tree; `nav-rail` renders the rail. This keeps `LayoutNode`
  unchanged (no new container) and lets views reuse the existing `calendar`,
  `contacts`, `tasks` and `settings` windows.
- KDL: `views default="mail" { view "<id>" label=… icon=… window=… badge=… }`
  and `rail { view "<id>"; item "<action>" icon=… badge=…; spacer;
  separator }`. `badge` is `BadgeSource` `unread` (unread mail in the
  inboxes) or `confirmations` (changes waiting for approval).
- Validation (`nail-config`, `config-view-*` / `config-rail-*` /
  `config-widget-needs-section` in `core.ftl`): a view's window exists and
  contains no `view-host` (no recursion); the default view and the rail's
  views exist; `view-host` needs `views`, `nav-rail` a `rail`,
  `settings-page` a `settings-page` section; view ids are unique; each of
  the three sections at most once per file.
- Merging: a `views`, `rail` or `settings-page` section in a user file
  replaces the base's as a whole; files without them (every pre-M5b file)
  inherit the defaults — the basis of WP165's migration.
- Actions: `view.show "<view>"`, `view.next`, `view.previous`
  (`VIEW_*_ACTION`); the default layout adds aliases `view.mail`,
  `view.calendar`, `view.contacts`, `view.tasks`, `view.settings` (palette
  entries) bound to `ctrl+1` … `ctrl+4` and `g m`. **Fallback:** without a
  `view-host` in the main window, `view.show` opens the view's window (the
  mail view's window `main` closes what covers it), so the shortcuts work
  with any layout — implemented in the shell slot now.
- The active view is `UiState.activeView` (`ui/src/state/uiStore.ts`),
  `null` = the start view; remembered in the core's UI state under
  `shell.view`.
- Pure rules shared by Rust and the UI (tested against the same cases):
  `step_view` / `view_of_window` (`ui/src/layout/views.ts`).

### Toolbar overflow

- `ToolbarItem::Action` gains optional `rank` (overflow order: unranked items
  move first, from the end; then the highest rank; `rank=1` stays longest)
  and `display` (`ItemDisplay` `icon` / `label` / `icon-label`; absent = the
  renderer's default). Both are `skip_serializing_if = None` and `ts(optional)`
  so pre-M5b JSON and hand-written items stay valid.
- `ToolbarItem::More {label, icon, items: Vec<MenuItem>}`: the "More" menu
  with its own entries; overflowing items are appended. At most one per
  toolbar (`config-toolbar-more-twice`); an overflowing toolbar without one
  gets one.
- `ResolvedLayout.toolbar_settings: IndexMap<name, ToolbarSettings
  {hide_disabled, overflow: more | wrap}>`, one entry per toolbar (defaults
  when the node sets nothing): `toolbar "<name>" hide-disabled=#true
  overflow="wrap"`. Kept in a separate map so `toolbars` keeps its shape.
  Default `overflow` is `more` (wrapping was the bug); `hide_disabled`
  defaults to `false` until WP165 sets it for the reader bar.
- `MenuItem::Header {label}` (`header "<label>"`), also in context menus and
  `ResolvedMenuEntry::Header`; a heading is kept only when an action or
  submenu follows it before the next heading or separator, and a menu of
  headings only resolves to nothing (`nail_api::menus` and
  `features/menus/resolve.ts`).
- The fitting rules are pure and shared by every toolbar:
  `ui/src/layout/overflow.ts` (`overflowOrder` = `nail_api::views::
  overflow_order`, `fitToolbar`, `normalizeSeparators`,
  `shownToolbarItems` for `hide_disabled`).
- The format-preserving write-back renders `rank` / `display` / `more` /
  `header` and keeps a toolbar node's `hide-disabled` / `overflow` when it
  re-renders the node (`write_back::toolbar_settings_of`).

### Folder-pane sections

- Sections: Favorites (the unified folders, `FavoriteFolder`
  `all-inboxes unread flagged snoozed drafts sent outbox`, default
  `all-inboxes flagged snoozed drafts`), one per account (in account order,
  colour slot `account_color_slot(index)` 1–8 = the avatar palette
  `color.avatar-<n>`, so High contrast and every theme cover it without new
  tokens), smart folders only when there is one
  (`nail_api::views::folder_pane_sections`, mirrored by
  `ui/src/widgets/folderPane.ts`). Section ids: `favorites`,
  `account:<id>`, `smart-folders`.
- `folder-tree` options: `sections` (word list `favorites accounts
  smart-folders`), `favorites` (word list), `collapsible`, `account-colors`.
  `nail-config` gained `OptionType::Words` (space-separated, known words,
  none twice; `config-option-words`).
- A colour **choice** per account (`accounts.kdl color=`) is not part of
  M5b: account colours follow the account order. Collapsed sections live in
  the UI state (`folder-pane.collapsed`, an array of section ids).

### List header and filters

`list-header` widget (`search`, `hint`, `title`, `counts`, `chips` word list);
`ListFilter` `all` / `unread` / `flagged` / `attachments` with
`search_term()` (`is:unread`, `is:flagged`, `has:attachment` — the existing
search language), `UiState.listFilter`, action `list.filter "<filter>"`
(WP162).

### The settings page

- `ResolvedLayout.settings: Vec<SettingsGroup {id, label, sections:
  Vec<SettingsSection {id, label, icon, keywords, root: LayoutNode}>}>` from
  `settings-page { group "<id>" label=… { section "<id>" label=… icon=…
  keywords=… { <nodes> } } }`; several nodes become a `stack` of `auto` rows.
  Group, section and node ids are unique on the page. Default labels:
  `@settings.group.<id>` / `@settings.section.<id>`.
- The default layout already has the page (groups General, Look & feel,
  Security, Extensions; 14 sections with Lucide icons) holding **every**
  widget of today's settings tabs (`tests/layout.rs`
  `the_default_settings_page_keeps_every_settings_tab`); node ids are
  `page-*` while the tabs window still exists. WP165 swaps the settings
  window's tabs for `settings-page`.
- Search index: structure from the layout, contents from the features —
  `Feature.settings: Record<widget kind, SettingsWidgetInfo {labels: Fluent
  ids of every setting label, keywords?}>`, merged as
  `appRegistry.settings` (duplicates refused); `ui/src/layout/
  settingsIndex.ts` builds the index (section, group, keywords, settings) and
  searches it (every query word in some entry of a section, case and
  diacritics folded). Built-in widgets' labels (`account-settings`,
  `virtual-account-editor`, the customize editors) are registered in
  `features/settings/` (WP163).
- `settings.section "<id>"` opens the settings page at a section (contract
  stage: remembers it in the UI state and opens the settings window);
  `settings.search` focuses the search (WP163).

### Icons for every action

Every registered action has a bundled Lucide icon, from the default
layout's `actions` or from `ActionInfo.icon` (new; for actions a package
adds without touching `defaults/layout.kdl`). The five registered actions
without one got entries in the defaults (`palette.clear-recent`,
`plugins.run-command`, `security.recovery-key.*`); tested by
`features/registry.test.tsx` ("action icons"). M5a's review had already
given all 99 default actions an icon (ADR 0108). Bundled icons gained
`user-round`, `shield`, `list-todo`, `server`, `users-round`, `database`.

### UI state (`nail_api::ui_state`)

Small state kept across restarts — collapsed sections, the last view, the
last settings section, and in M5c window geometry — lives in
`<config-dir>/ui-state.json` (JSON object; not configuration: not watched,
not backed up; `layout.kdl` keeps describing structure). `GetUiState` →
`Response::UiState {state}`; `SetUiState {key, value}` (`null` removes;
dotted kebab-case keys ≤ 64 chars, values ≤ 16 KiB, ≤ 512 keys;
`UiStateIssue` → `invalid_request` with `ui-state-issue-*`), atomic writes
under the config manager's write lock; `Event::UiStateChanged {key, value}`
on a change. Remote sessions: `session` access (no mail data; shared with the
desktop). UI: `ui/src/state/uiState.ts` (`useUiState`, `useUiStateValue`,
`useSetUiState`, `applyUiStateChange`; `App.tsx` applies the event), mock
`ui/src/api/uiStateMock.ts`. Keys: `UI_STATE_COLLAPSED_SECTIONS`,
`UI_STATE_VIEW`, `UI_STATE_SETTINGS_SECTION`.

### Per-session Follow Windows (remote browsers)

A remote session follows **its own browser's** light / dark mode instead of
the host OS:

- Its `ReportSystemAppearance` is stored per session
  (`Inner.session_appearance`, at most 1,024 sessions; beyond that the
  remembered states are dropped and sessions fall back to the host until they
  report again) and never changes the host's selection.
- `ConfigManager::appearance_for(system)` resolves the appearance and theme
  for a given OS state without changing anything.
- For a remote session, `GetConfig`, `ListThemes` / `SetTheme` /
  `SetThemePreferences` answers and the `ThemeChanged` / `ConfigChanged`
  events (`Service::remote_event`) are computed for the session: active
  theme from the session's report, else the host's state;
  `Appearance.system` = the session's report (`None` until it reported, so
  its UI reports at start).
- The UI's reporter applies the `Themes` answer (`applyAppearance`: refetch
  the config when the active theme changed), since the host's events are not
  the session's. The desktop's own UI is unchanged (the OS watch wins).

### Strings

`locales/en/core-shell.ftl` (contract ids: view labels, view actions,
toolbar `More` and group headings, account colour, settings groups and the
default sections, list filters, favorites, folder sections, UI-state issues;
checked by `nail-api/tests/catalog.rs`); new UI files `shell.ftl` (WP161),
`settings.ftl` (WP163); `app.ftl` (WP162) and `pim.ftl` (WP164) exist; German
files exist and are empty. `menus.ftl` gained
`context-menu-label-calendar-list-calendar` (new context-menu part
`calendar-list.calendar`, key context `calendar`).

### Widget and option slots

New widget kinds (`nail-config/src/widgets/shell.rs`): `nav-rail`
(`show-labels`, `show-logo`), `view-host`, `settings-page` (`search`),
`list-header`. New options: `folder-tree` (above), `calendar view-switch`
(`segmented` / `menu`), `calendar-list actions` (`menu` / `buttons`) and
`show-sources`. The UI registers placeholder components (render nothing)
so layouts naming them stay valid until the packages replace them.

### Slots (who edits what in M5b)

| Package | Owns |
| --- | --- |
| WP161 | `ui/src/features/shell/` (rail, view host, view switching, badges, Calendar / Contacts / Tasks as full views — rendering the existing PIM widgets, not editing them; styles in `shell/skin.ts` `SHELL_SKIN_CSS`), `locales/*/shell.ftl` |
| WP162 | `ui/src/widgets/` (folder tree with sections, message list, list header, reader, action bar with "More" via `layout/overflow.ts`, attachment cards; registrations in `widgets/feature.ts`) and `ui/src/theme/skin.ts` (the shared skin: mail panes, toolbars, the "More" menu), `locales/*/app.ftl` |
| WP163 | `ui/src/features/settings/` (settings page, navigation, search; styles in `settings/skin.ts` `SETTINGS_SKIN_CSS`) and the `settings` export (`Feature.settings`) in every feature folder except `ui/src/features/pim/`, `locales/*/settings.ftl` |
| WP164 | `ui/src/features/pim/` (calendar view redesign: toolbar on one line, segmented view switch, calendar list with colour squares and "…" menus, "New calendar" in a menu, readable event cards; styles in `pim/skin.ts` `PIM_SKIN_CSS` or `pim/parts.ts`), `locales/*/pim.ftl` |
| WP165 | integration: the default `layout.kdl` with rail and views (main = `nav-rail` + `view-host`, mail window, settings window = `settings-page`, ranked toolbars with `more` and `hide-disabled`), migration of user layouts, `defaults/theme.kdl` part rules if needed, every scenario, docs |

The three package skins are emitted by `ui/src/theme/compile.ts` right after
the shared `SKIN_CSS` (empty ones are skipped). Shared contract files nobody
edits in M5b except through WP165: `ui/src/layout/{views,overflow,
settingsIndex}.ts`, `ui/src/state/{uiStore,uiState}.ts`,
`ui/src/features/{types,registry}.ts`, `ui/src/theme/compile.ts`,
`ui/src/api/*`, `crates/`, `defaults/`, `e2e/` (packages add Vitest tests in
their folders; Playwright scenarios are WP165's). Contract changes go to
WP165 as requests.

## Consequences

- Views reuse windows, so no new layout container and no renderer change in
  the shared engine; the price is that a view's window must not host views
  (checked).
- Users' existing `layout.kdl` files keep working: they inherit the default
  views, rail and page; their own `window "main"` keeps showing (the
  fallback opens views as windows until their main window has a host).
- The default layout carries the views / rail / settings page already while
  its windows are unchanged, so the app looks the same until WP165; the UI
  packages build against real data in the fixture
  (`crates/nail-api/fixtures/default-layout.json`).
- The `MessageSummary` tag-line fields (attachments, invitation, warning)
  stay open: they need a store migration; WP162 shows what the summary
  already has.
- UI state is a file next to the configuration; M5c adds window geometry
  under `window.*` keys instead of inventing another store.
