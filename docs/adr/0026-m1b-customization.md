# ADR 0026: Customization tooling — all errors, write-back, merge, inspector

Status: accepted · 2026-09-29 · M1b WP24

## Context

Plan WP24 (design.md, "UI customization"): report every configuration error
at once, write UI edits back into `layout.kdl` keeping comments and
formatting, a merge dialog for shared themes and layouts with backups and
revert, an inspector, and smart folders. The protocol is fixed by ADR 0022
(`UpdateLayout`, `Preview/Resolve/ApplyConfigMerge`, `ListConfigBackups`,
`RevertConfig`, `OpenConfigFile`, `ConfigErrors`). WP24 owns `nail-config`
(except `rules.kdl`) and `ui/src/features/customization/`; the core's
`customize.rs` is wired by the integrator (see "Integration").

## Decisions

### Every error of a file (`nail-config`)

- Readers keep going after a bad item: an `Errors` collector records the
  error of one token, part declaration, icon, message setting, sound,
  section, window tree child, binding, action, toolbar item or menu, and
  reading continues with the next one. Syntax errors are every diagnostic
  of the `kdl` parser. Resolution (token references, cycles, missing
  message settings, raw CSS, `main` window, toolbar references) also
  collects all errors, but runs only on a file that read cleanly — one
  typo must not cause a cascade of follow-up errors.
- New functions return `Vec<ConfigError>` (never empty, file order):
  `parse_theme_all`, `parse_layout_all`, `parse_accounts_all`,
  `ThemeFile/LayoutFile::parse_all`, `resolve_all`. The existing
  first-error functions keep their exact M1a behaviour (same first error),
  so the core and its tests are unaffected. `accounts.kdl` recovers per
  top-level node.
- `ConfigManager` keeps all errors per file (`file_errors`,
  `error_events`); `errors()` still returns the first error per file.
  `ConfigUpdate::Errors {kind, errors}` is new: after an invalid file the
  watcher sends `Error` (first, M1a event) then `Errors` (all); a file that
  becomes valid gets `Changed` then an empty `Errors`. `to_event()` maps it
  to `Event::ConfigErrors`, so the core's existing watcher callback
  broadcasts it without changes. `issues_event(file, errors)` builds the
  event for any file (accounts, rules); errors located in another file (the
  shipped defaults) get that file name as a message prefix.

### Write-back (`apply_layout_patches`, `ConfigManager::update_layout`)

- Edits go through the `kdl` crate's document tree, which keeps every byte
  of formatting in node/entry formats; only touched values change
  (`value_repr` is updated with the value). `kdl_edit.rs` holds the few
  operations needed: set a property in place, append rendered nodes to a
  block (turning a one-line block into a multi-line one), replace a node
  keeping its leading comments and trailing comment, remove a node with
  the comment block directly above it (comments separated by a blank line
  stay, as in the accounts writer). Golden files in
  `crates/nail-config/tests/golden/` pin the output.
- No file yet → a sparse `layout "custom" extends="default"` file. A node
  addressed in a window the sparse file does not define: that window (all
  its breakpoints) is copied from `defaults/layout.kdl` first, because a
  window in a sparse file replaces all of its default breakpoints.
  `LayoutNodeRef.breakpoint` is the occurrence order of the window in the
  file, which is what `ResolvedLayout.windows[name]` is.
- Keys are compared normalized (`Ctrl+B` = `ctrl+b`); a binding replaces
  the user's last binding of that key in place, else goes into the last
  `keys` block of the context (or a new block). Removing a binding the
  defaults define writes `unbind`; a `keys` block emptied by a removal
  goes. Toolbar reordering reuses the existing item nodes (with their
  comments). Column order writes `columns="…"`; `message-list` gained the
  `columns` option for it (the row view ignores it).
- The result is validated with the full parser; a patch that would make
  the file invalid (unknown action, wrong number of sizes, unknown window
  in a complete file) is refused (`invalid_request`), an invalid file on
  disk is never touched (`config`). Writes are atomic (temp file + rename)
  and serialized by a lock; the file is re-read at once, so the returned
  `ConfigUpdate`s describe the new state and the watcher reports nothing
  twice.

### Merge engine (`preview_merge`, `resolve_merge`, `ConfigManager::apply_merge`)

