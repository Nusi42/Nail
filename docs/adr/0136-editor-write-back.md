# ADR 0136: Write-back of the M5d editor patches and theme operations (M5d WP184)

Status: accepted · 2026-10-04 · M5d WP184 (the integrator renumbers)

## Context

ADR 0131 fixed the M5d layout patches (`move-pane`, `swap-panes`,
`add-widget`, `remove-node`, `set-hidden`, `apply-preset`, `reset-window`,
the toolbar item moves) and the theme operations (`EditTheme`,
`DuplicateTheme`, `RenameTheme`, `DeleteTheme`, `GetThemeDetails`), and left
them answering `edit-issue-not-ready` in `nail-config/src/layout_edit.rs` and
`theme_edit.rs`. WP184 implements them in nail-config: format and comment
preserving, golden files per patch kind, recorded in the undo history, and
refused with `conflict` when the file changed on disk since the editor read
it.

## Decisions

### Moving KDL nodes (`kdl_edit.rs`)

The M1b helpers only appended, replaced and removed nodes in one place. M5d
moves subtrees between depths, so `kdl_edit` gains:

- `extracted` / `detach`: a node taken out for moving carries the comment
  block directly above it (the part of its leading text after the last blank
  line); comments separated by a blank line stay where they were
  (`remove_node`'s rule).
- `reindent`: the node is printed and every line is re-indented by the depth
  difference in the file's indentation unit, then parsed again. Text that no
  longer parses (only a multi-line string could do that) refuses the edit.
- `put`: inserts a detached node at an index, moving the line break after
  `{` (held by the first child's leading text in the `kdl` crate) as needed;
  an empty block's closing indentation moves behind the new node.
- `replace_with`: replaces a node keeping the slot's leading text (only the
  separated comments for a swap; everything for a split replaced by its only
  child).
- `set_arg`: a positional argument in place (token values).

`remove_node` also handles CRLF files at the first child (it only looked for
`\n`) and keeps a blank line above a removed node when the next node has
none.

### Layout patches (`layout_edit.rs`)

- Paths are KDL paths from the `window` node; every step of a
  `LayoutNodeRef` must go through a container, so a widget's `row`s are never
  addressed (`edit-issue-node-not-found`).
- References name the tree before the patch. While a move rearranges the tree,
  the target and the old parent carry temporary marker properties
  (`__nail_edit_target`, `__nail_edit_source`) that are removed before the
  text is produced; this keeps `move-pane` correct when the old parent
  disappears or is the dock target.
- Per-child lists: a split's `sizes` / `min-sizes` / `max-sizes` /
  `collapsible` and a stack's `sizes` lose the leaving child's entry and get
  `1fr` / `none` / `none` / `false` (stack: `auto` for bar-like widgets) for
  an arriving one; only lists the file writes are touched. A move inside its
  own container carries its entries along; a swap leaves them with the slots
  (ADR 0131: each node takes the other's size). `tabs active=` follows its tab.
- Tidying: an emptied container goes, a split left with one child is replaced
  by it (comments of both stay); removing or emptying the window's root is
  `edit-issue-invalid-placement`. Moves across windows or breakpoints are
  refused the same way.
- Docking beside a node goes into the parent only when the parent is a
  *split* of the side's orientation (ADR 0131's wording); next to a node in a
  stack the node is wrapped in a new split.
- `add-widget` checks the kind against the widget registry
  (`edit-issue-unknown-widget`), derives a unique id from the kind
  (`mini-month`, `mini-month-2`, …), writes a plug-in panel's `panel` as the
  argument, and reads the line on its own first so invalid options answer the
  config's own error.
- `apply-preset` resolves the preset from the file (the defaults' presets
  through `extends`), finds the named panes by id, keeps the root's id, shows
  the named panes and puts every other pane, hidden, after the arrangement in
  a root stack (`sizes="1fr auto …"`). Applying the same preset twice changes
  nothing. A named pane inside another named pane is refused.
- `reset-window` removes the window from a sparse file; a full file gets the
  default definitions back where the first one was.
- Toolbars: the bar's items are the children other than `more`; "More" is
  created at the end of the bar when needed; items going into "More" lose
  `rank` / `display`; spacers never go into "More", submenus / headers never
  onto the bar. A toolbar only the defaults define is copied into a sparse
  file with its `hide-disabled` / `overflow` settings.

### Theme operations (`theme_edit.rs`)

- Values are written as KDL numbers when they are plain numbers (so `px` is
  added where the category or property wants it), else as strings.
- A token's base value is edited in place or appended to the first `tokens`
  block; a mode value becomes the line's property, or a line in
  `modes { <mode> { … } }` when the file has no line for the token. Removing a
  base value removes the token from the file (line and `modes` lines);
  removing a mode removes only that value. Emptied `modes` blocks go.
- A part edit changes one declaration of the file's last rule for the
  selector (created when missing); emptied rules and `parts` blocks go.
- Each patch is first read on its own (`edit-issue-invalid-token` /
  `-invalid-part` with the config's diagnostic as `detail`); the edited file
  must then still work: the user's `theme.kdl` resolves with the selection, a
  user theme is usable in a catalog with the new text (a dangling `$token`
  reference fails here).
- `DuplicateTheme` derives the id from the name (lowercase ASCII words joined
  by `-`, `theme-` in front when needed) and makes it unique against every
  theme id, the base and the files in `themes/`; the copy keeps the source's
  text and comments with the new name.
- `RenameTheme` with `new_id` moves the file and points `theme.kdl`
  (`extends`, `appearance light= / dark=`) and every user theme extending it
  at the new id — one undo entry with all files. `DeleteTheme` falls back to
  Frost / Graphite / High contrast by the theme's kind (and Frost / Graphite
  for the pair).
- `GetThemeDetails`: tokens in the merged theme's order (base order first),
  origin `own` when the theme's file names the token (a mode value counts),
  `extended` when a theme it extends does, else `base`; parts base first,
  then the extended themes farthest first, then its own.
- Every operation writes under the write lock, checks `base_revision` where
  the request carries one, backs up `theme.kdl` before writing it, and records
  one `appearance` entry with a `FileChange` per touched file; an `EditTheme`
  without patches writes nothing.

## Consequences

- `EditIssue::NotReady` is no longer produced by nail-config; the integrator
  removes it (and the core test that expected `unsupported` for `EditTheme`).
- Every patch kind has a golden file (`tests/golden/layout_edit/`,
  `tests/golden/theme_edit/`; `NAIL_UPDATE_GOLDEN=1` rewrites them).
- Preset application rewrites the window root, so comments inside the old
  arrangement's containers (not attached to a pane) are not kept; the panes'
  own comments are.
