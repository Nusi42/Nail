# ADR 0131: M5d contract — editor patches, theme edits, undo history, change detection, slots

Status: accepted · 2026-10-04 · M5d WP180

## Context

Wave M5d (plan section 10) adds the in-app editors of the approved UX
proposal (a published page, not linked here, sections
"Layout editor" and "Theme editor"): a customize-layout mode (drag panes by
their header to swap or dock them, hide / show them, add widgets from a
palette, presets, Undo / Reset / Done), a toolbar customizer (right-click a
toolbar → Customize; bar, "More" and a palette of actions), and Appearance
settings with a full theme editor (tokens, part overrides, duplicate /
rename / delete). The scope decision of M5: every structure and style stays
expressible in `layout.kdl` / `theme.kdl`; the editors write those files
through the write-back with comments preserved, and offer undo.

Four packages build in parallel against this ADR: WP181 (layout editor),
WP182 (toolbar customizer), WP183 (Appearance and the theme editor), WP184
(the write-back engine in nail-config). WP185 integrates.

Before the contract, the first commit of the wave closed Q1's leftovers
(the visual screen `main-1024`, see plan section 10, Q1 status).

## Decisions

### Layout edits are `LayoutPatch`es (`nail_api::customize`, `nail_api::editing`)

The M1b `UpdateLayout { patches }` request carries the new patch kinds, so
the layout editor, the toolbar customizer and the M1b editors share one
write path, one validation and one undo history:

| Patch | What it writes |
| --- | --- |
| `move-pane {node, to}` | moves a node with its subtree; `to` is a `LayoutPlacement`: `into {parent, index}` or `beside {target, side}` (dock left / right / top / bottom: into the parent split of that orientation, else the target is wrapped in a new split `1fr 1fr`) |
| `swap-panes {a, b}` | swaps two nodes (each takes the other's place and split size / limits) |
| `add-widget {widget, to}` | `NewWidget {kind, id?, label?, options}` from the palette (plug-in panels: kind `plugin-panel`, option `panel`) |
| `remove-node {node}` | removes a node (never the root); empty containers go, a split with one child is replaced by it |
| `set-hidden {node, hidden}` | writes / removes `hidden=#true` on the node |
| `apply-preset {preset, target}` | arranges a breakpoint's panes as the preset says; unnamed panes stay, hidden, in a stack after the preset's tree |
| `reset-window {window}` | restores one window (all breakpoints) to the default |
| `move-toolbar-item {toolbar, from, to}` | `ToolbarSlot {area: bar \| more, index}`; an action becomes a menu item in "More" and back (`label` / `icon` move along) |
| `add-toolbar-item {toolbar, item, to}` | an action (plug-in actions included), separator or spacer (not into "More") |
| `remove-toolbar-item {toolbar, from}` | takes an item off the bar or "More" |

Nodes are addressed by `LayoutNodeRef {window, breakpoint, path}` as
rendered from the current file. Errors are `invalid_request` with an
`edit-issue-*` id (`node-not-found`, `invalid-placement`, `unknown-widget`,
`unknown-preset`, `preset-pane-missing`, `toolbar-slot-invalid`) or the
config's own errors for invalid widget options. `LayoutPatch::kind()` /
`kind_of()` give the undo entry's `EditKind` (several patches of one kind =
that kind, else `layout-edits`).

### Hidden nodes and presets in `layout.kdl`

- Every node takes `hidden=#true|#false` (`LayoutNode.hidden`, omitted from
  JSON when false; `LayoutNode::is_hidden`, `at_path`). The renderer skips
  hidden nodes; the merge dialog and the write-back keep the property.
  Not a widget option.
- A new top-level section `presets { preset "<id>" window="<window>"
  label=… icon=… { <arrangement> } }`; the arrangement is `split`
  (orientation argument, `sizes=`), `stack` (`sizes=`), `tabs` and
  `pane "<node id>"` (`PresetNode`). `ResolvedLayout.presets`. A sparse file's
  `presets` replace the base's as a whole. Checks: one root node, a pane at
  most once, ids kebab, the window exists (`config-preset-*` in
  `core-editors.ftl`). **A preset naming panes a breakpoint lacks is not an
  error**: a user file that replaced the mail window keeps working;
  `ApplyPreset` refuses that breakpoint (`edit-issue-preset-pane-missing`)
  and the editor offers only presets whose panes the breakpoint has.
