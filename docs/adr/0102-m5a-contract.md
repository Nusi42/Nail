# ADR 0102: M5a contract — themes, selection, density, overrides, token schema, Lucide icons, slots

Status: accepted · 2026-10-03 · M5a WP150

## Context

Milestone M5 (plan section 10) implements the UX proposal the maintainer
approved (a published page, not linked here,
memory `ux-overhaul-proposal`). Wave M5a lays the visual foundation: five
built-in themes (Frost, Sage — light; Graphite, Nocturne — dark; High
contrast), "Follow Windows" between a light and a dark theme, Comfortable /
Compact density for every theme, the user's text-size / corner-radius /
accent overrides, a shared token schema (4 px spacing grid, type scale in
Segoe UI Variable, radius, elevation) and one icon set (Lucide). Four
packages build in parallel against this ADR: WP151 (theme files, default
token scales, contrast test), WP152 (theme loading and selection in
`nail-config` and the core), WP153 (the UI) and WP154 (visual-regression
harness); WP155 integrates.

Scope decisions that bind the contract (plan section 10): everything stays
expressible in `theme.kdl` / `layout.kdl`; components keep emitting
`data-part`s and never hard-code anything visual; the font is Segoe UI
Variable with the system stack as fallback; icons are `lucide-react` (ISC).

The port fix that precedes this contract (separate commit) is documented in
`docs/dev-setup.md` "Development ports" and the CLAUDE.md rule "No fixed
development ports".

## Decisions

### Themes and where they live (`nail_api::appearance`)

- **Built-in themes** are ordinary theme files `defaults/themes/<id>.kdl`
  (`BUILTIN_THEMES`: `frost`, `sage`, `graphite`, `nocturne`,
  `high-contrast`, in gallery order), each `theme "<name>" extends="default"
  mode="light|dark|high-contrast" { … }` on top of the shipped base
  `defaults/theme.kdl` (`BASE_THEME_ID` = `default`, valid in `extends`,
  never listed). `nail-config` embeds them (`themes.rs`:
  `BUILTIN_THEME_FILES`, `builtin_theme`, `resolve_builtin_theme`,
  `builtin_theme_infos`, `kind_of_mode`).
- **User themes** are `<config-dir>/themes/<id>.kdl` (`THEMES_DIR`), the
  same format; the id is the file stem (`is_theme_id`: kebab-case, ≤ 64).
  They may extend `default`, a built-in or another user theme (cycles are a
  `theme-problem-extends-cycle`). A user theme with a built-in's id is
  listed with `theme-problem-shadows-builtin` and never applied; a file that
  does not parse is listed with `theme-problem-invalid` (detail = the
  error) — listed anyway so the user sees it, refused by `SetTheme`.
- **Metadata** (`ThemeInfo`): `id`, `name` (`theme "<name>"`, user data),
  `label` (Fluent id `theme-name-<id>` for built-ins; the UI prefers it),
  `kind` (`ThemeKind` light / dark / high-contrast, from `mode`; `system` /
  none reads as light), `source` (`ThemeSource` builtin / user), `extends`,
  `file`, `problem` (`LocalizedMessage`). The plan's "kind light|dark" is
  extended by `high-contrast` so Follow Windows never offers High contrast
  as a light or dark partner and the gallery can group it.

### Selection and overrides in the user's `theme.kdl`

The selection is part of the user's `theme.kdl`, so it stays expressible
in the file and a hand-written file and the Appearance settings edit the
same thing:

```kdl
theme "My settings" extends="frost" {
    appearance follow-system=#true light="frost" dark="graphite" density="compact"
    tokens {
        color "accent" "#3b5bdb"   // ThemeOverrides.accent
        font-size "body" 15        // ThemeOverrides.font_size
        radius "md" 8              // ThemeOverrides.radius
    }
    // the user's own part rules, as before
}
```

- `extends` = the **fixed theme** (`AppearanceSettings.theme`). The
  `appearance` node (`APPEARANCE_NODE`, every property optional) holds
  `follow-system` (bool), `light` / `dark` (theme ids of the pair; kept
  while Follow Windows is off, so switching it back on remembers the pair)
  and `density` (`comfortable` / `compact`).
- Defaults (`AppearanceSettings::default`): Follow Windows on with Frost /
  Graphite, Comfortable. No `theme.kdl`, or `extends="default"` without an
  `appearance` node, means exactly that — existing user files that extend
  `default` get the new default look and keep their part rules. A file with
  `extends="<theme>"` and no `appearance` node is a fixed theme
  (`follow-system` defaults to on only for `default` / none).
- **Overrides** (`ThemeOverrides`) are ordinary tokens of that file:
  `font-size "body"` (11–20 px, `FONT_SIZE_RANGE`), `radius "md"` (0–20 px,
  `RADIUS_RANGE`), `color "accent"` (`#rgb` / `#rrggbb`, `is_hex_color`).
  `None` = the token line is removed. `ThemeOverrides::issues` returns
  `ThemeOverridesIssue` (`theme-overrides-issue-*`, args `min` / `max`).
  The base theme defines the other type steps and radii relative to these
  tokens (WP151), and derived accent colours (soft accent, selection) by
  `color-mix` from `color.accent`, so one value restyles consistently. An
  accent override applies to the light and the dark theme of a pair alike.
