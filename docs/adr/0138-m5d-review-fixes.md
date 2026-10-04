# ADR 0138: M5d review fixes — panes keep their sizes, undo not overtaken, ways back from a broken theme

Status: accepted · 2026-10-04 · M5d review

## Context

The two M5d reviews (conformance and UX quality of the in-app editors)
reported defects in the editors integrated by ADR 0137. Each was confirmed
in the code (most also on `nail serve --seed demo`), fixed, and covered by a
regression test (Rust, Vitest and Playwright).

## Decisions

### Panes take their size and limits along (deviates from ADR 0131)

ADR 0131 said a swap leaves the split's `sizes` / `min-sizes` / `max-sizes`
/ `collapsible` with the slots ("each takes the other's size"), and ADR 0136
gave a pane docked beside another the defaults `1fr` / `none` / `false`.
On the default mail view, "Swap Folders with Message" then capped the reader
at the folder pane's 200–480 px and gave the folder tree 1.4fr; docking the
folders right of the reader dropped their width, limits and
collapsibility.

Now a node carries its own entries whenever it swaps or moves between
containers of the same kind — within its container, stack to stack, split to
split of the same orientation (`ListKind` in `nail-config/src/layout_edit.rs`;
the UI mock `features/layout-editor/mock.ts` does the same). Between kinds
(a stack's height is no split width) the slot entries stay / new entries
are written as before. Presets write their own `sizes`, and each pane they
place in a split keeps its `min-sizes` / `max-sizes` / `collapsible` from a
split of the same orientation: its current one, else the default layout's
(so Three panes after Reader below gets the default limits back). The
deferral "presets drop min / max / collapsible" is resolved.

### Undo never overtaken by a delayed write

The Appearance sliders / colour picker and the theme editor write after a
400 ms pause. Undo right after a change undid an older entry, then the
delayed write put the change back (and Redo was gone). Forms with a waiting
write register a flush (`features/appearance/pendingWrites.ts`); the
history buttons write everything waiting first and then undo — which undoes
the change just made. `useThemeEdits.flush` now resolves only once every
write, including one already on its way, is answered; the overrides form
uses the latest preferences for a delayed write. The layout editor's Undo /
Redo mark the session busy synchronously: a double-click undoes one entry.

### Ways back from an unreadable theme

The theme editor accepted white text on white; the editor (styled by the
theme it edits) became unreadable with no visible way back. Kept: edits are
not refused (the user may be on the way to another palette). Added:

- a contrast warning in the editor for the text pairs of
  `tests/theme_contrast.rs` (fg on bg / bg-subtle / bg-raised / canvas,
  fg-muted on bg, accent-fg on accent, selection-fg on selection; hex values
  only; WCAG 4.5:1), naming Ctrl+Z and "Reset appearance"
  (`features/appearance/contrast.ts`);
- Ctrl+Z / Ctrl+Y (Ctrl+Shift+Z) in the Appearance page, the theme editor and
  customize-layout mode (`historyKey` in `state/editHistory.ts`). Text fields
  keep their own text undo unless they hold their saved value (marked
  `data-history-keys`); the token / property fields no longer remount on a
  write, so the focus stays and Ctrl+Z right after Enter undoes the edit;
- the palette action `theme.reset` "Reset appearance" (also in the rail's
  View menu): Follow Windows with Frost / Graphite and no overrides; the
  density and the theme files stay; one undo entry. It needs only Ctrl+K and
  typing.

Keeping the editor chrome on a fixed built-in theme was not chosen: the
editor is a normal window of the themed app, and the warning plus the three
ways back cover the case.

### Appearance overrides in the theme editor

Overrides (`tokens` of the user's `theme.kdl`: accent, body text size,
corner radius) win over every theme, so editing `color.accent` of the active
theme changed nothing visible while the preview showed the edit. The editor
now tags such a token "Overridden in Appearance" (with the value in use)
and offers "Clear override", and the preview and contrast check lay the overrides over the
theme (`overrideTokens` / `withOverrides` in `tokens.ts`).

### Customize layout follows the view on screen

Customize mode of a view the user left stayed active (the view stays
mounted while hidden), and the next "Customize layout" only ended it. Now
switching views ends the session (`CustomizeViewWatcher`, a feature effect),
and the action customizes the shown view's window when another window is
being edited (it is a toggle only for the same window).

### Scenarios

`e2e/tests/m5d.spec.ts` gains: swapped panes keep widths and limits (file and
on-screen widths), docking on every side, Remove from layout, Redo, one undo
per double-click, Ctrl+Z / Ctrl+Y, customize mode following the view, Undo
right after a slider / picker change, rename (with the file) / delete / undo
of a theme, a part override from the inspector's "Edit style", the contrast
warning with Ctrl+Z and Reset appearance, an Appearance override named and
cleared in the editor. The existing scenarios check the files: the
duplicated theme's name / `extends` / tokens, the toolbar after drag, undo
and reset, the exact radius value.

## Consequences

- Moving a pane keeps its geometry; a pane moved between container kinds
  still gets defaults (documented in `userDocs/en/customizing-the-layout.md`).
- Undo / Redo of the appearance scope may first write a waiting change; the
  history then shows the undone change in Redo.
- Follow-up: a visible "View" affordance on the rail (the View menu is still
  the rail's right-click menu; the palette button on the rail lists the
  editors). A rail button changes every visual baseline and is left to the
  maintainer's review of the real app.
