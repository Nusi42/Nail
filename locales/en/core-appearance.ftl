# Nail — English texts of themes and appearance named by the contract or
# built in Rust (M5a; contract ADR 0102): built-in theme names, theme kinds
# and sources, densities, the OS colour scheme, override issues and theme
# problems.
#
# The ids named by the contract (nail_api::appearance) are pre-filled;
# nail-api/tests/catalog.rs checks they exist. WP152 adds the core's own
# texts below its heading; the UI's texts are in appearance.ftl (WP153).

## Not served yet (contract stage)

m5a-not-ready = Themes cannot be changed in this version of Nail yet.

## Built-in themes (nail_api::appearance::builtin_theme_label)

theme-name-frost = Frost
theme-name-sage = Sage
theme-name-graphite = Graphite
theme-name-nocturne = Nocturne
theme-name-high-contrast = High contrast

## Theme kinds and sources (ThemeKind, ThemeSource)

theme-kind-light = Light
theme-kind-dark = Dark
theme-kind-high-contrast = High contrast
theme-source-builtin = Built in
theme-source-user = Your theme

## Density (Density)

density-comfortable = Comfortable
density-compact = Compact

## The operating system's colour scheme (ColorScheme)

color-scheme-light = Light
color-scheme-dark = Dark

## Override issues (ThemeOverridesIssue; optional arguments $min, $max)

theme-overrides-issue-font-size-range = Choose a text size between { $min } and { $max } pixels.
theme-overrides-issue-radius-range = Choose a corner radius between { $min } and { $max } pixels.
theme-overrides-issue-accent-invalid = Enter the accent colour as a hex colour, for example #3b5bdb.

## Theme problems (ThemeProblem; optional argument $detail)

theme-problem-unknown = There is no such theme.
theme-problem-invalid = This theme file has an error: { $detail }
theme-problem-shadows-builtin = This theme has the same name as a built-in theme. Rename its file to use it.
theme-problem-extends-cycle = This theme extends itself through other themes.

## Core texts (WP152)
