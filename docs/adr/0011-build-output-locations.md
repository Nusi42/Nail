# ADR 0011: Build output locations

Status: accepted · 2026-09-29 · M0 review fixes

## Context

The repository lives in a OneDrive folder. The ground rule is that build
output never goes into the repository, because OneDrive would sync it on
every build. Three tools write output:

- cargo (`target/`, several GB),
- Playwright (`test-results/` with traces and screenshots, the HTML report),
- Vite (`ui/dist`, the UI bundle, about 1 MB).

The plan also says that `nail serve` serves `ui/dist` and the Tauri shell
embeds it.

## Decisions

- **cargo:** a local, gitignored `.cargo/config.toml` sets `target-dir` to
  `%LOCALAPPDATA%\nail-build\target` (see `docs/dev-setup.md`).
- **Playwright:** `outputDir` and the HTML report go to
  `%LOCALAPPDATA%\nail-build\e2e\` (`$TMPDIR/nail-build/e2e/` on other
  systems; `NAIL_BUILD_DIR` overrides the base). See ADR 0010.
- **`ui/dist` stays in the repository, as a deliberate exception.**
  `app/src-tauri/tauri.conf.json` names the bundle with a path relative to
  the config file (`frontendDist`) and cannot read environment variables,
  so a per-user location outside the repository would need a generated
  Tauri config. `nail serve` finds the bundle next to the repository for
  development builds. The bundle is small, rebuilt only by
  `npm run build`, and gitignored.

## Consequences

- OneDrive syncs about 1 MB after each UI build. That is accepted.
- If the bundle grows or sync becomes a problem, move it with a generated
  Tauri config (`tauri build --config`) and `NAIL_UI_DIR` for `nail serve`.