- The default layout gets four presets for the mail view (Three panes,
  Reader below, Wide reader, Focus — over the panes `sidebar`, `list-pane`,
  `reader`), the actions `layout.customize` (`layout-dashboard`) and
  `toolbar.customize` (`settings-2`) and `menu "context:toolbar"`
  (Customize toolbar). New Lucide icons: `columns-3`, `layout-dashboard`,
  `panel-bottom`, `panel-left`, `pencil`, `rows-2`, `settings-2`, `square`.
- Widget kinds registered for the packages (`nail-config/src/widgets/editors.rs`,
  no options yet): `layout-editor-settings` (WP181), `toolbar-editor-settings`
  (WP182), `theme-editor` (WP183). None is placed in the default layout by
  the contract.

### Theme edits (`nail_api::editing`)

- `EditTheme {target, patches, base_revision?}` → `Themes`; `target` is
  `user` (the user's `theme.kdl`) or `theme {theme_id}` (a user theme in
  `themes/`; built-ins are refused with `edit-issue-not-editable`). Patches:
  `set-token {category, name, value|null, mode?}` (`mode` = `dark`,
  `high-contrast`, `print`, `compact`), `set-part-property {selector,
  property, value|null}` (an empty rule is removed), `reset-part {selector}`.
- `DuplicateTheme {theme_id, name}` → `ThemeSaved {theme_id, appearance}`
  ("Duplicate as new theme" and "New theme": a built-in or user theme copied
  into `themes/<id>.kdl`, id from the name, unique). `RenameTheme {theme_id,
  name, new_id?}` → `ThemeSaved` (with `new_id` the file is renamed and the
  selection follows). `DeleteTheme {theme_id}` → `Themes` (a selection naming
  it falls back to the default theme of its kind, in the same undo entry).
- `GetThemeDetails {theme_id}` → `ThemeDetails {info, editable, revision,
  tokens: ThemeTokenValue[], parts: ThemePartRule[]}` with each value's
  origin (`base`, `extended`, `own`) — what the editor's token groups and
  part list show.
- Errors: `edit-issue-theme-not-found` (`not_found`),
  `edit-issue-invalid-theme-name`, `-theme-exists`, `-invalid-token`,
  `-invalid-part`, `-not-editable` (`invalid_request`).

### Undo history (`nail-config/src/edit_history.rs`)

- Two scopes (`HistoryScope`): `layout` (`layout.kdl`) and `appearance`
  (`theme.kdl` and `themes/*.kdl`). Every write the `ConfigManager` makes to
  them is one `EditEntry {id, kind, label, at, files}` with the files' text
  before and after: `UpdateLayout`, `SetTheme`, `SetThemePreferences`,
  merges, reverts, and (WP184) the theme operations. At most 50 entries per
  scope, in memory for the running core; a new entry clears the redo list.
  Entry labels are `edit-kind-<kind>` (`core-editors.ftl`).
- `GetEditHistory {scope}`, `UndoEdit {scope, force?}`, `RedoEdit {scope,
  force?}` → `EditHistory {scope, revision, undo, redo}`;
  `Event::EditHistoryChanged {history}` after every change of a history
  **and** after the watcher saw the scope's files change (a new `revision`).
- Why in the core and not in the UI: undo must restore exactly what was
  written (comments and formatting included) and must work across windows,
  pop-outs and remote sessions; only the core has the file texts.

### Changes on disk (conflicts)

- `config_revision(text)` = FNV-1a 64 of the file's text (16 hex digits,
  `none` for a missing file) — for noticing changes, not for security.