- Both sides are compared **as they take effect**: a sparse file merged
  onto the defaults, a complete file as it is. Items: name; `mode`; tokens
  (`category.name`, with their mode values); part rules grouped by selector
  text; icons; message settings and sounds (section `message` — the
  contract has no `sounds` section); raw CSS by part; windows (all
  breakpoints); actions; effective key bindings (`context key`); toolbars;
  menus. Each item is shown as canonical KDL (`render.rs`); equal canonical
  text means unchanged. Unchanged items are listed only when the incoming
  file names them. Ids are `<section>:<key>` (`tokens:color.accent`,
  `keys:list: e`, `message:sound:sounds.mail.new`).
- Writing: the user's file is edited in place; an item taken that the file
  defines exactly once is replaced where it is (keeping its comments),
  otherwise all its definitions are removed and the new one is appended
  to its section. Taking every differing item writes the incoming text
  verbatim; keeping all writes nothing new. A sparse file cannot remove an
  item the defaults define: such "removed" items are flagged `invalid` and
  stay (key bindings become `unbind`).
- Conflicts: `duplicate-key` (same key and context bound differently, a
  key that is a prefix of another sequence in a related context, or a
  global binding a context binding would hide); `unknown-action` (not in
  the defaults, the user's layout or an alias the incoming file defines;
  `plugin.*` never, since no plug-ins exist in M1); `unknown-widget`
  (`plugin.*` widgets); `drops-plugin-items`; `invalid` (a token reference
  only the incoming theme defines, default items a sparse file cannot
  remove).
- Stateless as ADR 0022 requires: every request re-reads the user's file
  and recomputes the comparison.

### Backups, revert, open file

- Backups live in `<config dir>/backups/<stem>-<unix millis>.kdl`
  (`.absent` when the file did not exist, so reverting removes it again);
  the newest 20 per file are kept; ids are the file stems (strictly
  parsed, no path traversal). Revert backs the current file up first.
- `ConfigManager::open_file_command` returns the `Command` to spawn: the
  editor from `NAIL_EDITOR` / `VISUAL` / `EDITOR` (templates with
  `{file}`, `{line}`, `{column}`; known editors get their go-to-line
  arguments), else Notepad on Windows. A missing theme / layout file is
  created as a sparse file first.

### UI (`ui/src/features/customization/`)

- Feature state is a Zustand store of the slot (errors per file, inspector
  state), fed by the slot's event handler. `ConfigError` (M1a) is honoured
  for a file until it sends `ConfigErrors`.
- Inspector: a feature effect renders the overlay; the highlight box and
  the panel are positioned through geometry rules of the theme's
  stylesheet (`useGeometry`, including `position: fixed`), never inline
  styles. Matching theme rules are found by testing the compiled selectors
  (`theme/compile.ts`) against the element with pseudo states removed; the
  layout node is found by the nearest `data-node-id` in the window and
  breakpoint shown. Widgets carry their source position in the resolved
  layout; containers have none (a contract limitation, see follow-ups).
- Split write-back: a feature effect watches `uiStore.splitSizes` and sends
  `UpdateLayout` 600 ms after the last change, addressing splits by tree
  path or by id (breakpoint shown first).
- Merge dialog (`config-merge`): the live preview puts the resolved result
  into the query cache's configuration; closing without applying restores
  and re-fetches the real one. Config errors (`config-errors`) and smart
  folders (`smart-folders`) are widgets; the error list is also a dialog.
- The mock core answers every WP24 request; its merge understands only
  the file name, theme token lines and layout `bind` lines (enough for
  demos and UI tests).

## Integration (for WP25)

`nail-core/src/customize.rs` maps each request to one `ConfigManager` call
and broadcasts the returned updates with `ConfigUpdate::to_event`;
`OpenConfigFile` spawns `open_file_command` (`unsupported` in web mode).
Connect-time error events should add `ConfigManager::error_events()` plus
an `issues_event` for `accounts.kdl` / `rules.kdl`. Details are in the
package report.

## Consequences

- Users see every mistake at once; the first-error API keeps M1a callers
  and tests working unchanged.
- Formatting of user files survives UI edits and merges; golden files
  guard it.
- Known limits: containers have no source position for the inspector;
  toolbar / key / column edits have write-back helpers but no editing UI
  yet; the merge of a part rule taken from a complete incoming theme keeps
  default declarations the incoming theme leaves out.
