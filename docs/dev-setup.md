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
| Playwright end to end (builds UI, `nail` and the desktop app first) | `npm run e2e` |
| Playwright only (already built) | `npm run e2e:only` |
| The desktop app for the WebView2 smoke test | `npm run build:desktop` |
| Installer and release scripts (node:test) | `node --test "scripts/test/*.test.mjs"` |
| Licences, bans, sources, advisories | `cargo deny check` (advisories need the RustSec database from GitHub) |

CI (`.github/workflows/ci.yml`) runs these jobs:

| Job | Runner | What |
| --- | --- | --- |
| `build-test` | `windows-latest` | build, `cargo test --workspace`, clippy, fmt, Vitest, lint, the Playwright web tests, the script tests |
| `desktop` | `windows-latest` | `npm run build:desktop`, then the WebView2 smoke test with `NAIL_E2E_DESKTOP=1` |
| `installer` | `windows-latest` | `node scripts/build-installer.mjs`, the install → start → uninstall smoke test, uploads installer, portable zip, SBOMs and checksums |
| `deny` | `ubuntu-latest` | `cargo deny check` |
| `interop` | `ubuntu-latest` | the engines against real Dovecot and Stalwart (below) |

### Installer, portable zip and SBOMs (ADR 0040)

```sh
node scripts/build-installer.mjs [--skip-ui]      # NSIS installer, portable zip, SBOMs, SHA256SUMS → <build dir>/dist
powershell -File scripts/installer-smoke.ps1      # silent install to a temp dir, start, uninstall
node scripts/sbom.mjs                             # CycloneDX SBOMs of the Rust and npm dependencies
```

Nothing is written into the repository. The installer's NSIS template is a
copy of Tauri's (`app/src-tauri/installer/installer.nsi`); a test fails when
the Tauri CLI version changes, so re-copy it on upgrades.

### Interop tests (real servers, CI job `interop`)

`scripts/interop/` is its own Cargo workspace (copy the root `Cargo.lock`
into it first); see `scripts/interop/README.md`. It needs Docker, so it runs
in CI only; without `NAIL_INTEROP=1` the server tests return at once.

### Dependency rules

No GPL / LGPL / AGPL dependency, no OpenSSL and no aws-lc (TLS uses
`ring`; ADRs 0029 and 0036). `deny.toml` enforces the licence allowlist,
bans `openssl-sys` / `aws-lc-*` and allows only crates.io: run
`cargo deny check` after adding a crate (CI runs it too). It records the
RUSTSEC-2023-0071 exception (`rsa` 0.9, ADR 0034).

### End-to-end tests (Playwright)

The suite in `e2e/` drives the real web UI served by the real core: the
prebuilt `nail serve --seed demo` binary with temporary data and config
directories. It builds nothing itself (ADR 0010). `npm run e2e:only` needs,
first:

```sh
npm run build -w ui                      # ui/dist, served by nail serve
cargo build -p nail-cli                  # the nail binary (debug profile)
npm run build:desktop                    # nail-app for the desktop smoke test
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
- The desktop smoke test (`e2e/tests/desktop.spec.ts`) starts the real
  `nail-app.exe --seed demo` with
  `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=<free port>`
  set only in that child's environment, and attaches with
  `chromium.connectOverCDP` (`e2e/support/desktop.ts`). The app is built by
  `npm run build:desktop` (`e2e/scripts/build-desktop.mjs`: `nail-app` with
  `tauri/custom-protocol`, so it serves the embedded `ui/dist`) into its own
  target directory, `%LOCALAPPDATA%\nail-build\desktop-target`, so the
  feature does not force rebuilds of the shared target directory.
  `NAIL_APP_BIN` selects another binary. A seeded app keeps its data,
  config and WebView2 profile in a temporary directory. The test always
  runs locally on Windows; in CI it is skipped unless `NAIL_E2E_DESKTOP=1`,
  which the CI workflow sets after `npm run build:desktop` (ADR 0028).
- Traces, screenshots and the CI HTML report go to
  `%LOCALAPPDATA%\nail-build\e2e\` (`$TMPDIR/nail-build/e2e/` elsewhere;
  `NAIL_BUILD_DIR` overrides the base), not into the repository.

| File | Covers |
| --- | --- |
| `e2e/tests/mail.spec.ts` | Default three-pane layout from `data-part` attributes (no inline styles or classes), seeded folders and messages, sandboxed reader iframe with remote images blocked, hostile HTML neutralised, `j`/`k` navigation, German locale falling back to English |
| `e2e/tests/config.spec.ts` | `theme.kdl` hot reload of a CSS custom property (and keep-last-valid on errors), `layout.kdl` moving and removing panes, `--locale de` fallback |
| `e2e/tests/sync.spec.ts` | `--seed demo-imap`: an IMAP account synced from the mock server, `F5` (`sync.now`) round trip |
| `e2e/tests/accounts.spec.ts` | `--seed demo-multi`: unified inbox with two accounts and their status, adding an account through discovery (autoconfig mock), unverified discovery answers shown with a warning (`elsewhere.example` SRV mock), a POP3 account receiving mail, a virtual account merging folders and deduplicating a mailing-list message |
| `e2e/tests/mailops.spec.ts` | `--seed demo-multi`: flag / archive / delete reach the mock IMAP server, a server-side flag change arrives through IDLE, writes made offline replay after reconnecting |
| `e2e/tests/search-threads.spec.ts` | `--seed demo-multi`: the query language (fields, quotes, OR / NOT, error position), the thread view across folders |
| `e2e/tests/send.spec.ts` | `--seed demo-multi`: a plain-text mail cancelled by undo send, then sent to the mock SMTP server, with the Sent copy on the IMAP server; a Sent copy queued while IMAP is offline and filed once it is back |
| `e2e/tests/a11y.spec.ts` | axe-core scan of the main window, the reader and dark mode, and (`--seed demo-multi`) of the composer in each mode, the outbox, filter rules, account setup, every settings tab and the merge dialog; any violation, whatever its impact, fails |
| `e2e/tests/composer.spec.ts` | `--seed demo-multi`: HTML and Markdown mails (bold text, Markdown source part, attachment, core-rendered preview) with their MIME parts as the mock SMTP server received them; reply quoting in HTML (blockquote) and Markdown (`>`); drafts autosaved to the server's Drafts folder, continued from the outbox after a reload, removed after sending |
| `e2e/tests/rules.spec.ts` | `--seed demo-multi`: an arrival rule (entered in the text view) moves and flags new mail on the IMAP server and its run is undone as one unit; the test-rule preview with "why it matches"; a rule created from a message and written to `rules.kdl` |
| `e2e/tests/mcp.spec.ts` | an MCP client created in the settings is denied until "Read mail" is granted, then reads `nail://accounts` and finds messages with `search_messages` over Streamable HTTP (`e2e/support/mcp.ts`); the activity log shows both |
| `e2e/tests/customization.spec.ts` | the inspector (part path, theme rules, layout node with file positions), dragging a divider writes `layout.kdl`, the merge dialog applies a shared theme and reverts it (backup), the error list shows every error of two broken files and clears one when it is fixed |
| `e2e/tests/security.spec.ts` | `--seed demo-multi` (both accounts have an OpenPGP key and an S/MIME certificate of the test CA): a signed + encrypted OpenPGP mail from Work to Home with `Subject: ...` on the mock SMTP server, read in Home with a verified badge; a tampered signature shows as invalid; an S/MIME sign + encrypt round trip with a valid chain; Autocrypt turns encryption on after one message (`autocrypt-alice.eml`); the sender badge from a trusted `Authentication-Results` (`authserv-id "mx.nail.example"`) and a look-alike link warning; encryption at rest on and off with mail and search intact; an MCP client reads an encrypted message only with the "Encrypted messages" grant |
| `e2e/tests/desktop.spec.ts` | the desktop app over WebView2 CDP: demo mail and reading through Tauri IPC, search, the rich composer's core-rendered Markdown preview, the running MCP server; a second app started with a `mailto:` link opens the composer with its fields |
| `e2e/tests/remote.spec.ts` | the remote UI (turned on over the local WebSocket: TLS with a self-signed certificate, host name `localhost`) in a second browser context: pairing with a code from the desktop's dialog (fingerprint, used code refused, Trusted Types block an injected script), passkey registration and login with a CDP virtual authenticator, password + TOTP (a used code refused), password guessing slowed down (backoff, audit), session revoke on the desktop logs the browser out, internet mode locked in the settings and in the core, the local UI's CSP blocking an inline script (`e2e/support/remote.ts`: `rpc`, `enableRemoteUi`, `totp`, `trustingBrowser`) |
| `e2e/tests/remote-content.spec.ts` | `--seed demo-multi`: remote images blocked, then loaded through the proxy after "Always load from" the sender; a private target is refused (403), a forged token is a 404, a loopback literal is never signed |