- `UpdateLayout` and `EditTheme` take an optional `base_revision` (the
  history's `revision`, or `ThemeDetails.revision`); a file that changed
  since answers `conflict` / `edit-issue-conflict` and nothing is written.
  Node references therefore never hit a node that moved under them.
- Undo / redo check that every file still holds the entry's `after`
  (redo: `before`) text; otherwise `conflict`, unless `force` — then the
  current `theme.kdl` / `layout.kdl` is backed up (ADR 0022 backups) and
  replaced. The UI offers "Undo anyway".

### Remote sessions

`GetEditHistory` / `GetThemeDetails` need a session; every edit, undo,
redo and theme operation is `admin` (like `UpdateLayout` / `SetTheme`).
`EditHistoryChanged` is forwarded to remote sessions.

### What the contract serves

- The core serves `GetEditHistory`, `UndoEdit`, `RedoEdit`, the revision
  check of `UpdateLayout` and the history of every existing write
  (`nail-core/src/editing.rs`, `customize.rs`, `appearance.rs`).
- The new layout patches answer `unsupported` with `edit-issue-not-ready`
  (`nail-config/src/layout_edit.rs`) until WP184. The theme operations check
  the theme they name (`not_found`, `not-editable`), then answer
  `unsupported` / `edit-issue-not-ready` (`nail-config/src/theme_edit.rs`).
  `EditIssue::NotReady` goes away at the integration.
- The UI mock (`MockTransport`) serves the histories
  (`ui/src/api/editHistoryMock.ts`: snapshots per scope, registered by the
  customization and appearance mocks; `simulateExternalEdit` plays a change
  on disk), checks `base_revision`, and hands the new patches to the
  packages' mocks (`features/layout-editor/mock.ts`,
  `features/toolbar-editor/mock.ts`, both answering `MOCK_NOT_READY` until
  their packages land) and the theme requests to
  `features/appearance/themeEditMock.ts`.
- UI contract modules: `ui/src/state/editHistory.ts` (`useEditHistory`,
  `useEditHistoryStep`, `applyEditHistory`; `App.tsx` applies
  `EditHistoryChanged`), `ui/src/layout/LayoutChrome.ts`
  (`LayoutChromeContext {showHidden, renderNode(node, ref, content)}`,
  `nodeRefOfPath`, `rendersNode`): the renderer passes every node through
  the chrome, so customize mode draws pane headers around the real panes.

### Libraries

No new dependencies. Drag and drop and colour inputs come from
`react-aria-components` (already used; Apache-2.0: `useDragAndDrop`,
`DropZone`, `ColorPicker`, `ColorArea`, `ColorSwatch`, `Slider`) or plain
pointer events; every drag has a keyboard alternative.

## Slots

| Package | Owns |
| --- | --- |
| WP181 layout editor | `ui/src/features/layout-editor/`, `ui/src/layout/LayoutRenderer.tsx` + `LayoutChrome.ts`, placing `layout-editor-settings` / `toolbar-editor-settings` on the settings page in `defaults/layout.kdl` (+ `crates/nail-api/fixtures/default-layout.json`), `locales/{en,de}/layout-editor.ftl` |
| WP182 toolbar customizer | `ui/src/features/toolbar-editor/`, `ui/src/widgets/Toolbar.tsx` + `toolbarModel.ts` (context menu, customize overlay), `locales/{en,de}/toolbar-editor.ftl` |
| WP183 appearance + theme editor | `ui/src/features/appearance/` (incl. `themeEditMock.ts`, the `theme-editor` widget), the inspector's "Edit style" in `ui/src/features/customization/Inspector.tsx`, `locales/{en,de}/appearance.ftl` |
| WP184 write-back | `crates/nail-config/` (`layout_edit.rs`, `theme_edit.rs`, `write_back.rs`, `edit_history.rs` extensions, golden files in `tests/golden/`); not `defaults/` |
| WP185 integration | everything else: `nail-core`, `nail-api`, e2e (`tests/m5d.spec.ts`, the a11y window sweep), visual baselines, `userDocs/en` (appearance, themes, customizing the layout, windows), plan and memory |

A package that needs a contract change (a new `EditIssue`, a widget option,
an icon, a Fluent id in `core-editors.ftl`) asks WP185 for it in its report
instead of editing a shared file.

## Consequences

- One write path: every editor change is a typed patch the core validates
  and writes format-preserving; the files stay the single source of truth.
- Undo spans every way the files were written through Nail, and refuses to
  overwrite hand edits silently.
- The history is lost on restart (the files are the state; backups remain).