- Writes go through the format-preserving write-back (comments kept, the
  backup of the previous file in `backups/`), like `UpdateLayout`.

### Which theme is active

`nail_api::appearance::active_theme(settings, system, themes)` is the one
rule (the core and the UI mock share it): with Follow Windows on, a Windows
contrast theme picks `HIGH_CONTRAST_THEME` (when usable), else the OS
colour scheme picks the pair's dark or light theme (light when the OS
state is unknown); with it off, the fixed theme. A missing or unusable
choice falls back to `DEFAULT_LIGHT_THEME` / `DEFAULT_DARK_THEME` by kind.

The resolved theme the UI renders is: base ← the active theme's `extends`
chain ← the user's `theme.kdl` (overrides and part rules); `extends` /
`appearance` of the user file only select, they are not merged as tokens.
`ResolvedTheme` gains `density` (from the `appearance` node; serde default
comfortable). Hot reload stays: editing `theme.kdl` or a file in `themes/`
re-resolves and sends `ConfigChanged` (+ `ThemeChanged` when the list or
selection changed).

### Density

Comfortable / Compact applies to every theme through a **token variant**:
density-dependent tokens carry `compact=` (`space "row" 12 compact=8`;
`nail-config` accepts `compact` next to the colour modes, `css::MODES`).
It resolves into `ResolvedTheme.modes["compact"]` (`COMPACT_VARIANT`),
which the UI applies under `:root[data-nail-density="compact"]` after the
colour modes (the colour modes keep `data-nail-mode`). Part rules that
differ by density use a condition on the root part: the UI sets
`data-density` on the `app` part, so
`part "app[density=\"compact\"] message-list.row.tags" { display "none" }`
works with the existing selector machinery. A theme overriding a
density token's base value drops the inherited `compact=` unless it gives
its own (normal token merge semantics).

### Token schema (spacing, type scale, radius, elevation)

`nail_api::appearance` lists the tokens every theme can rely on
(`token_schema()`; `TokenSpec {category, name, value, density}`); the base
theme must define all of them and give a `compact=` variant where
`density` is set (`nail-config/tests/defaults.rs`
`the_base_theme_defines_the_token_schema`, also checked on every
built-in):

| Group | Tokens (base px; Compact) |
| --- | --- |
| Spacing, 4 px grid | `space.xxs` 2 (half step), `xs` 4, `sm` 8, `md` 12, `lg` 16, `xl` 24, `xxl` 32; semantic `space.pane` 16 (pane padding; 12), `gap` 8, `section` 24, `row` 12 (list-row padding; 8) |
| Type scale | `font-size.label` 11.5 (600, caps), `small` 12.5, `body` 14 (13), `heading` 16 (600), `title` 20 (18); `font-weight.regular` 400 / `medium` 500 / `semibold` 600; `line-height.tight` 1.25 / `normal` 1.45 / `relaxed` 1.65 (reader body) |
| Radius | `radius.sm` 6 (inputs, menu items), `md` 10 (buttons, cards; the override), `lg` 14 (windows), `pill` 999 |
| Elevation | `shadow.raised` (floating cards / popovers), `overlay` (menus), `window` (dialogs, tool windows); nothing else gets a shadow |
| Sizes | `size.icon` 16, `control` 32 (button / input height; 28), `avatar` 34 (list avatar; 28) |

Token names stay kebab-case starting with a letter (the KDL reader's rule),
so the grid uses t-shirt names rather than `space.1`. The pre-M5 names
(`font-size.xs`…`xl`, existing `space` / `radius` names) stay for user
themes; WP151 keeps them as aliases of the new scale. The contract stage
added the missing names to `defaults/theme.kdl` with today's values;
WP151 sets the final values (the table above) and colours.

### Icons (`nail_api::icons`)

- Every icon name in the layout model (`ActionDef.icon`, `ToolbarItem` /
  `MenuItem` `icon`), in plug-in manifests and in UI code is a **Lucide**
  name (kebab-case, canonical names — Lucide's deprecated aliases such as
  `trash-2` are avoided). The UI renders `lucide-react` (ISC, added to
  `ui/package.json`) at `size.icon` with `currentColor`.
- Resolution in the UI's `Icon`: the active theme's `icons` (custom SVG by
  name, so a theme can still replace or add any icon) → `lucide_name`
  (a name in `BUNDLED_ICONS`, or a pre-M5 Nail name from
  `LEGACY_ICON_NAMES`, e.g. `compose` → `square-pen`, `delete` → `trash`,
  `sync` → `refresh-cw`, so old user layouts and plug-ins keep their icons)
  → nothing.
- The UI bundles only `BUNDLED_ICONS` (static imports);
  `crates/nail-api/fixtures/icons.json` mirrors both tables for the UI's
  registry test (`nail-api/tests/fixtures.rs` keeps it current;
  `NAIL_UPDATE_SNAPSHOTS=1`). Adding an icon = add the name (sorted) there
  and to the UI registry.
- `defaults/layout.kdl` now names Lucide icons only (the fixtures test
  checks every default action icon is bundled); `mail.move` uses
  `folder-input`. The default theme's own `icons` section (custom SVGs
  named `reply`, `inbox`, …) is removed by WP155 once WP153's Lucide
  registry is in, so Lucide shows instead of the old glyphs.

