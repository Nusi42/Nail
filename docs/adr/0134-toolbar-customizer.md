# ADR 0132 (WP182): Toolbar customizer

Status: accepted · 2026-10-04 · M5d WP182 (the integrator renumbers)

## Context

Plan section 10, wave M5d: "toolbar customizer (right-click a toolbar →
Customize; drag buttons between the bar, 'More' and a palette of available
actions, plug-in actions included)". The contract (ADR 0131) gives the
patches `move-toolbar-item`, `add-toolbar-item`, `remove-toolbar-item`
(`ToolbarSlot {area: bar | more, index}`), the M2c `reset-toolbar`, the
`layout` undo history with `base_revision` checks, the action
`toolbar.customize` and the layout menu `context:toolbar`.

## Decisions

### Where it opens

- The toolbar widget (`ui/src/widgets/Toolbar.tsx`) opens the layout's
  `menu "context:toolbar"` itself on a right-click, on the menu key and on
  every `menu.context` binding (Shift+F10) pressed on one of its buttons.
  `toolbar` is not one of the contract's context-menu parts
  (`CONTEXT_MENU_PARTS`); the bar handles the event before the global
  `ContextMenuHost` (which skips prevented events), so a toolbar inside
  another part (the composer) still gets its own menu. Without a
  `context:toolbar` menu the browser's menu stays.
- Actions of that menu run with the toolbar's name as their argument when
  they have none (`toolbarMenuArgs`): `toolbar.customize ["reader"]`.
- `toolbar.customize [toolbar]` opens the window
  `toolbar-customizer:<toolbar>` (one per toolbar, non-modal, no pop-out);
  without an argument it takes the toolbar around the focused element, else
  the window without an instance, which lets the user pick the toolbar (the
  command palette). The settings card `toolbar-editor-settings` lists every
  toolbar with a Customize button.
- While a toolbar's customizer is open the live bar carries
  `data-customizing` (dashed outline in the skin).

### The window

- Three lists: the bar (without the `more {}` item), the "More" menu's own
  entries and the available actions: Separator, Flexible space, then every
  registered action, layout alias and plug-in command by title (the command
  palette's `buildPalette`, so the titles match the palette's); plug-in
  commands carry a "Plug-in" badge, actions already on the toolbar "On
  toolbar" (they may still be added again). The list shows the first 30
  matches and says how many there are; the search finds the rest (hundreds
  of rows would make the window slow to render and to check with axe).
- The lists always show the layout the core announced (`ConfigChanged`); the
  window keeps no copy of the toolbar. Each change is exactly one
  `UpdateLayout` patch with the layout history's `revision` as
  `base_revision`; after a write the window refetches the history so the next
  edit carries the new revision even before `EditHistoryChanged` arrives.
- What may go where (`model.ts` `canPlace`, mirrored by the mock): the bar
  takes actions, separators and spacers; "More" takes actions, separators,
  headings and submenus. An action moving into "More" becomes a menu item
  with its label and icon (rank and display are bar-only and dropped); a
  heading or submenu never leaves the menu; a `more {}` item is added at the
  end of the bar when the toolbar has none. A destination index is counted
  after the item left its place (`moveDestination`).

### Drag and drop and the keyboard alternative

- Native HTML drag and drop (`draggable`, `dragover`, `drop`), no new
  dependency: dropping on a bar / "More" item inserts before or after it by
  the pointer's half (horizontal on the bar, vertical in "More"), dropping on
  the list's empty space appends, dropping a bar or "More" button on the
  available actions removes it. Targets that cannot take the dragged item do
  not accept the drop. Drop positions show as `data-drop="before"` on the
  item / `data-drop="end"` on the list / `data-drop="remove"` on the palette.
- Every list is a `listbox` with a roving tab stop. Arrow keys / Home / End
  choose; Ctrl+arrow moves the chosen bar or "More" item one place; M moves it
  to the end of the other area; Delete / Backspace removes it; Enter adds an
  available action to the end of the bar, Shift+Enter to the end of "More".
  Focus follows a moved item. The same operations are buttons for the chosen
  item (Move earlier / later, Move to More / toolbar, Remove, Add to toolbar /
  More). The keys are described by the window's intro text
  (`aria-describedby` of every list); results are announced in a
  `role="status"` line.

### Undo, Redo, Reset, conflicts

- Undo / Redo step through the shared `layout` history
  (`useEditHistoryStep("layout")`; the latest layout change, whichever editor
  made it), labelled with the entry's `edit-kind-*` text. "Reset to default"
  sends `reset-toolbar` (itself undoable). Done closes the window.
- An edit or undo on a file changed on disk shows the core's
  `edit-issue-conflict` text; for undo / redo the status line offers "Undo
  anyway" / "Redo anyway" (`force`).

### Styling

- Data parts only (`toolbar-customizer.*`, `toolbar-customizer-settings.*`;
  shared `form.*` controls); the token-only skin is
  `TOOLBAR_CUSTOMIZER_SKIN_CSS` in `features/toolbar-editor/skin.ts`, to be
  emitted by the theme compiler after the shared skin (WP185 wires it in
  `theme/compile.ts`, which this package does not own).

## Consequences

- The mock core applies the three patches with the same model
  (`features/toolbar-editor/mock.ts` → `model.ts` `applyToolbarEdit`), so the
  UI's tests exercise real edits, undo and conflicts; nail-config's
  `layout_edit.rs` (WP184) must follow the same placement rules.
- The window kind and root part names are written as template literals:
  the i18n coverage test treats quoted `toolbar-…` strings as message ids.
