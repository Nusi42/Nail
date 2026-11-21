# 0223 — R1 leftovers: dynamic submenus, literal labels, search hints popup, folder change

Status: accepted (2026-11-21)

## Context

ADR 0222 (the R2 contract) left five R1 leftovers to one package: "Run
rule" replaced the open context menu in place instead of opening a
submenu; rule names starting with `@` were read as Fluent ids in that
menu; the search field's hints pushed the content under the field down
(an overlay tried in R1 caught clicks meant for the list, ADR 0220);
picking another folder kept the typed search text without searching it;
and the UI mock answered `RunRules` with `running` runs where the core
answers finished ones.

## Decision

### Dynamic submenus (`ui/src/features/menus/`)

- A layout menu item whose action has a menu source
  (`Feature.menuSources`, merged as `AppRegistry.menuSources`) shows as a
  submenu. The context menu loads the sources of a list when the list
  appears (`MenuSourceLoader`: `has` / `load`), so an empty source shows
  its item disabled (`aria-disabled`) before it is hovered, and a source
  that fails counts as empty (logged). While a source loads, its item is
  `aria-busy`; opening it waits for the entries.
- Every submenu (layout or source) now opens when the pointer rests on its
  item, keeping the focus on the item; ArrowRight / Enter / Space open it
  with the focus on its first entry, ArrowLeft / Escape close it. The
  popup stays inside the window: it flips to the other side when it would
  overflow the inline end and moves up when it would overflow the bottom
  (`submenuPlace`, geometry through `useGeometry`), and is as wide as the
  top menu's rules allow (the skin sizes `context-menu.popup` like
  `context-menu`).
- Sources load with the actions' context while the menu is open, so they
  read its target (`contextTarget`); their entries are filtered like the
  layout's (`withoutDisabled`).
- **How the menu reaches the sources:** the app shell provides
  `appRegistry.menuSources` as `MenuSourcesContext` around the feature
  effects. The context menu cannot import the registry: the registry
  imports every feature, the widgets import `features/menus`, and a static
  import back would evaluate the registry before its inputs exist (tests
  that import a widget first failed with a TDZ error). Rejected: loading
  the host lazily like the palette (every app test would suspend once and
  log an `act` warning, which fails the suite).

### "Run rule" on a source (`ui/src/features/rules/runMenu.ts`)

- `rules.run-menu` keeps its `enabled` check (the menu's target can run
  rules) and runs nothing; its entries come from `runMenuSource`
  (registered as the rules feature's `menuSources`): All rules that are
  on, the rules that are on, under Off the rules that are off, or "No
  rules yet" opening the rules window. A failed `ListRules` shows the
  rules toast and an empty (disabled) submenu.

### Labels that are user data

- `literalMenuLabel(text)` marks a label with a leading backslash
  (`LITERAL_LABEL_MARK`) when it starts with `@` or with the mark itself;
  the menu shows a marked label as written (`menuLabelText`), everything
  else resolves as before (`@a.b` is a Fluent id). Rule names in the Run
  rule submenu go through it, so a rule named "@home" shows as "@home".
  Consequence: a layout label starting with a backslash loses it in menus
  (no layout label does; labels are Fluent ids or plain words).

### Search hints as a combobox popup (`ui/src/widgets/SearchBar.tsx`)

- The field is a WAI-ARIA combobox (`role="combobox"`,
  `aria-autocomplete="list"`, `aria-expanded`, `aria-controls`,
  `aria-activedescendant`) whenever hints are on; with `hints=#false` it
  stays a plain search box.
- The hints are a listbox (`search-bar.hints.list`) in a popup
  (`search-bar.hints`) that the skin places absolutely under the field,
  like the problem details (ADR 0220), so the list never moves.
- They open while a word is typed that a hint completes (`hintsWhileTyping`;
  after a space nothing pops up) and with ArrowDown (Alt+ArrowDown opens
  without picking). Up / Down pick a hint (`nextHint`, wrapping), Enter or
  a click takes it; Escape closes only the popup (a second Escape clears
  the search as before), as do Tab, a pointer press outside the form and
  focus leaving the form. The problem details stay closed while the popup
  is open.
- **Why clicks reach the list now:** the R1 overlay opened on focus, so a
  focused field covered the top rows until it lost the focus. The popup
  never opens on focus alone, closes on any press outside the form and
  shows only while a hint fits what is typed.
- The hints are no longer in the tab order (the combobox pattern): the
  M5b "Tab into the search hints" is replaced by the arrow keys.

### A folder change searches the typed text (`ui/src/features/search/liveSearch.ts`)

- ADR 0220 makes the field search what is typed; a folder change cleared
  the search but kept the text, so the field and the list disagreed. Two
  ways were possible: clear the text, or search it again. Chosen: **search
  it in the folder just picked** (checked first, like any committed text;
  an invalid or empty text searches nothing), because the user picked a
  folder to look at and kept a query to apply. While that search is the
  active one, editing the text keeps searching that folder
  (`commitScope`); once the search ends (an emptied field, Clear), the
  field searches its own scope (`scope=` of the widget) again. Searches
  another part sets together with a folder (Gmail labels, Exchange views)
  are left alone: their text equals the typed text, so nothing is
  committed.
- Rejected: re-running in the field's own scope. The default list header
  searches the whole account, so picking a folder while searching would
  have changed nothing visible.

### Mock `RunRules`

- The mock answers `RunRule` / `RunRules` with finished runs like the
  core: `state: "done"`, `finished_at` set, `undoable` when the run queued
  reversible writes; `RuleRunFinished` follows on a microtask. The run
  toast now offers Undo in mock-backed tests and in the web demo as well.

## Consequences

- New sources (saved searches, recent folders, …) are a `menuSources`
  entry and a layout item; the menu code does not change.
- Tests and e2e scenarios find the mail search as `combobox` "Search mail"
  (other search boxes stay `searchbox`).
- Theme parts: new `search-bar.hints.list`; `search-bar.hint.button`
  carries `data-state="active"` for the picked hint; `search-bar.hint` is
  the option (`role="option"`). The skin styles the popup's place, the
  picked hint and the submenu width; themes may override them.
- The folder pane marks a folder as current only without an active search
  (`FolderTree`): while the typed text searches a picked folder, no folder
  row is marked. A follow-up can mark the search's folder.