### Protocol

- `ListThemes` → `Response::Themes {appearance}`; `SetTheme {theme_id}`
  (fixed theme: writes `extends`, Follow Windows off) and
  `SetThemePreferences {preferences: ThemePreferences}` (Follow Windows,
  pair, density, overrides — everything but the fixed theme) answer the
  same. Errors: `invalid_request` with `theme-problem-*` /
  `theme-overrides-issue-*`, `config` when `theme.kdl` is invalid.
- `Appearance {themes (built-ins first, then user themes by id), settings,
  active, system?}`.
- `Event::ThemeChanged {appearance}` on any change of the list, the
  selection or the OS state; `ConfigChanged` follows when the resolved
  theme (active theme or density) changed.
- Remote sessions: `ListThemes` = session; `SetTheme` /
  `SetThemePreferences` = admin (they write the desktop's `theme.kdl`, like
  `UpdateLayout`); `ThemeChanged` reaches every session.
- Until WP152 serves them, the core answers `unsupported` +
  `m5a-not-ready` (`nail-core/src/appearance.rs`); the UI mock
  (`features/appearance/mock.ts`) implements the rules already.

### Follow Windows

`nail_api::appearance::SystemAppearanceSource` (Rust only): `current() →
Option<SystemAppearance {scheme: ColorScheme light|dark, high_contrast}>`,
`watch(listener) → handle`, `unwatch(handle)`. The core takes it from
`ServiceOptions::system_appearance`; `None` = the platform default, which
WP152 implements on Windows (registry `HKCU\Software\Microsoft\Windows\
CurrentVersion\Themes\Personalize` `AppsUseLightTheme` watched with
`RegNotifyChangeKeyValue`, high contrast via `SystemParametersInfoW
(SPI_GETHIGHCONTRAST)`; `windows-registry` / `windows-sys` features are
declared in `nail-core`). The core owns the decision, so `nail serve` and
the desktop app agree and the UI only renders the resolved theme.
`ManualSystemAppearance` serves tests and seeded runs (WP155 adds a
demo-multi test control to flip it for the Follow Windows scenario).

### Strings

`locales/en/core-appearance.ftl` holds the contract ids (theme names,
kinds, sources, densities, colour schemes, override issues, theme problems,
`m5a-not-ready`; `nail-api/tests/catalog.rs` checks them);
`locales/en/appearance.ftl` is the UI's (WP153). Both embedded in
`nail-config` `l10n.rs`; German files exist and are empty.

### Slots (who edits what in M5a)

| Package | Owns |
| --- | --- |
| WP151 | `defaults/themes/` (the five theme files; the contract placeholders carry the proposal palettes), `defaults/theme.kdl` (token scales, density variants, aliases of the old names), `crates/nail-config/tests/theme_contrast.rs` (WCAG AA for text and UI parts, AAA for High contrast), and the regenerated `crates/nail-api/fixtures/default-theme.json` |
| WP152 | `crates/nail-config` (everything else: the `appearance` node, `extends` of theme ids, the themes directory, user themes, write-back of selection / overrides, watcher for `themes/`), `crates/nail-core/src/appearance.rs` (requests, the Windows source, `ThemeChanged`); small wiring edits elsewhere in `nail-core` (watcher, `Service` fields) are allowed — no other M5a package edits `nail-core` |
| WP153 | `ui/` (primitives on the new tokens, Lucide everywhere, the appearance feature `ui/src/features/appearance/`, density on the root, the listed fixes) and `locales/` |
| WP154 | `e2e/visual/` and `scripts/visual-review/` (new; READMEs only so far) |
| WP155 | integration: wiring, removing the default theme's `icons`, the demo test control, every scenario, `userDocs/en` (appearance, themes) |

Contract changes after this ADR go to WP155 as requests.

## Consequences

- One rule for the active theme lives in Rust and is mirrored by the mock,
  so UI tests and the core agree; the core decides, the UI renders.
- `theme.kdl` stays the single source of truth, including the selection; a
  user can still hand-edit everything and the editors of M5d write to the
  same file.
- Density via a token variant keeps the "every theme" promise without
  duplicating themes; a part condition on `app` covers structural
  differences such as the hidden tag line.
- Icons become a fixed, bundled set; arbitrary Lucide names outside it
  render nothing unless a theme defines them, which keeps the bundle small.
- `ResolvedTheme` gained a field and token variants gained `compact`;
  both are additive (serde defaults), so old fixtures and clients still
  parse.
