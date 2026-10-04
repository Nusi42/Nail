# ADR 0133: WP183 — Appearance settings and the theme editor

Status: accepted · 2026-10-04 · M5d WP183 (package of ADR 0131; the integrator renumbers)

## Context

Plan section 10, wave M5d, WP183: Appearance settings (theme gallery with
live previews, "New theme", the Follow Windows pair, accent, density, text
size, corner radius) and the full theme editor (token groups for colour,
type, spacing, radius and motion; per-part overrides; live preview;
duplicate / rename / delete; the inspector's "Edit style" jumps to the
part), on the contract's theme operations (ADR 0131: `GetThemeDetails`,
`EditTheme`, `DuplicateTheme`, `RenameTheme`, `DeleteTheme`, the
`appearance` undo history). The approved proposal's "Theme editor" section
shows the look. Slot: `ui/src/features/appearance/`, the inspector's "Edit
style" entry, `locales/{en,de}/appearance.ftl`.

## Decisions

### The editor is a window and a widget

- `theme-editor` is both a dialog (a non-modal tool window, 1100 × 760,
  minimum 560 × 420, no pop-out) and the contract's layout widget kind, so
  a user can also place it in `layout.kdl`. The default layout does not
  place it; Settings → Appearance and the inspector open the window, and
  the actions `theme.edit` / `theme.new` (palette, key bindings) do too.
- What the window shows (theme, part to jump to, the subject of Rename /
  Delete, the base of New theme) lives in a small Zustand store per UI
  store (`editorStore.ts`), like `widgets/DeletePermanently.tsx` keeps its
  pending messages: windows have no props, and pop-outs have their own UI
  store.
- Further windows: `theme-new` (name, "start from", "use it now"),
  `theme-rename` (name, optionally the file name too = `new_id`),
  `theme-delete` (a confirmation, the only modal one).

### What the editor edits

- User themes (`EditTheme` target `theme`). Built-in themes open read-only
  with a notice and "Duplicate as new theme"; when the built-in was in use,
  the copy replaces it (its half of the Follow Windows pair, else the fixed
  theme: `themeChangeFor`), and the editor continues on the copy — also
  after "Edit style", so a part can be styled in two clicks from any theme.
- The user's own `theme.kdl` (target `user`) is not edited by the editor:
  its accent / text size / radius are the Appearance page's overrides
  (`SetThemePreferences`), as before. The mock supports target `user` for
  completeness.
- Tabs: Colours, Type (font, font-size, font-weight, line-height), Spacing
  (space, size), Corners (radius), Motion, More (every other category, so
  nothing in the schema is unreachable), Parts. Tokens show their origin
  (base / inherited / this theme), a colour picker for colours, and the
  value as written in KDL (`#3b5bdb`, `14`, `$color.fg`, `calc(…)`); "Values
  for" switches between the base value and the `dark`, `high-contrast`,
  `print` and `compact` variants (`set-token` `mode`). A theme's own value
  can go back to the inherited one (`value: null`).
- Parts: every selector a rule of the theme names, searchable (at most 200
  listed), plus any part typed in or jumped to without a rule. The theme's
  own properties are edited in place or removed; inherited ones can be
  overridden (copied into the theme's rule) and show struck through once
  overridden; a property can be added (common names offered, any accepted);
  "Reset part" removes the theme's own rule. The core validates for real
  (`edit-issue-invalid-part`); the UI only screens obvious typos.

### Live edits

- Every change shows at once as a draft (field, row marked "changed",
  preview) and is written after a 400 ms pause (Enter / leaving a field
  writes at once). Changes in one pause are one `EditTheme` = one undo
  entry. Writes never overlap; each carries the revision the previous
  answer left (`ThemeDetails.revision` read from the query cache at send
  time), so a file changed on disk answers `conflict` and nothing is
  overwritten; the editor then offers "Reload the theme". Pending changes
  are written when the editor closes or switches themes.
- The preview is a miniature mail window drawn with the edited theme's own
  values (drafts included, in the mode shown): references (`$color.accent`
  as written in KDL, `var(--nail-…)` as compiled) are resolved through the
  same theme (`tokens.ts` `resolveTokenValues`); each element's colours and
  sizes come from a geometry rule (like the gallery swatches), because the
  style manager accepts no custom properties. When the edited theme is in
  use, the app itself restyles after each write (`ConfigChanged`).
- The Appearance page's accent picker and the new text-size / corner-radius
  sliders apply live the same way (after the pause); the accent text field
  still applies with Enter / Apply, "Use theme defaults" clears all three.

### Undo

Undo / Redo of the `appearance` history sit on the Appearance page and in
the editor; the buttons name the step ("Undo Edit theme"). A conflict
offers "Undo anyway" (`force`). `EditHistoryChanged` of the appearance
scope refetches the editor's details (undo, redo, changes on disk).

### Styling

The new parts (`theme-editor.*`, `theme-editor.preview.*`, `theme-dialog*`,
`appearance.header`, `appearance.history`, `appearance.slider`,
`appearance.range*`) are styled by `APPEARANCE_SKIN_CSS`
(`features/appearance/skin.ts`, tokens only, a container query stacks the
editor below 52rem); the integration registers it in
`theme/compile.ts` after the M5c skins. Everything else uses the shared
primitives (`form.*`, `tabs.*`, `tag`, `card`).

### Mock core

`themeEditMock.ts` answers the five requests like the core: user theme
files with tokens / part rules, ids from names (unique, `-2` …), details
in schema order with origins, `base_revision` checks, one undo entry per
write (the history snapshot now covers the files), the selection following
a renamed file and falling back on delete; a user theme in use is laid over
its built-in by the mock's resolver (`themes.ts` `ThemeLayer`), so the app
restyles in `?mock=1` and the UI tests.

## Consequences

- Built-in themes stay untouched; every change is in a user theme file or
  `theme.kdl`, written by the core with comments kept, and undoable.
- The preview shows tokens, not part rules; part rules show in the app when
  the theme is in use.
- Requests to the integration (WP185): register `APPEARANCE_SKIN_CSS`; the
  contract test `api/editHistoryMock.test.ts` expects `DuplicateTheme` to be
  not ready; e2e (`m5d.spec.ts`, the a11y window sweep for `theme-editor`,
  `theme-new`, `theme-rename`, `theme-delete`); `m5a.spec.ts` still finds
  the accent field, Apply and "Use theme defaults"; user docs.
