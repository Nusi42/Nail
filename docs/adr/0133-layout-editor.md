# ADR 0133: the layout editor (M5d WP181)

Status: accepted · 2026-10-04 · M5d WP181

## Context

ADR 0131 fixed the contract of the in-app editors: layout edits are
`LayoutPatch`es through `UpdateLayout` (with the layout history's revision as
`base_revision`), one undo history per scope, hidden nodes, presets, and the
layout chrome hook in the renderer. WP181 builds customize-layout mode as the
approved proposal's "Layout editor" section shows: pane headers with drag
handles, dock and swap targets, a hide / show eye, the widget palette ("Add
to layout"), the presets (Three panes, Reader below, Wide reader, Focus),
Undo, Reset to default and Done, and a keyboard alternative for every drag.
WP181 owns `ui/src/features/layout-editor/`, `ui/src/layout/LayoutRenderer.tsx`
and `LayoutChrome.ts`, the settings-page placement of its widget in
`defaults/layout.kdl` (plus the nail-api fixture) and `layout-editor.ftl`.

## Decisions

### A chrome registered per window

`LayoutChrome.ts` gains a registry (`setWindowChrome(window, chrome)`,
`useWindowChrome`). `WindowView` provides the chrome registered for its
window and lets it frame the whole window (`renderWindow`, new and optional).
A chrome may name its `window`; a nested window of another name (the view
host's window inside the main window) then renders plain
(`chromeForWindow`). So customize mode needs no change in the view host,
the window layer or the pop-out host: the same `WindowView` everywhere
draws the editor for the window being customized.

The renderer gives a hidden node that the chrome still shows (customize mode)
only the room of its header: an `auto` row in a stack, `flex: 0 0 auto`
without limits in a split. Customize mode also shows the panes `view.toggle`
hid for the session.

### Panes

A pane is a named node (a widget, or a container with an id or label) that
is a child of the window's root or of a split / tabs, and inside no other
pane (`model.ts` `isPane`). In the default mail view these are Folders,
Messages and Message; the widgets inside them get no header. A window that is
one widget shows it as a fixed frame (lock icon): it cannot move, hide or go,
but panes can be docked beside it. Hidden nodes inside a pane stay out of
sight while customizing (only panes are hidden by the editor).

The pane's real content stays rendered under the header, `inert`: nothing in
it can be clicked or focused while customizing, so Tab moves from pane header
to pane header.

### One model for pointer and keyboard

Moving is a session state (`store.ts`: `moving` = a pane reference or a
palette widget). It is set by pressing a pane's handle or a palette entry
(Enter / Space / click: "pick up"), or by dragging it (pointer events past a
4 px threshold, `dnd.ts`). While something is picked up, every other pane
shows its drop targets as real buttons in a compass (`left`, `right`, `top`,
`bottom`, and `swap` for a pane or `into` for a widget dropped on a
container); the keyboard tabs to them, the pointer hovers them (hit-tested
with `elementFromPoint`) and releasing over one clicks it. So both paths run
the same code and the targets the pointer sees are the ones assistive
technology reads ("Put Folders right of Message"). Escape puts back what is
picked up (the focus returns to its handle), else ends customize mode; after
an edit the focus goes to the moved pane's handle or the editor's title.
While the focus is in the editor the app's single-key shortcuts do not apply
(`data-key-modal`), so a key never acts on the mail behind it.

Drops map to `swap-panes`, `move-pane` with a `beside` placement, or
`add-widget` (`model.ts` `dropPatch`).

### Presets, palette, Undo, Reset

- Presets come from `ResolvedLayout.presets` of the window; one is enabled
  only when the breakpoint has every pane it names (`presetFits`), and
  pressed when the breakpoint shows exactly its arrangement (`presetActive`:
  its panes in its containers, nothing else shown, sizes aside).
- "Add to layout" offers mini month, today's agenda (`days=1`), task list,
  smart folders and the panels of enabled plug-ins (`plugin-panel` with
  `panel="<plugin>.<panel>"`).
- Undo / Redo use the layout history (`useEditHistoryStep`); their tooltip
  names the entry. A `conflict` (the file changed on disk) offers "Undo
  anyway" (`force`). An edit refused for a stale revision shows the core's
  message in the status line.
- Reset to default is `reset-window`; Done (or Escape) ends the mode and
  gives the focus back to where it was.

### Entering the mode

`layout.customize` customizes the shown view's window (the main window for
a layout without a view host) and toggles off again. The settings card
(`layout-editor-settings`, Settings → Look & feel → Layout & toolbars) picks
a view's window, applies its presets directly, resets it, or starts
customizing it (the view is shown first).

### Look

`skin.ts` (`LAYOUT_EDITOR_SKIN_CSS`), token-only like the other skins: the
banner on the accent colour (the accent / accent-fg pair of primary
buttons), dashed pane frames, the compass of drop targets, the palette
cards. Buttons are the shared `form.button`. A test checks that every part
the editor renders is styled by the skin.

### The mock core

`mock.ts` implements the seven patches on the resolved layout as ADR 0131
documents them (placements against the tree before the change, docking into
a split of that orientation else wrapping in `1fr 1fr`, tidying empty
containers and one-child splits, swaps keeping slot sizes, presets keeping
unnamed panes hidden in a stack at the root, reset from the shipped
default) and answers problems with the `edit-issue-*` id.

## Consequences

- The integration wires `LAYOUT_EDITOR_SKIN_CSS` into `theme/compile.ts`
  (after `COMPOSER_SKIN_CSS`) and the feature's `messages.ts` into the
  catalog coverage test.
- WP182's `toolbar-editor-settings` is placed on the settings page by the
  integration, once it registers its `settings` search entry (the settings
  registration test requires one for every widget on the page).
- `nail-config`'s `apply-preset` must keep the panes a preset does not name
  hidden in a stack at the root, as the mock does, so the editor can show
  them again.
