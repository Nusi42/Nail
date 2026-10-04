# ADR 0137: M5d integration — the in-app editors wired, View menu, scenarios

Status: accepted · 2026-10-04 · M5d WP185

## Context

Wave M5d (plan section 10) built the in-app editors in four parallel
packages against the contract of ADR 0131:

- ADR 0133 — customize-layout mode (WP181);
- ADR 0134 — the toolbar customizer (WP182);
- ADR 0135 — Settings → Appearance and the theme editor (WP183);
- ADR 0136 — the write-back of the editor patches and theme operations in
  nail-config (WP184).

The package ADRs were written as `0132-wp182-…` / `0133-wp18x-…` (0132 is
the work-folders ADR) and are renumbered here. WP185 merges the packages,
resolves their requests to the integrator and adds the wiring, the
scenarios, the visual baselines and the documentation.

## Decisions

### What the core serves

The contract already routed every request (`nail-core/src/editing.rs`,
`customize.rs`, `appearance.rs`); with WP184 the new `LayoutPatch` kinds and
the theme operations are real. `EditIssue::NotReady` (`edit-issue-not-ready`)
is gone from nail-api, the catalog and the UI mock (`MOCK_NOT_READY`). A
core test now duplicates a built-in theme, edits a token of the copy with
the details' revision, gets `conflict` for the stale revision and undoes the
edit (`theme_operations_write_files_and_undo`).

### Where the editors are mounted

- **View menu:** the proposal starts Customize layout "from the View menu".
  The M5b shell has no menu bar, so the View menu is the navigation rail's
  right-click menu: a new context-menu part `nav-rail` (key context
  `global`; `nail_api::menus::CONTEXT_MENU_PARTS`, the UI's `resolve.ts` /
  `targets.ts`) and `menu "context:nav-rail"` in `defaults/layout.kdl`
  (header "View", Customize layout, Edit theme, the command palette).
- **Command palette:** `layout.customize`, `toolbar.customize`, `theme.edit`
  and `theme.new`; the two theme actions are now also in the layout's
  `actions` (label and Lucide icon, as every default action must be).
- **Settings → Layout & toolbars:** `layout-editor-settings` (WP181) and
  `toolbar-editor-settings` (WP182, placed now that it has a search entry).
- **Settings → Appearance:** New theme / Edit theme (WP183).
- **Toolbars:** right-click → Customize toolbar (`menu "context:toolbar"`).

### The skins

`LAYOUT_EDITOR_SKIN_CSS`, `TOOLBAR_CUSTOMIZER_SKIN_CSS` and
`APPEARANCE_SKIN_CSS` join the theme compiler's skin list after the composer
skin. The toolbar customizer's areas overflowed their grid row in the
default 820 × 620 window (the footer was painted over the More list): the
customizer now fills the window (`height: 100%`) and the More list takes the
remaining room and scrolls.

### The toolbar customizer loads lazily

The customizer imported `paletteInfos` from the palette dialog's module,
which the palette feature loads lazily, so the build reported an
ineffective dynamic import (a warning fails `npm run build`, ADR 0123). The
action descriptions moved to `features/palette/infos.ts`, and the customizer
window itself is now loaded lazily like the palette (it lists every action
through the app registry, which imports the feature barrels).

### Theme ids from names

The UI showed a file-name hint and sent `RenameTheme.new_id` computed with
NFKD (accents dropped: "Café" → `cafe`) and no prefix, while the core
dropped every non-ASCII letter ("Über" → `ber`) and prefixed ids starting
with a digit (`theme-2024`, required by `is_theme_id`). Both now follow one
rule: NFKD without combining marks, lower-case ASCII letters and digits,
hyphens for the rest, `theme-` before an id not starting with a letter,
`theme` for nothing usable, at most 60 characters (room for `-<n>`). The
core uses `unicode-normalization` (MIT / Apache-2.0, already in the
dependency tree through `comrak` and `unicode-security`) as a direct
dependency of nail-config.

### Scenarios and baselines

- `e2e/tests/m5d.spec.ts`: swap panes from the rail's View menu and confirm
  `layout.kdl` (the pane's comment moves with it), Undo to no file, hide /
  show; add a widget, apply presets, Reset to default, the settings card;
  customize the reader toolbar (keyboard M into More, Undo, drag into More,
  Reset to default); accent and corner radius in `theme.kdl` with Undo; New
  theme ("Ocean Café" → `themes/ocean-cafe.kdl`), a token edit in the theme
  editor applied to the page, Undo, Duplicate as new theme; a `layout.kdl`
  changed on disk → Undo refuses, "Undo anyway" backs the hand edit up and
  restores.
- `a11y-windows.spec.ts` covers the toolbar customizer, the theme editor
  (Colours and Parts), New theme, Rename, Delete theme?, the View and
  toolbar context menus and customize-layout mode (also while a pane is
  picked up), in the default, high-contrast and dark modes.
- The visual harness has four more screens (`layout-editor`,
  `toolbar-customizer`, `appearance`, `theme-editor`; 22 in all) for 5
  themes × 2 densities; their baselines were reviewed. The layout-editor
  screen selects no message: with the pane headers the list's
  keep-the-selection-visible scroll made two captures differ.

## Consequences

- Every structure and style the editors change stays in `layout.kdl` /
  `theme.kdl` / `themes/*.kdl`, written format-preserving, with one undo
  history per scope in the core.
- Follow-ups (plan section 10): remembered collapse of panes in customize
  mode; focusing a widget the editor just added (the core derives its id);
  hidden nodes nested inside a pane are not shown in customize mode; the
  main window's rail / view host is not customizable from the editor;
  docking above / below a node inside a stack wraps it in a split; presets
  drop the old split's min / max sizes and `collapsible`; user themes in
  `themes/` are not backed up before edits (Undo restores them while the
  core runs); the theme editor's preview shows tokens only; the available
  actions list of the toolbar customizer shows 30 rows (search for more).