### Generated files and snapshots

Some checked-in files are generated. A test fails when one is stale;
regenerate it with the matching command:

| File(s) | Regenerate with |
| --- | --- |
| `ui/src/api/generated/` (TypeScript types from `nail-api`) | `npm run gen:api` |
| `crates/nail-config/schema/*.schema.json` | `NAIL_UPDATE_SNAPSHOTS=1 cargo test -p nail-config --test schema` |
| `crates/nail-mime/tests/snapshots/` (insta) | `INSTA_UPDATE=always cargo test -p nail-mime --test corpus`, or `cargo insta review` |
| `crates/nail-api/fixtures/default-{theme,layout}.json` (after changing `defaults/*.kdl`) | `NAIL_UPDATE_SNAPSHOTS=1 cargo test -p nail-config --test defaults`, then `npx prettier --write crates/nail-api/fixtures` |
| `crates/nail-config/schema/rules.schema.json` | `NAIL_UPDATE_SNAPSHOTS=1 cargo test -p nail-config --test rules_schema` |
| `crates/nail-config/tests/golden/` (layout write-back, merge) | `NAIL_UPDATE_GOLDEN=1 cargo test -p nail-config --test write_back` (or `--test merge`) |
| `crates/nail-compose/tests/golden/*.eml` (built MIME, CRLF, kept byte for byte by `.gitattributes`) | `NAIL_UPDATE_GOLDEN=1 cargo test -p nail-compose --test mime` |
| `crates/nail-testkit/corpus/crypto/pgp/` (OpenPGP interop vectors made by GnuPG 2.4; Git for Windows ships it as `/usr/bin/gpg`) | `bash crates/nail-testkit/corpus/crypto/pgp/generate.sh` (random session keys: every file changes, the tests must still pass) |
| `crates/nail-testkit/corpus/crypto/smime/` (S/MIME test CA, certificates incl. the demo-multi `ada-work` / `ada-home`, PKCS#12 files password `nail-test`, OpenSSL-made messages) | `bash crates/nail-testkit/corpus/crypto/smime/generate.sh` (needs OpenSSL 3.4+, Git for Windows ships `/mingw64/bin/openssl`; OpenSSL is a fixture tool only) |

## Run

### Web mode (`nail serve`)

```sh
npm run build
cargo run -p nail-cli -- serve --seed demo --open
```

This serves the UI and API on `http://127.0.0.1:4780/`. Useful options:

| Option | Meaning |
| --- | --- |
| `--seed demo` | Load the demo mail corpus as a local account (no network). Every seeded run keeps secrets in memory, trusts only the corpus' S/MIME test root and looks up no keys online |
| `--seed demo-imap` | Serve the demo corpus from an in-process mock IMAP server and sync it like a real account |
| `--seed demo-multi` | Two configured accounts on in-process mock servers: "Work" (IMAP with IDLE, the demo corpus) and "Home" (POP3), SMTP for both, and `bob@new.example` (password `new-secret`) for trying account setup with discovery (`bob@elsewhere.example` shows an unverified SRV answer). Writes its `accounts.kdl` into the config dir (refusing one that holds real accounts), keeps passwords in memory (never the OS keychain) and exposes the test controls (below). Both accounts get an OpenPGP key (generated) and an S/MIME certificate (`ada-work.p12` / `ada-home.p12` of the corpus), `prefer-encrypt="mutual"` and `authserv-id "mx.nail.example"` |
| `--port 0` | Use a free port; the URL is the only line printed on stdout |
| `--data-dir DIR` | Store location (default `%LOCALAPPDATA%\Nail`; with `--seed`, a temporary directory) |
| `--config-dir DIR` | `theme.kdl` / `layout.kdl` location (default `%APPDATA%\Nail`; with `--seed`, a temporary directory) |
| `--ui-dir DIR` | UI bundle (default `$NAIL_UI_DIR`, `ui/` next to the binary, or the repository's `ui/dist`) |
| `--locale de` | UI language override |
| `--no-watch` | Do not watch the config files |
| `--mcp-port PORT` | Port of the MCP server on 127.0.0.1 (default 0: a free one, written to `<data-dir>/mcp-endpoint.json`) |
| `--no-mcp` | Do not start the MCP server |
| `-v`, `-vv` | More logging; `NAIL_LOG=nail_core=debug` sets the filter precisely |

A seeded run never uses your real profile: without `--data-dir` /
`--config-dir` it runs in a temporary directory that is removed on exit
(ADR 0021). To keep the demo data between runs, pass directories:

```sh
cargo run -p nail-cli -- serve --seed demo --data-dir "$TEMP/nail-data" --config-dir "$TEMP/nail-config"
```

The server binds to 127.0.0.1 only and has no authentication (ADR 0009):
it is the desktop's own UI. The **remote UI** is separate (ADR 0041): off
by default, turned on in Settings → Remote access (`UpdateRemoteUiSettings`),
it listens on its own port with TLS (a self-signed certificate kept in the
store, its key in the keychain) and requires a login (pairing code,
passkey, password + code). `nail serve` and the desktop app both run it
when enabled. Seeded runs never fetch remote content from the internet:
their proxy resolves no names (demo-multi: only its mock image host).
A `nail.exe` in the portable zip (`bin\nail.exe` next to the `portable`
marker's folder) defaults to the zip's `data` and `config` folders. `nail serve` never opens an editor on the machine (the browser
may be elsewhere): "open file at line" (`OpenConfigFile`) answers
`unsupported` there (`ServiceOptions::open_editor`, ADR 0028). The desktop
app opens one unless it is seeded.

#### MCP server and `nail mcp`

`nail serve` and the desktop app start the read-only MCP server (ADR 0025)
on `127.0.0.1` with the core as its data provider
(`Service::start_mcp_server`); `GetMcpServerInfo` reports its URL. Agents
with a stdio transport use the bridge:

```sh
NAIL_MCP_TOKEN=<token> nail mcp --client <name> [--url URL] [--data-dir DIR]
```

It finds the server through `<data-dir>/mcp-endpoint.json` (or `--url`),
checks that the token belongs to `--client`, and forwards stdio to the
loopback HTTP endpoint. Tokens come from Settings → MCP clients.

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
| `remote-content` | – | `{images, private, requests}`: base URLs of the made-up image hosts (`images.nail.example` → the mock HTTP server, `private.nail.example` → a private address the proxy refuses) and the paths the image host was asked for |

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
<config-dir>/rules.kdl         filter rules (M1b); see userDocs/en/rules.md
<config-dir>/backups/          earlier theme.kdl / layout.kdl versions from the merge dialog (newest 20)
<data-dir>/mcp-endpoint.json   URL of the running MCP server (no secret), for `nail mcp`
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

The app accepts `--data-dir`, `--config-dir`, `--seed demo` and `--locale`,
plus what Windows passes after `--` (a `mailto:` link, a `.eml` / `.ics` /
`.vcf` file; ADR 0041): the UI takes these launch requests from the core.
A second start of an unseeded app hands its targets to the running one.
With `--seed demo` and no directories it runs in a temporary directory
(data, config and the WebView2 profile), like `nail serve --seed`. It
starts the MCP server like `nail serve`.

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
- **0021:** M1a review fixes (UIDVALIDITY-safe replay, Sent copies via the queue, `ChooseIdentity`)

For M1b:

- **0022:** the M1b contract (rich drafts, rules, MCP, UI context, config write-back and merge,
  saved searches, `RetryOp`), store v4, the `nail-compose` / `nail-rules` / `nail-mcp` stubs
  and the per-package slots (`ui/src/features/README.md`)
- **0023:** the composer (`nail-compose`: MIME, Markdown, CSS inlining, quoting; TipTap / CodeMirror UI)
- **0024:** the rules engine (`rules.kdl` reader and format-preserving writer, planned actions, rules UI)
- **0025:** the read-only MCP server (Streamable HTTP on loopback, tokens, grants, audit, `nail mcp` bridge)
- **0026:** customization tooling (every config error, `layout.kdl` write-back, merge dialog, inspector)
- **0027:** M1b integration (composer, rules, MCP and customization in the core; server Drafts
  copy, `ListDrafts`; default layout / theme; desktop smoke test over WebView2 CDP)
- **0028:** M1b review fixes (rule mail marked against loops, redirects through the outbox,
  escaped reply templates, autosave upload races, restricted MCP search / context, partial undo)

For M2a:

- **0029:** the M2a contract (message security, keys and certificates, recipient key status,
  sign / encrypt options, auth results, reader warnings, encryption at rest, security settings),
  identity `crypto` in `accounts.kdl`, store v5, the `nail-pgp` / `nail-smime` / `nail-vault`
  stubs, the crypto test-vector folders and the `security` UI slot
- **0030:** OpenPGP and Autocrypt (`nail-pgp` on rPGP: keyring, PGP/MIME, inline read, header
  protection, Autocrypt Level 1, WKD / keys.openpgp.org)
- **0031:** S/MIME (`nail-smime` on RustCrypto `cms`: sign / encrypt / verify / decrypt, chain
  validation, PKCS#12, the Windows certificate store)
- **0032:** data protection (`nail-vault`: sealed columns, blobs and search index, the data key,
  the migration; `Authentication-Results`; link and attachment safety)
- **0033:** the security UI (key manager, identity settings, composer toggles, badges,
  warnings, encryption-at-rest settings, Autocrypt setup)
- **0034:** M2a integration (decrypt / verify on read, sign / encrypt on send, Autocrypt in and
  out, the vault switch with the swappable search index, MCP `encrypted` rule, demo keys, e2e)
- **0035:** M2a review fixes

For M2b:

- **0036:** the M2b contract (remote UI, remote content, store v6, `webauthn_rp`, slots)
- **0037:** remote-UI authentication (`nail-remote-auth`: passkeys, Argon2id + TOTP, pairing,
  sessions, rate limits, authorization, axum login route and middleware)
- **0038:** transport and browser hardening (`nail-server` TLS, certificates, ACME, headers,
  sandbox origin; `nail-proxy`)
- **0039:** the remote-UI and privacy UI
- **0040:** installer, portable build, cargo-deny, SBOMs, CI jobs
- **0041:** M2b integration (remote-UI host, `handle_remote`, remote content through the proxy,
  Trusted Types, sandbox rendering, launch requests, e2e)
