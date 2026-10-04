# scripts/visual-review — baseline review page (M5a WP154)

`generate.mjs` builds a static, self-contained review page from a run of the
visual-regression harness (`e2e/visual/`, ADR 0106): baseline, actual and
difference side by side for every screen × theme × density, a summary per
status, filters, the skipped variants with their reasons and baselines no
screen uses any more. A human accepts the changes with
`npm run visual:update` or fixes the UI.

```text
node scripts/visual-review/generate.mjs [--results <dir>] [--out <dir>] [--baselines <dir>]
```

- `--results`: the harness output (default `<build dir>/visual`, i.e.
  `NAIL_VISUAL_RESULTS`, else `NAIL_BUILD_DIR` or `%LOCALAPPDATA%\nail-build`);
  the manifest is read from `<results>/manifest/*.json`.
- `--out`: where the page goes (default `<results>/review`): `index.html`,
  the images it shows (copied into `images/`, so the page is a complete
  artifact) and `summary.json`.
- `--baselines`: the baselines of this platform (default
  `e2e/visual/baselines/<platform>`, or `NAIL_VISUAL_BASELINES/<platform>`),
  for the "baselines without a screen" list.

`e2e/visual/run-visual.mjs` calls it after every run. Output goes to the
build directory, never into the repository. Nothing is served (open the file
in a browser), so no port is bound. Tests:
`node --test "scripts/visual-review/*.test.mjs"`.
