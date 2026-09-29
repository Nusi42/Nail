# Developer setup

How to build, test and run Nail on a development machine. Windows 11 (x64)
is the primary platform. User-facing documentation lives in
[`userDocs/`](../userDocs/); this page is for people working on Nail.

## Prerequisites

| Tool | Version | Notes |
| --- | --- | --- |
| Rust | 1.97, `x86_64-pc-windows-msvc` | `rustup component add rustfmt clippy` |
| Visual Studio Build Tools | 2022, "Desktop development with C++" | MSVC linker; SQLite and zstd are compiled from source |
| Node.js / npm | 26 / 11 | npm workspaces (`ui`, `e2e`); **not** pnpm (ADR 0001) |
| WebView2 runtime | evergreen | Preinstalled on Windows 11; needed by the desktop app only |
| Playwright Chromium | matches `@playwright/test` | `npx -w e2e playwright install chromium`, once per machine |

Docker is not needed. Integration tests use the in-process mock servers from
`crates/nail-testkit` (IMAP) and `crates/nail-testkit-net` (SMTP, POP3,
OAuth2, HTTP, DNS; M1a).

### Keep build output out of OneDrive

The repository lives in a OneDrive folder. Cargo's `target/` directory must
not be synced, so create a local, gitignored `.cargo/config.toml`:

```toml
[build]
target-dir = "C:/Users/<you>/AppData/Local/nail-build/target"
```

When several cargo processes run in parallel (for example several agents),
give each its own `CARGO_TARGET_DIR`.

