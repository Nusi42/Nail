# End-to-end tests (Playwright)

The suite drives the real web UI served by the real core: the prebuilt
`nail serve --seed demo` binary with temporary data and config directories.
It builds nothing itself.

## Run

From the repository root:

```sh
npm run e2e          # builds ui/dist and nail-cli, then runs Playwright
npm run e2e:only     # runs Playwright against what is already built
```

`npm run e2e:only` needs, first:

```sh
npm run build -w ui          # ui/dist, served by nail serve
cargo build -p nail-cli      # the nail binary (debug profile)
npx -w e2e playwright install chromium   # once per machine
```

## How it works

- `playwright.config.ts` starts one shared server
  (`nail serve --seed demo --port 4791 --data-dir <tmp> --config-dir <tmp>`)
  for the read-only scenarios.
- Scenarios that edit `theme.kdl` / `layout.kdl`, use another locale or
  another seed start a private server on a free port
  (`startNail()` in `support/nail.ts`), so they never disturb each other.
- The binary is found through `cargo metadata` (the cargo target directory
  may live outside the repository, see `docs/dev-setup.md`). Set `NAIL_BIN`
  to use another binary, `NAIL_E2E_PORT` to move the shared server, and
  `NAIL_LOG` (e.g. `info`) to see server logs.

## Scenarios

| File                   | Covers                                                                                                                                                                                                                                                    |
| ---------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `tests/mail.spec.ts`   | Default three-pane layout from `data-part` attributes (no inline styles or classes), seeded folders and messages, sandboxed reader iframe with remote images blocked, hostile HTML neutralised, `j`/`k` navigation, German locale falling back to English |
| `tests/config.spec.ts` | `theme.kdl` hot reload of a CSS custom property (and keep-last-valid on errors), `layout.kdl` moving and removing panes, `--locale de` fallback                                                                                                           |
| `tests/sync.spec.ts`   | `--seed demo-imap`: an IMAP account synced from the mock server, `F5` (`sync.now`) round trip                                                                                                                                                             |
| `tests/a11y.spec.ts`   | axe-core scan (no serious or critical violations) of the main window, the reader and dark mode                                                                                                                                                            |
