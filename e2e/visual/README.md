# e2e/visual — visual-regression harness (M5a WP154)

Playwright screenshots of eighteen screens — the main window (navigation rail
and mail view) with a message open, the reader's More menu (M5b), the composer,
the settings page, the calendar's week, month, year, agenda and free-time views,
the Contacts and Tasks views, the main window and the calendar at 1024 px (M5b
review; `Screen.viewport`), a context menu, an HTML newsletter (reader
column and remote-content banner), a message with attachment cards before and
after saving one, and a dialog — for every
built-in theme (Frost, Sage, Graphite, Nocturne, High contrast) × density
(Comfortable, Compact), compared with reviewed baselines. ADR 0106 (package),
contract ADR 0102.

## Running

Prerequisites, as for `npm run e2e:only`: `npm run build -w ui`,
`cargo build -p nail-cli` (or `NAIL_BIN=<path to nail.exe>`), and
`npx -w e2e playwright install chromium` once.

| Command                                                              | What it does                                                                        |
| -------------------------------------------------------------------- | ----------------------------------------------------------------------------------- |
| `node e2e/visual/run-visual.mjs` (`npm run visual`)                  | all variants against the baselines, then the review page                            |
| `node e2e/visual/run-visual.mjs --update` (`npm run visual:update`)  | also writes the baselines of new and changed screens (`--update-snapshots=changed`) |
| `node scripts/visual-review/generate.mjs` (`npm run visual:review`)  | rebuilds the review page from the last run                                          |
| `node e2e/visual/selftest.mjs`                                       | end-to-end self-test of the harness with temporary baselines                        |
| `node --test e2e/visual/scripts.test.mjs "e2e/visual/lib/*.test.ts"` | unit tests                                                                          |

Arguments after `--` go to Playwright, e.g.
`node e2e/visual/run-visual.mjs -- -g graphite`. Filters (comma-separated):
`NAIL_VISUAL_THEMES`, `NAIL_VISUAL_DENSITIES`, `NAIL_VISUAL_SCREENS`. More
workers: `NAIL_VISUAL_WORKERS` (default 2; each worker runs its own server).

The review page lands in `<build dir>/visual/review/index.html`
(`%LOCALAPPDATA%\nail-build\visual\review\index.html` locally,
`.nail-build/visual/review/` in CI): baseline, actual and difference side by
side per screen, filterable by status (changed, new, error, passed, captured,
skipped). Open the file in a browser; nothing is served.

## What a run does

1. **Theme list from the core.** While Playwright collects the tests, the
   config starts a throw-away `nail serve --seed demo-multi` and asks
   `ListThemes` (`lib/probe-themes.mjs`). The matrix (`lib/matrix.ts`) is the
   five built-ins in gallery order plus any further built-in the core lists,
   each in both densities; user themes are never captured. A theme the core
   does not list, lists with a problem, or cannot list at all (before
   WP152 the core answers `unsupported` / `m5a-not-ready`) becomes a skipped
   test whose reason says exactly that; `run-visual.mjs` prints the reasons.
2. **One server per worker** (`support.ts`): `nail serve --seed demo-multi`
   on a free port with temporary directories, plus three fixed events on the
   mock CalDAV server for the calendar screen. Each variant selects its theme
   (`SetTheme`) and density (`SetThemePreferences`, Follow Windows off, no
   overrides), checks the core's answer and that the UI shows the density
   (`data-nail-density` on the root).
3. **Deterministic screens** (`screens.ts`): each screen starts from a fresh
   page load with a fixed clock (`2026-09-30T10:00Z`; the calendar
   `2026-08-12T10:00Z`, a week before anything the demo data derives from the
   real date), time zone UTC, locale en-US, light colour scheme (the theme
   decides colours, not the OS), reduced motion, viewport 1280 × 1000 (M5b; 800 before) at scale
   1. It waits for the demo mail and settled account states, fonts
      (`document.fonts.ready`), parks the pointer, and captures with animations
      disabled and the caret hidden once two consecutive screenshots are
      identical.
4. **Comparison** with `baselines/<platform>/<theme>-<density>/<screen>.png`
   (Playwright's comparator; tolerance `NAIL_VISUAL_MAX_DIFF_RATIO`, default
   0.002 of the pixels, and `NAIL_VISUAL_THRESHOLD`, default 0.15 per
   pixel). Every screen is recorded in the manifest
   (`<build dir>/visual/manifest/`) with its status and images, so one broken
   screen never hides the others.
5. **Self-check** (`harness.visual.ts`): in the server's own theme, every
   screen opens, settles and captures twice from fresh page loads within the
   tolerance — a screen that fails here would make its baselines flaky. Shown
   on the review page as "current".

## Baselines

- Kept per platform (fonts differ): `baselines/win32/…` is the reference; CI
  and the integrator generate them on Windows. Only reviewed baselines are
  committed; everything else stays in the build directory.
- Generate or accept: `npm run visual:update`, open the review page, check
  every "new" / "changed" screen, then commit `e2e/visual/baselines/`.
- A missing baseline fails the run as "new"; baselines of screens a run no
  longer has are listed on the review page ("Baselines without a screen").
- The pseudo theme `current` (`NAIL_VISUAL_THEMES=current`, never in the
  default matrix) captures the server's own theme without switching; with
  `NAIL_VISUAL_BASELINES=<temp dir>` it compares looks before/after a change
  without touching the committed baselines (that is what `selftest.mjs`
  does).

## Changing the screens

The `open` functions in `screens.ts` find windows by layout window name
(`[data-part="window"][data-window=…]`) and controls by role and name. When
the shell changes (M5b: navigation rail, views), only they need to follow;
then regenerate and review the baselines.