Playwright output also goes to `%LOCALAPPDATA%\nail-build\`. The one
exception is `ui/dist` (gitignored, about 1 MB), see ADR 0011.

## Install and build

```sh
npm ci                       # JS dependencies for ui/ and e2e/
npm run build                # ui/dist (the Tauri shell embeds it; nail serve serves it)
cargo build                  # all crates except the desktop shell
cargo build -p nail-app      # the desktop shell (needs ui/dist)
```

`cargo build` / `cargo test` without `--workspace` skip `app/src-tauri`
(`default-members`). Use `--workspace` after `npm run build` to include it.

## Test

| What | Command |
| --- | --- |
| Rust unit and integration tests | `cargo test --workspace` |
| Rust lints | `cargo clippy --workspace --all-targets -- -D warnings` |
| Rust formatting | `cargo fmt --all --check` |
| UI unit tests (Vitest), incl. the lint-rule self-test | `npm test` |
| ESLint, TypeScript, Prettier (ui and e2e) | `npm run lint` |
| Playwright end to end (builds UI and `nail` first) | `npm run e2e` |
| Playwright only (already built) | `npm run e2e:only` |

CI (`.github/workflows/ci.yml`, `windows-latest`) runs all of these.

### End-to-end tests (Playwright)

The suite in `e2e/` drives the real web UI served by the real core: the
prebuilt `nail serve --seed demo` binary with temporary data and config
directories. It builds nothing itself (ADR 0010). `npm run e2e:only` needs,
first:

```sh
npm run build -w ui                      # ui/dist, served by nail serve
cargo build -p nail-cli                  # the nail binary (debug profile)
npx -w e2e playwright install chromium   # once per machine
```

How it works:

- `e2e/playwright.config.ts` starts one shared server
  (`nail serve --seed demo --port <free port> --data-dir <tmp> --config-dir <tmp>`)
  for the read-only scenarios. The runner picks the free port once and
  hands it to the workers in `NAIL_E2E_PORT`; set it yourself to pin one.
- Scenarios that edit `theme.kdl` / `layout.kdl`, use another locale or
  another seed start a private server on port 0 (`startNail()` in
  `e2e/support/nail.ts`), so they never disturb each other.
- The M1a scenarios start their own `--seed demo-multi` server each: two
  configured accounts on in-process mock servers (see "Run" below). They
  drive the mocks through the server's test controls with `control()` /
  `serverMessages()` from `e2e/support/nail.ts`.
- The binary is found through `cargo metadata` (the target directory lives
  outside the repository). `NAIL_BIN` selects another binary, `NAIL_LOG`
  (e.g. `info`) shows server logs.
- Traces, screenshots and the CI HTML report go to
  `%LOCALAPPDATA%\nail-build\e2e\` (`$TMPDIR/nail-build/e2e/` elsewhere;
  `NAIL_BUILD_DIR` overrides the base), not into the repository.

| File | Covers |
| --- | --- |
| `e2e/tests/mail.spec.ts` | Default three-pane layout from `data-part` attributes (no inline styles or classes), seeded folders and messages, sandboxed reader iframe with remote images blocked, hostile HTML neutralised, `j`/`k` navigation, German locale falling back to English |
| `e2e/tests/config.spec.ts` | `theme.kdl` hot reload of a CSS custom property (and keep-last-valid on errors), `layout.kdl` moving and removing panes, `--locale de` fallback |
| `e2e/tests/sync.spec.ts` | `--seed demo-imap`: an IMAP account synced from the mock server, `F5` (`sync.now`) round trip |
| `e2e/tests/accounts.spec.ts` | `--seed demo-multi`: unified inbox with two accounts and their status, adding an account through discovery (autoconfig mock), a POP3 account receiving mail, a virtual account merging folders and deduplicating a mailing-list message |
| `e2e/tests/mailops.spec.ts` | `--seed demo-multi`: flag / archive / delete reach the mock IMAP server, a server-side flag change arrives through IDLE, writes made offline replay after reconnecting |
| `e2e/tests/search-threads.spec.ts` | `--seed demo-multi`: the query language (fields, quotes, OR / NOT, error position), the thread view across folders |
| `e2e/tests/send.spec.ts` | `--seed demo-multi`: a plain-text mail cancelled by undo send, then sent to the mock SMTP server, with the Sent copy on the IMAP server |
| `e2e/tests/a11y.spec.ts` | axe-core scan of the main window, the reader and dark mode; any violation, whatever its impact, fails |

### Generated files and snapshots

Some checked-in files are generated. A test fails when one is stale;
regenerate it with the matching command:

| File(s) | Regenerate with |
| --- | --- |
| `ui/src/api/generated/` (TypeScript types from `nail-api`) | `npm run gen:api` |
| `crates/nail-config/schema/*.schema.json` | `NAIL_UPDATE_SNAPSHOTS=1 cargo test -p nail-config --test schema` |
| `crates/nail-mime/tests/snapshots/` (insta) | `INSTA_UPDATE=always cargo test -p nail-mime --test corpus`, or `cargo insta review` |

## Run

### Web mode (`nail serve`)

```sh
npm run build
cargo run -p nail-cli -- serve --seed demo --open
```

This serves the UI and API on `http://127.0.0.1:4780/`. Useful options:

| Option | Meaning |
| --- | --- |
| `--seed demo` | Load the demo mail corpus as a local account (no network) |
| `--seed demo-imap` | Serve the demo corpus from an in-process mock IMAP server and sync it like a real account |
| `--seed demo-multi` | Two configured accounts on in-process mock servers: "Work" (IMAP with IDLE, the demo corpus) and "Home" (POP3), SMTP for both, and `bob@new.example` (password `new-secret`) for trying account setup with discovery. Writes `accounts.kdl` into the config dir if there is none, keeps passwords in memory (never the OS keychain) and exposes the test controls (below) |
| `--port 0` | Use a free port; the URL is the only line printed on stdout |
| `--data-dir DIR` | Store location (default `%LOCALAPPDATA%\Nail`) |
| `--config-dir DIR` | `theme.kdl` / `layout.kdl` location (default `%APPDATA%\Nail`) |
| `--ui-dir DIR` | UI bundle (default `$NAIL_UI_DIR`, `ui/` next to the binary, or the repository's `ui/dist`) |
| `--locale de` | UI language override |
| `--no-watch` | Do not watch the config files |
| `-v`, `-vv` | More logging; `NAIL_LOG=nail_core=debug` sets the filter precisely |

For throw-away runs, use temporary directories so the demo data does not
land in your real profile:

```sh
cargo run -p nail-cli -- serve --seed demo --data-dir "$TEMP/nail-data" --config-dir "$TEMP/nail-config"
```

The server binds to 127.0.0.1 only and has no authentication in M0
(ADR 0009).

#### Test controls (`--seed demo-multi` only)

With `--seed demo-multi` (and only then) the server answers
`POST /api/test/<command>` with a JSON body, to drive the mock servers
from tests or by hand (`curl -X POST http://127.0.0.1:4780/api/test/smtp-messages`):

| Command | Arguments | Effect |
| --- | --- | --- |
| `imap-deliver` | `account?`, `mailbox?`, `raw`, `flags?` | New message on the IMAP server (`account` `work` or `new`, mailbox `INBOX` by default) |
| `imap-flags` | `account?`, `mailbox?`, `subject`, `flags` | Replaces the flags of the first message whose subject contains `subject` |
| `imap-messages` | `account?`, `mailbox?` | Lists `{uid, subject, flags}` |
| `imap-mailboxes` | `account?` | Lists `{name, count}` |
| `imap-offline` | `account?`, `offline` | Simulates an outage (connections refused and dropped) |
| `pop3-deliver` | `raw`, `uid?` | New message in the POP3 maildrop |
| `pop3-state` | – | `{uids, deleted}` of the POP3 server |
| `smtp-messages` | – | What the SMTP server received: `{from, to, data}` |

For UI work with hot module reloading, run `npm run dev` (Vite on
`http://localhost:5173`). Add `?mock=1` to use the in-memory mock core. Add
`?lang=de` or `?mode=dark` to override the locale or the colour mode.

### Data and config directories

```
<data-dir>/nail.sqlite3        metadata (SQLite, WAL; plus -wal/-shm files)
<data-dir>/blobs/ab/<62 hex>   raw messages, zstd-compressed, addressed by SHA-256
<config-dir>/theme.kdl         optional; see userDocs/en/theme-reference.md
<config-dir>/layout.kdl        optional; see userDocs/en/layout-reference.md
<config-dir>/accounts.kdl      accounts, identities, virtual accounts (M1a); see userDocs/en/accounts-reference.md
```

`nail config schema theme`, `nail config schema layout` and
`nail config schema accounts` print the JSON Schemas of the config files.
Secrets are never in these files: `accounts.kdl` only holds keychain
references (ADR 0013).

### Desktop app

```sh
npm run tauri dev
```

This starts Vite and the desktop app. The app loads the UI from the dev
server and uses the default directories. To run the built UI without a dev
server, with demo data in temporary directories:

```sh
npm run build
cargo run -p nail-app --features tauri/custom-protocol -- --seed demo --data-dir "$TEMP/nail-app-data" --config-dir "$TEMP/nail-app-config"
```

The app accepts `--data-dir`, `--config-dir`, `--seed demo` and `--locale`.

To inspect the WebView2 window with Chrome DevTools or Playwright, start it
with `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9333`
and connect to `http://127.0.0.1:9333`, for example with Playwright's
`chromium.connectOverCDP`.

## Layout of the code

See [`CLAUDE.md`](../CLAUDE.md) for the crate map, [`plan.md`](plan.md) for
milestones and [`adr/`](adr/) for decisions. The main ADRs for the M0
architecture are:

- **0002:** the protocol
- **0003:** the config models
- **0006:** KDL parsing
- **0007:** the store
- **0008:** the renderer
- **0009:** the core, server, CLI and desktop shell
- **0010:** the e2e harness
- **0011:** build output locations (`ui/dist` stays in the repository)

For M1a:

- **0012:** the M1a contract (protocol additions, engine traits, crate shapes)
- **0013:** `accounts.kdl` (format, validation, format-preserving writes)
- **0014:** libraries for SMTP, POP3, OAuth2, keychain, discovery and search
- **0015:** store v2, threading, drafts / outbox, sync-side merge
- **0016:** search index and query language
- **0017:** SMTP, POP3, OAuth2, keychain, discovery, mock network servers
- **0018:** the IMAP engine (own client on `imap-proto`, IDLE, QRESYNC, writes)
- **0019:** the M1a user interface (windows, forms, optimistic writes, mock core)
- **0020:** core integration (engines, replay, outbox, virtual accounts, demo seed)
