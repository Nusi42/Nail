# ADR 0103: WP151 — the five built-in themes, the base token scales and the contrast test

Status: accepted · 2026-10-03 · M5a WP151

## Context

ADR 0102 (M5a contract) fixed where the built-in themes live
(`defaults/themes/<id>.kdl`, each `extends="default"`), the token schema
(spacing, type scale, radius, elevation, sizes, `compact=` density
variants) and that the accent override restyles what derives from the
accent. WP151 delivers the palettes of the approved UX proposal (Frost,
Sage, Graphite, Nocturne, High contrast), the final base values in
`defaults/theme.kdl`, and an automated WCAG contrast check.

## Decisions

### Colour tokens

The pre-M5 colour names stay the API that the ~4000 lines of default part
rules (and user themes) use. The proposal's roles map onto them, and
five names are new:

| Proposal | Token | Use |
| --- | --- | --- |
| surface / surface-2 / surface-3 | `bg` / `bg-subtle` / `bg-raised` | panes; side panes, inputs, hovered rows; hovered controls, tags |
| bg | `canvas` (new) | the window behind the panes |
| rail | `rail` (new) | navigation rail |
| text / text-2 / text-3 | `fg` / `fg-muted` / `fg-subtle` (new) | primary, secondary, tertiary text |
| line | `border` | dividers, cards (decorative, not checked) |
| — | `border-strong` (new) | outlines of input controls, 3:1 on every surface; `border "control"` |
| accent / accent-ink / accent-soft | `accent` / `accent-fg` / `accent-soft` (new) | |
| sel | `selection` (+ `selection-fg`) | |
| good / warn / bad | `success` / `warning` / `danger` | `flagged` = `$color.danger` |

`unread` and `focus-ring` are `$color.accent`; `accent-soft` and
`selection` are `color-mix(in srgb, $color.accent N%, $color.bg)` with N
tuned per theme to land close to the proposal swatch (exact values are not
reachable by a mix, e.g. Frost `#e5ebff` vs `#e7ebfa`). So a user's
`color "accent"` restyles them all (ADR 0102). High contrast keeps fixed
values (cyan focus ring, solid yellow selection).

The theme files set every semantic colour themselves (so no colour of the
base palette leaks into a theme) and nothing else; High contrast also sets
`font-size "body" 16 compact=15`, 2 px borders, outline shadows, no motion
and its focus / outline part rules (it was the M3 example theme; it is
now a built-in). Each theme gives `print=` values: the print view is
white paper with black text in every theme, and dark themes print with
the light themes' accent and state colours.

### Contrast adjustments to the proposal palettes

The test requires AA text contrast for every text colour on every surface
it can meet, including the soft selection (secondary text and dates stay
as they are inside a selected row in the proposal). The proposal's swatches
fail some pairs, so these were adjusted (by mixing toward the theme's text
colour, keeping the hue):

- `fg-subtle` (text-3): Frost `#8794a5` → `#5b6775`, Sage `#8a978d` →
  `#5b675e`, Graphite `#6f7889` → `#99a0ad`, Nocturne `#66749a` → `#8c98b6`.
  At 4.5:1 on the selection, tertiary text ends up close to `fg-muted`;
  the hierarchy comes from size and weight, not from a fainter grey.
- Light themes' states: Frost success `#2f9e6b` → `#277355`, warning
  `#c47f17` → `#875e1e`, danger `#d6455d` → `#b03e53`; Sage accent
  `#2e7d5b` → `#2c7153` (also success), warning `#b7791f` → `#845d1e`,
  danger `#c24152` → `#ae3e4c`, fg-muted `#5a695e` → `#59685d`.
- Dark themes' accents and states pass unchanged.
- Base (`default`, used by user themes that extend it directly): new
  colours added, danger / success darkened in light, the dark selection at
  16 %; the colours other tests pin (`fg`, `accent`, `border`) are
  unchanged.

### Scales in `defaults/theme.kdl`

As in the ADR 0102 table. Every type step is `calc($font-size.body * x /
14)` (label 11.5, small 12.5, heading 16, title 20; Compact title
`calc(body * 18 / 13)` = 18), and the pre-M5 names `xs`…`xl` are aliases
(`$font-size.label` … `$font-size.title`), so the text-size override and
Compact density scale every existing rule. Radii: `md` 10, `sm` =
`calc(md * 0.6)`, `lg` = `calc(md * 1.4)`, `pill` 999. `space.row` is 12 /
Compact 8: the ADR 0102 value on the 4 px grid (the proposal mock used 7).
Elevation follows the proposal (`overlay` 0 10px 30px at 14 % in light).
`font.display` (Segoe UI Variable Display) is new. The reader body uses
the UI face for plain text too, `line-height.relaxed`, at most 68ch
(proposal); its size stays `$font-size.md` (= body), which an existing
nail-config test pins.

### The contrast test (`crates/nail-config/tests/theme_contrast.rs`)

- Resolves the base and each built-in through `nail-config` and evaluates
  colour tokens like the browser on `:root`: the mode's value (`dark=`
  etc.) or the base value, following `var(--nail-color-*)` and
  `color-mix(in srgb, …)` (anything else, e.g. translucent colours, is an
  error, never a guess).
- Modes: a theme's own mode, or light / dark / high-contrast for the base;
  plus the print view (paper must be white, text AA).
- Thresholds: text 4.5:1, UI parts 3:1 (SC 1.4.11), High contrast 7:1 for
  everything. Pairs: `fg`, `fg-muted`, `fg-subtle`, `accent`, `unread`,
  states on `bg` / `bg-subtle` / `bg-raised` (and on the soft
  selection); text on `canvas` / `rail`; `selection-fg` on `selection`;
  `accent-fg` on `accent` / `danger` / `warning`; `accent` and `fg` on
  `accent-soft`; `border-strong`, `focus-ring`, `flagged` as UI parts.
- High contrast's selection is a solid highlight: inside a selected item
  everything uses `selection-fg` (the default parts set `inherit`), so
  only that pair is checked on it, and the focus ring is checked against
  the surfaces it is drawn on.
- Keeping the table honest: every text colour a default part rule uses,
  and every text / background pair a rule declares together, must be in
  the table; the input-control rules must use `$border.control`; the four
  palette themes may only set colours.

## Consequences

- A new part rule with an unchecked colour combination, or a palette
  change below AA, fails `cargo test -p nail-config`.
- The UI (WP153) can use `canvas`, `rail`, `fg-subtle`, `accent-soft`,
  `border-strong` / `$border.control` and `font.display`.
- `crates/nail-api/fixtures/default-theme.json` was regenerated (ADR 0102
  slot table assigns it to WP151).
