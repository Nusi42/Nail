# ADR 0010: Playwright harness against the prebuilt `nail serve`

Status: accepted · 2026-09-29 · M0 WP6

## Context

The plan asks for Playwright scenarios against `nail serve --seed demo`
(web mode) and for every user-visible behaviour to have one. Some scenarios
change `theme.kdl` / `layout.kdl` or start the core with other options.
Those changes must not leak into other scenarios, which run in parallel.

## Decisions

- **The harness never builds anything.** It runs the `nail` binary from
  cargo's debug target directory and uses whatever `ui/dist` is present.
  The target directory is found with `cargo metadata`, because it may live
  outside the repository (`.cargo/config.toml`). `NAIL_BIN` overrides the
  binary. The root script `npm run e2e` builds `ui/dist` and `nail-cli`
  first; `npm run e2e:only` skips the build, and CI uses it after its own
  build steps. Building inside Playwright's `webServer` would hide
  compilation time in a start-up timeout and duplicate the CI build.
- **One shared server plus private servers.** Playwright's `webServer`
  starts one `nail serve --seed demo` on a free port, with temporary data
  and config directories, for read-only scenarios. The runner picks the
  port once (a child process binds port 0, because the config is evaluated
  synchronously) and passes it to the workers in `NAIL_E2E_PORT`, as it
  does for the directories; setting `NAIL_E2E_PORT` pins a port. A
  scenario that edits configuration files, needs `--locale` or needs
  `--seed demo-imap` starts its own server on port 0 (`support/nail.ts`).
  It reads the URL from the server's first stdout line and deletes the
  server's directories afterwards.
- **What the scenarios assert.** They use only the stable contract:
  `data-part`, `data-node-id`, `data-state`, `data-role` and ARIA roles
  and names, never generated class names or CSS. One scenario checks that
  no element inside the app has a `style` or `class` attribute. This
  enforces "everything is themeable" at runtime as well as through lint.
- **The reader** is checked inside its `sandbox=""` iframe with
  `frameLocator`: no remote `img` sources, no scripts, forms, frames or
  `on*` attributes.
- **Accessibility:** `@axe-core/playwright` scans the main window, the
  reader and dark mode. Any violation fails the scenario, whatever its
  impact. The app root is a `main` landmark, so layout plumbing such as
  split dividers is inside a landmark (axe rule `region`).
- **Output outside the repository:** traces, screenshots and the HTML
  report go to `%LOCALAPPDATA%\nail-build\e2e\` (ADR 0011).
- **Chromium only** for now, matching the WebView2 engine of the desktop
  app. Playwright against the packaged app over CDP comes later (design,
  "Testing & quality"). A manual CDP smoke check is described in
  `docs/dev-setup.md`.

## Consequences

- `cargo build -p nail-cli` and `npm run build -w ui` must be current before
  `npm run e2e:only`. A stale binary tests old behaviour.
- Each private server costs about 0.3 s to start. Scenarios that need
  isolation stay few and focused.
