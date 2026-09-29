# ADR 0006: Parsing theme.kdl and layout.kdl

Status: accepted · 2026-09-29 · M0 WP1 (`crates/nail-config`)

## Context

`design.md` ("UI customization") defines two user files, `theme.kdl` and
`layout.kdl`, that may be sparse (`extends="default"`), reference theme
tokens as `$color.accent`, are validated with file/line/column errors, keep
the last valid configuration when broken, reload live and come with a JSON
Schema. ADR 0003 fixes the resolved output models. This ADR records the
libraries and the concrete KDL syntax and merge rules chosen while
implementing it.

## Decisions

### Libraries

| Crate | Version | Use |
| --- | --- | --- |
| `kdl` | 6.7 (`span` feature, no `serde`/v1) | KDL v2 parser with byte spans on nodes, entries and identifiers; errors carry spans too. Format-preserving, so the M1 write-back can edit the same document |
| `schemars` | 1.2 (`indexmap2`) | JSON Schema (draft 2020-12) generated from the file models |
| `notify-debouncer-mini` | 0.7 (notify 8.2) | Watching the config directory with debouncing. `notify` 9 and `notify-debouncer-full` 0.8 are release candidates only; the mini debouncer is enough because every event just triggers a re-read |

`knuffel`/`kdl` serde derive were not used: they either target KDL v1 or
lose positions for semantic errors, and the mapping has several
KDL-specific shorthands (see below) that a derive cannot express.

### Pipeline

1. **Read** the KDL into a file model (`ThemeFile`, `LayoutFile`). Every
   node, property and value is checked here; values keep their source
   location (`Located<T>`).
2. **Merge** a file with `extends="default"` onto the parsed shipped
   defaults (compiled in with `include_str!` from `defaults/`). Only
   `"default"` is a valid base in M0.
3. **Resolve**: token references → `var(--nail-…)`, selector parsing, token
   cycles, required `message` fields, toolbar references, `main` window.

The JSON Schema is generated from the file models, so it describes the file
as a JSON object (each KDL section is a property, each named node a map key).
It is snapshotted in `crates/nail-config/schema/*.schema.json`.

### Merge rules for sparse files

| Section | Rule |
| --- | --- |
| theme tokens | Redefining a token replaces its whole definition, including the default's mode variants (a new accent colour should not keep the old dark accent). A `modes { dark { … } }` entry for an existing token overrides just that mode |
| theme parts | Appended; a rule whose selector (part, states, conditions, ancestors) equals an earlier rule is merged into it, later declarations winning |
| icons, sounds, message fields | Override by name / field |
| raw-css | Appended |
| layout windows | A window named in the file replaces all breakpoints of that window; other windows stay |
| actions | Merged field by field |
| keys | A binding of the same key in the same context replaces the earlier one in place; `unbind "<key>"` removes it; others are appended |
| toolbars, menus | Replaced by name |

### Syntax choices not spelled out in design.md

- Tokens: `color "bg" "#fff" dark="#000" high-contrast="#000"`, or
  `light=` instead of the positional value. A `modes { <mode> { … } }` block
  is also accepted. Unitless numbers get `px` for `font-size`, `space`,
  `size`, `radius` tokens and for length properties in parts.
- Split per-child settings are whitespace-separated strings with one entry
  per child: `sizes`, `min-sizes`/`max-sizes` (`none` = unbounded),
  `collapsible` (`true`/`false`).
- `bind "g i" "nav.goto-folder \"Inbox\""` (design form) and
  `bind "x" "view.toggle" "sidebar"` (extra arguments) are equivalent.
  Toolbar and menu items use the same action syntax.
- Top-level `window` nodes (outside any `breakpoint`) have open bounds.
- Actions used by keys, toolbars or menus without an `action` entry get an
  empty entry in the resolved `actions`, so the renderer can rely on it.

### Validation

- CSS properties are a fixed subset (`css::PROPERTIES`); `raw-css` covers
  the rest. Values may not contain `; { } < > \` or load resources
  (`url(`, `image(`, `image-set(`, `@import`, `expression(`, `javascript:`).
  Icon paths accept only SVG path characters. Sound paths must stay inside
  the config directory.
- Widgets: the M0 widgets have typed options (unknown options are errors);
  widgets from the design that are not built yet and `plugin.<id>.<name>`
  accept any options. `grid`, `dock`, `scroll`, `overlay` are rejected as
  "not supported yet".
- Errors are `ConfigError {kind, file, line, column, message}`; the column
  counts characters. Only the first error is reported.

### Keep-last-valid

`ConfigManager::load` never fails: a missing file means the default, an
invalid file at start-up means the default plus a recorded error. Later
reloads (manual or from the watcher) replace a file's configuration only
when the new text is valid; unchanged text is ignored. Updates are reported
as `ConfigUpdate::{Changed, Error}`, which map to `Event::ConfigChanged` /
`Event::ConfigError`. After a `Changed`, the watcher reports the errors of
files that are still invalid again, because clients clear the error
display on `Changed`.

Theme and layout files are shared between users, so they are untrusted
input. The `kdl` parser recurses once per child block (and per nested
block comment), and a deeply nested file overflowed the stack, which aborts
the process instead of returning an error. A cheap lexical pre-scan (it
skips strings and comments) therefore refuses files nested deeper than 64
levels with a syntax error at the offending brace, before `kdl` sees them.

## Consequences

- Writing UI changes back into the files (M1) can reuse the `kdl` documents
  and the source locations kept in the models.
- New sections, containers or widget options are added in the reader, the
  file model (and so the schema snapshot) and the user references
  (`userDocs/en/theme-reference.md`, `userDocs/en/layout-reference.md`).
