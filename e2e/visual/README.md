# e2e/visual — visual-regression harness (M5a WP154)

Slot of WP154 (contract ADR 0102): Playwright screenshots of the main
window, composer, settings, calendar, menus and dialogs for the five
built-in themes (Frost, Sage, Graphite, Nocturne, High contrast) × two
densities (Comfortable, Compact), with checked-in baselines and a review
page (`scripts/visual-review/`).

Not part of `e2e/tests/` (the default `npm run e2e` suite) unless WP154
decides otherwise; like `e2e/perf/`, it may bring its own Playwright config.
Screenshots of a run and the review page go to the build directory
(`%LOCALAPPDATA%\nail-build\…`), never into the repository; only the
reviewed baselines are checked in.

Themes are switched through the core (`SetTheme` / `SetThemePreferences`,
ADR 0102) or the `?mock=1` transport (`features/appearance/mock.ts`).
