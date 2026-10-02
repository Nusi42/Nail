# Developer setup

How to build, test and run Nail on a development machine. Windows 11 (x64)
is the primary platform. User-facing documentation lives in
[`userDocs/`](../userDocs/); this page is for people working on Nail.

## Prerequisites

| Tool | Version | Notes |
| --- | --- | --- |
| Rust | 1.97, `x86_64-pc-windows-msvc` | `rustup component add rustfmt clippy` |
| Visual Studio Build Tools | 2022, "Desktop development with C++" | MSVC linker; SQLite and zstd are compiled from source |
| Node.js / npm | 26 / 11 | npm workspaces (`ui`, `e2e`, `packages/*`); **not** pnpm (ADR 0001) |
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
npm ci                       # JS dependencies for ui/, e2e/ and packages/*
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
| Socket-bind check (ADR 0057), then UI unit tests (Vitest) incl. the lint-rule self-test, then the `test` scripts of `packages/*` | `npm test` |
| Socket-bind check only | `npm run check:sockets` |
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
The servers are Dovecot and Stalwart (mail, ManageSieve) and, since M3a,
**Radicale** on port 5232 over TLS for CalDAV / CardDAV
(`NAIL_INTEROP_DAV=radicale=https://localhost:5232/`; `start-servers.sh`
starts `tomsquest/docker-radicale`, `NAIL_INTEROP_RADICALE=0` skips it;
ADR 0060). The DAV checks of `tests/dav.rs` also run locally without
Docker against the in-process mock (`MockDavServer` over TLS), and can be
pointed at a Radicale installed from PyPI (see the interop README).

**Owner-run checks (M3 human gate).** Microsoft and Google services cannot
run in CI; the automated suite covers them with recorded-style fixtures
and the mock servers (`MockGraphServer`, `MockEwsServer`,
`MockAutodiscoverServer`, `MockGmailServer`). Before the M3 gate the owner
runs, by hand:

- **Exchange Online** (a Microsoft 365 test tenant): add the account
  through Autodiscover (Graph, Microsoft sign-in with the app's client id),
  then the `exchange.spec.ts` checklist by hand — mail sync / flag / move /
  categories / send, a shared mailbox, automatic replies, a Nail rule
  placed as an inbox rule, the room finder (needs `Place.Read.All` admin
  consent), a meeting answered, a To Do task. Also confirm the behaviours
  listed as owner checks in ADR 0073 (nested folders in `mailFolders/delta`,
  `cancelledOccurrences`, `If-Match` change keys, MIME creates outside
  Drafts) and the EWS retirement date (ADR 0075).
- **Exchange Server 2019 / SE**: the same with EWS — Windows sign-in as the
  domain user (Kerberos), with explicit credentials and with NTLM
  (`cargo test -p nail-exchange --test sspi_live -- --ignored` with
  `NAIL_EWS_LIVE_URL`, optionally `NAIL_EWS_LIVE_PACKAGE=ntlm`), streaming
  notifications, the `CategoryList`, `ResolveNames` (ADR 0074).
- **Google**: a test account with the restricted Gmail scope on the Google
  OAuth client: the `gmail.spec.ts` checklist by hand, Google CalDAV /
  CardDAV with the calendar and contacts scopes.

### Dependency rules

No GPL / LGPL / AGPL dependency, no OpenSSL and no aws-lc (TLS uses
`ring`; ADRs 0029 and 0036). `deny.toml` enforces the licence allowlist,
bans `openssl-sys` / `aws-lc-*` and allows only crates.io: run
`cargo deny check` after adding a crate (CI runs it too). It records the
RUSTSEC-2023-0071 exception (`rsa` 0.9, ADR 0034).

### Sockets and the Windows firewall (ADR 0057)

Nail and its tests never open a socket on a non-loopback or unspecified
address, so Windows Defender Firewall never asks to "allow this app". The
one exception is the remote UI when the user enables it on a non-loopback
address. DNS lookups go through the OS resolver (`DnsQuery_W`), the mock
DNS server is asked from `127.0.0.1`, and every mock server and test
listener binds `127.0.0.1`. `npm test` runs `scripts/check-socket-binds.mjs`,
which fails on new binds to `0.0.0.0` / `[::]` (allowlist with reasons in
`scripts/lib/socket-binds.mjs`).

Builds before this rule left firewall rules for hashed test binaries
(`discover-<hash>.exe`, `engines-<hash>.exe`) and `nail.exe` under
`%LOCALAPPDATA%\nail-build`. To remove those inbound rules, run in an
**elevated** PowerShell as the same user (it only touches rules whose
program lies under that folder):

```powershell
Get-NetFirewallApplicationFilter | Where-Object { $_.Program -like "$env:LOCALAPPDATA\nail-build\*" } | Get-NetFirewallRule | Where-Object Direction -eq Inbound | Remove-NetFirewallRule
```

Leave out `| Remove-NetFirewallRule` first to see what would go.

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
  (`nail serve --seed demo --port <free port> --data-dir <tmp> --config-dir <tmp> --ui-dir <repo>/ui/dist`)
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
  (e.g. `info`) shows server logs. Every server gets `--ui-dir
  <repo>/ui/dist` explicitly: building `nail-app` copies Tauri's resources
  into `target/debug/ui`, which `nail serve` would otherwise prefer (a
  stale bundle after a UI rebuild).
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
- The global setup (`e2e/support/global-setup.ts`) builds the "Follow up"
  example plug-in once (esbuild + `@nail/plugin-api/tools`' `packPlugin`,
  `e2e/support/plugins.ts`) into `%LOCALAPPDATA%\nail-build\e2e\plugins\`
  and hands the package path to the workers (`NAIL_E2E_FOLLOW_UP`).
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
| `e2e/tests/a11y.spec.ts` | axe-core scan of the main window, the reader and dark mode, and (`--seed demo-multi`) of the composer in each mode, the outbox, filter rules, account setup, the snoozed list, the snooze picker, a folder's context menu and sync dialog, every settings tab (incl. Server rules, Vacation reply, Junk mail, Notifications, Synchronization and the Customize editors) and the merge dialog; any violation, whatever its impact, fails |
| `e2e/tests/composer.spec.ts` | `--seed demo-multi`: HTML and Markdown mails (bold text, Markdown source part, attachment, core-rendered preview) with their MIME parts as the mock SMTP server received them; reply quoting in HTML (blockquote) and Markdown (`>`); drafts autosaved to the server's Drafts folder, continued from the outbox after a reload, removed after sending |
| `e2e/tests/rules.spec.ts` | `--seed demo-multi`: an arrival rule (entered in the text view) moves and flags new mail on the IMAP server and its run is undone as one unit; the test-rule preview with "why it matches"; a rule created from a message and written to `rules.kdl` |
| `e2e/tests/mcp.spec.ts` | an MCP client created in the settings is denied until "Read mail" is granted, then reads `nail://accounts` and finds messages with `search_messages` over Streamable HTTP (`e2e/support/mcp.ts`); the activity log shows both |
| `e2e/tests/customization.spec.ts` | the inspector (part path, theme rules, layout node with file positions), dragging a divider writes `layout.kdl`, the merge dialog applies a shared theme and reverts it (backup), the error list shows every error of two broken files and clears one when it is fixed |
| `e2e/tests/security.spec.ts` | `--seed demo-multi` (both accounts have an OpenPGP key and an S/MIME certificate of the test CA): a signed + encrypted OpenPGP mail from Work to Home with `Subject: ...` on the mock SMTP server, read in Home with a verified badge; a tampered signature shows as invalid; an S/MIME sign + encrypt round trip with a valid chain; Autocrypt turns encryption on after one message (`autocrypt-alice.eml`); the sender badge from a trusted `Authentication-Results` (`authserv-id "mx.nail.example"`) and a look-alike link warning; encryption at rest on and off with mail and search intact; an MCP client reads an encrypted message only with the "Encrypted messages" grant |
| `e2e/tests/desktop.spec.ts` | the desktop app over WebView2 CDP: demo mail and reading through Tauri IPC, search, the rich composer's core-rendered Markdown preview, the running MCP server; a second app started with a `mailto:` link opens the composer with its fields |
| `e2e/tests/remote.spec.ts` | the remote UI (turned on over the local WebSocket: TLS with a self-signed certificate, host name `localhost`) in a second browser context: pairing with a code from the desktop's dialog (fingerprint, used code refused, Trusted Types block an injected script), passkey registration and login with a CDP virtual authenticator, password + TOTP (a used code refused), password guessing slowed down (backoff, audit), session revoke on the desktop logs the browser out, internet mode locked in the settings and in the core, the local UI's CSP blocking an inline script (`e2e/support/remote.ts`: `rpc`, `enableRemoteUi`, `totp`, `trustingBrowser`) |
| `e2e/tests/remote-content.spec.ts` | `--seed demo-multi`: remote images blocked, then loaded through the proxy after "Always load from" the sender; a private target is refused (403), a forged token is a 404, a loopback literal is never signed; "Load once" loads without adding an allow-list entry |
| `e2e/tests/plugins.spec.ts` | M2d plug-ins (private `--seed demo` each): the example installed through Settings → Plug-ins with its permissions, placed (toolbar item after Forward and Shift+F in `layout.kdl`), run from the reader toolbar (dialog, stored reminder, notification; axe on dialog and toast); a plug-in without `mail.read.body` refused a body (toast, log line); a runaway plug-in stopped and flagged while another message opens; `nail plugin dev` loading a folder, its console and a hot reload |
| `e2e/tests/a11y-windows.spec.ts` | axe on every layout window, settings tab (nested ones too) and feature dialog (incl. the plug-in install dialog) in default, high-contrast and dark mode against `--seed demo-multi`; checks its own coverage against the layout |
| `e2e/tests/pim.spec.ts` | `--seed demo-multi` (M3a): a weekly event edited "this and following" (two series); an event created in the CalDAV calendar reaches the mock CalDAV server and another client's event (`dav-put`) appears after "Sync"; the ICS subscription's holiday is read-only and a second feed is added from the sources dialog; a reminder fires at the controlled clock time (`clock-set`) and is dismissed; a contact created in the CardDAV book reaches the server and is suggested in the composer; duplicate contacts (local + CardDAV) merged into one linked person; a task with a subtask completed; an `.ics` given with `--launch` previewed and imported; an MCP client lists events only after a grant |
| `e2e/tests/scheduling.spec.ts` | `--seed demo-multi` (M3b): an iMIP invitation (delivered with `imap-deliver`) is added on arrival and accepted from the reader's card, the `REPLY` reaches the mock SMTP server and the event is in the calendar; an organizer's update moves the event and the cancellation is removed with "Remove from calendar"; a meeting created in the event editor is sent ("Send invitations"), Grace's `COUNTER` is accepted from the card and the updated `REQUEST` goes out; the attendee picker shows `bob`'s busy blocks from the mock CalDAV server's outbox |
| `e2e/tests/gmail.spec.ts` | `--seed demo-multi` (M3b): the Gmail account switched on in the settings syncs (four inbox conversations, chat skipped), category tabs filter the inbox, a label applied with "Labels…" reaches the mock Gmail server, archive removes `INBOX`, mail is sent through the API (`gmail-sent`), a message delivered on the server (`gmail-deliver`) arrives by history polling |
| `e2e/tests/exchange.spec.ts` | `--seed demo-multi` (M3c): an Exchange Online account added through Autodiscover (mock Autodiscover → Graph, the redirect confirmed, the mock Microsoft sign-in) syncs; an on-premises account added through Autodiscover (EWS, password) syncs and gets a server-side message by streaming notifications (`ews-deliver`); the "Contoso" Graph account switched on: Focused / Other tabs, flag, move into a subfolder, a new category and a category set on a message reach the mock Graph tenant (`graph-messages`, `graph-settings`), mail sent through Graph (`graph-sent`); a shared mailbox opened (its folder tree) and automatic replies set through Settings → Vacation reply and read back in the Exchange editor; a rule created from a message placed as an inbox rule; a To Do task created, a room booked through the room finder and an invitation answered through Graph's meeting API (`graph-pim`, `graph-meetings`, no mail sent) |
| `e2e/tests/m2c.spec.ts` | `--seed demo-multi` (M2c): a rule placed "Server" with its Sieve script and the vacation reply on the mock ManageSieve server; "Not junk" / "Mark as junk" train the filter and the next similar message is filed in Junk; a snoozed message leaves the list and returns when the core's clock passes the wake time; a new-mail notification (recorded Web Notification) and quiet hours suppressing the next; the server search finds a message of the headers-only folder and opening it fetches the body; the right-click menu (and Shift+F10); a key recorded in Settings → Customize → Keys is written to `layout.kdl` and works |

### Generated files and snapshots

Some checked-in files are generated. A test fails when one is stale;
regenerate it with the matching command:

| File(s) | Regenerate with |
| --- | --- |
| `ui/src/api/generated/` (TypeScript types from `nail-api`) | `npm run gen:api` |
| `packages/plugin-api/src/generated/` (plug-in-facing TypeScript types from `nail-api`, M2d) | `npm run gen:api` |
| Reference sections of `userDocs/en/plugins/api-reference.md` and `components.md` (from the SDK types and `ui.rs` doc comments) | `npm run docs -w packages/plugin-api` |
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
| `--seed demo-multi` | Two configured accounts on in-process mock servers: "Work" (IMAP with IDLE, the demo corpus, a Junk folder, a folder `Newsletters` synced headers only, and a mock ManageSieve server whose `sieve` node starts switched off) and "Home" (POP3), SMTP for both, and `bob@new.example` (password `new-secret`) for trying account setup with discovery (`bob@elsewhere.example` shows an unverified SRV answer). Writes its `accounts.kdl` into the config dir (refusing one that holds real accounts), keeps passwords in memory (never the OS keychain) and exposes the test controls (below). Both accounts get an OpenPGP key (generated) and an S/MIME certificate (`ada-work.p12` / `ada-home.p12` of the corpus), `prefer-encrypt="mutual"` and `authserv-id "mx.nail.example"`. M3a: a mock CalDAV / CardDAV server (sources `work-calendars` / `work-contacts`, user `ada`, password `dav-secret` in the memory keychain) with sample events ("Team sync" weekly, "Quarterly review" tomorrow) and contacts (Grace Hopper, Charles Babbage), and the ICS subscription `holidays` on the mock HTTP server. M3b: the mock CalDAV server's user address is `ada@work.example`, a second principal `bob` (`bob@example.org`, busy tomorrow 10:00 and 14:00 UTC) answers free/busy through the schedule outbox, "Project kickoff" (in two days) is a meeting the work identity organizes with bob invited; a Gmail API account "Gmail" (`ada@gmail.example`, mock Gmail server with the demo mailbox, signed in through a mock OAuth2 server, polls every 10 s) is configured but **switched off** (`enabled=#false`) so the other suites keep their counts — Settings → Accounts → Gmail → "Synchronize this account" turns it on |
| `--port 0` | Use a free port; the URL is the only line printed on stdout |
| `--data-dir DIR` | Store location (default `%LOCALAPPDATA%\Nail`; with `--seed`, a temporary directory) |
| `--config-dir DIR` | `theme.kdl` / `layout.kdl` location (default `%APPDATA%\Nail`; with `--seed`, a temporary directory) |
| `--ui-dir DIR` | UI bundle (default `$NAIL_UI_DIR`, `ui/` next to the binary, or the repository's `ui/dist`) |
| `--locale de` | UI language override |
| `--no-watch` | Do not watch the config files |
| `--mcp-port PORT` | Port of the MCP server on 127.0.0.1 (default: the MCP settings' fixed port, else a free one; written to `<data-dir>/mcp-endpoint.json`) |
| `--no-mcp` | Do not start the MCP server |
| `--launch FILE_OR_LINK` | Open a file or link as the desktop app does when Windows starts it (`.ics` / `.vcf` → import dialog, `.eml` → reader, `mailto:` → composer, `webcal:` → new subscription); repeatable (M3a) |
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
loopback HTTP endpoint. Tokens come from Settings → MCP clients. When no
Nail runs on the data directory, `nail mcp` starts one without a window
(`--config-dir`, `--mcp-port`; `--no-start` refuses instead) for as long as
the client is connected. One process per data directory: `nail serve`,
the desktop app and a headless `nail mcp` take `nail.lock`
(`DataDirLock`); the desktop app shows who holds it and exits.

#### Plug-ins and `nail plugin dev`

Plug-ins run inside the core (ADR 0055): installed packages are kept in
`<data-dir>/plugins/<id>.nailplugin` and verified by hash at every start.
To develop one:

```sh
node packages/create-nail-plugin/index.mjs <dir> [--id ID] [--name NAME] [--sdk PATH]
npm run package --prefix examples/follow-up    # build + pack the example (examples/follow-up/dist, gitignored)
nail plugin dev <folder> [--url URL | --data-dir DIR]
```

`nail serve` writes `<data-dir>/ui-endpoint.json` with its URL;
`nail plugin dev` finds it there (or takes `--url`), loads the folder with
every declared permission over the local WebSocket (`LoadDevPlugin`),
reloads it on changes and prints its log (host lines, its own lines, API
calls) until Ctrl+C. The desktop app has no such endpoint: use "Load a
development folder…" in Settings → Plug-ins there.

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
| `sieve-scripts` | – | `{active, uploads, scripts: [{name, active, content}]}` of the mock ManageSieve server (M2c) |
| `dav-objects` | `collection?` | `[{name, etag, data}]` of the mock CalDAV / CardDAV server: `personal` (default), `tasks`, `holidays` (read-only) or `contacts` (M3a) |
| `dav-put` | `collection?`, `name`, `data` | Stores an iCalendar / vCard object as another client would; `{etag}` |
| `dav-delete` | `collection?`, `name` | Deletes an object as another client would |
| `dav-user-put` | `user?`, `name`, `data` | Stores an event in the `personal` calendar of another principal (`bob` by default; free/busy, M3b) |
| `gmail-deliver` | `raw`, `labels?` | New message on the mock Gmail server (labels `INBOX`, `UNREAD` by default); `{id}` (M3b) |
| `gmail-message` | `id?` or `subject?` | `{id, labels}` of a Gmail message (by id or a subject it contains) |
| `gmail-modify` | `id?` / `subject?`, `add?`, `remove?` | Changes a message's labels on the server (history polling picks it up) |
| `gmail-labels` | – | `[{id, name}]` of the mock Gmail account |
| `gmail-sent` | – | Raw texts of what Nail sent through `messages.send` |
| `graph-deliver` | `raw`, `folder?` | New message in the mock Graph tenant of the `exchange` account (`inbox` by default); `{id}` (M3c) |
| `graph-messages` | `mailbox?` | `[{id, folder, subject, read, flag, categories, inference}]` of the user's (or a shared) mailbox |
| `graph-folders` | `mailbox?` | `[{id, name, well_known}]` |
| `graph-sent` | – | Raw texts of what Nail sent through `sendMail` |
| `graph-settings` | – | `{categories, automatic_replies, rules}` (Graph JSON) |
| `graph-pim` | – | `{events, tasks, contacts}` (Graph JSON; tasks with their list and checklist) |
| `graph-meetings` | – | `[{event_id, action}]`: meeting answers / cancellations received |
| `ews-deliver` | `raw`, `folder?` | New message on the mock EWS server (distinguished folder name, `inbox` by default); `{id}` (M3c) |
| `ews-messages` | `folder?` | `[{id, subject, read, categories, start, start_time_zone}]` of an EWS folder (distinguished name such as `inbox` / `calendar`, or a display name such as `Archive`) |
| `ews-settings` | – | `{oof: {state, internal, external}, rules: [{name, enabled}]}` of the mock EWS server |
| `ews-meetings` | – | `[{kind, subject, comment}]`: meeting requests, answers and cancellations the mock EWS server sent |
| `ews-sent` | – | Raw texts of what Nail sent through EWS |
| `feeds` | – | `{holidays, town}`: URLs of the demo ICS feeds (`holidays` is configured, `town` is for adding a subscription) |
| `clock` | – | `{now}`: the core's triage clock (snooze, quiet hours, notifications, reminders), Unix seconds |
| `clock-set` | `at` | Sets that clock to `at` (Unix seconds); it keeps running |
| `clock-advance` | `seconds` | Moves that clock forward (or back, negative) |

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

For M2c:

- **0043:** the M2c contract (localized core messages, Sieve / vacation, junk / snooze /
  notifications, folder sync policies and on-demand bodies, context menus, layout reset
  patches, MCP port setting, `RuleFromMessage`, store v7, the `nail-sieve` / `nail-triage`
  stubs, slots). Fluent texts built in Rust live in `locales/<lang>/core*.ftl`;
  `cargo test -p nail-api --test catalog` checks that every id the contract names exists.
- **0044:** `nail-sieve` — the Sieve compiler (placement + localized reasons), the
  parse / decompile round-trip harness, the ManageSieve client, upload / remove of the
  managed script; `nail-testkit-net`'s `MockSieveServer` + `validate_script`
- **0045:** `nail-triage` — tokenizer, junk classifier, snooze scheduler, notification policy
- **0046:** engine polish — localized Rust messages (`nail_config::l10n`), body sync policies,
  IMAP search keys, Markdown bodies, fixed MCP port, headless `nail mcp`, `DataDirLock`
- **0047:** UI polish — context menus, Customize editors, triage / Sieve / sync UI, lazy
  editors (vendor / catalogs chunks in `ui/vite.config.ts`), axe in Vitest
  (`ui/src/a11y.test.tsx`), `defaults/themes/` example themes
- **0048:** M2c integration (Sieve upload task, junk / snooze / notifications in the core,
  headers-only sync + on-demand bodies + server search, replay connection, localized
  messages end to end, desktop MCP port + lock, M2b follow-ups, the adjustable clock)

For M4a:

- **0079:** the M4a contract (writes by agents and plug-ins: `nail_api::agents` action kinds, policies
  (destructive kinds always ask; "always allow" per MCP client / plug-in for the rest) and
  confirmations; the MCP tool table, write-tool arguments (`--features json-schema` derives their JSON
  Schemas), prompts and rate limits; plug-in write / hook permissions, hooks, composer / attachment /
  link / MCP extensions, background jobs (`CronSchedule`), `nail.l10n`, binary fetch bodies; store v14;
  slots). Texts built in Rust: `locales/<lang>/core-agents.ftl`, `core-mcp.ftl`; UI: `agents.ftl`.

For M3c:

- **0072:** the M3c contract (Exchange accounts as an `exchange` node in `accounts.kdl` with
  `engine="graph"` (Exchange Online) or `"ews"` (on-premises), `auth "windows"` (SSPI Negotiate /
  Kerberos / NTLM); shared / delegate mailboxes, categories ↔ tags with colours, Focused Inbox,
  inbox rules, automatic replies, rooms, To Do; the engine seams `nail_api::exchange::engine` and
  PIM item shapes `nail_api::exchange::pim`; store v12 (a rebuild step with foreign keys off);
  the `nail-proto-graph` / `nail-proto-ews` stubs (mock Graph / EWS servers:
  `--features testing`) and `nail-exchange` (SSPI, Autodiscover, mapping; mock Autodiscover:
  `--features testing`); slots). Texts built in Rust: `locales/<lang>/core-exchange.ftl`,
  `core-graph.ftl`, `core-ews.ftl`; UI: `exchange.ftl`.
- **0073 (WP91):** `nail-proto-graph` — the Microsoft Graph engine and the mock Graph server
  (appended messages are drafts, category renames by replacement, To Do ids `<list>/<task>`,
  4 MB request limit).
- **0074 (WP92):** `nail-proto-ews` — the EWS engine and the mock EWS server (Exchange2016 with
  a 2013 SP1 fallback, connection-bound Negotiate, streaming notifications, Windows zone ids).
- **0075 (WP93):** `nail-exchange` — Autodiscover with engine choice and the credential rule,
  SSPI sign-in, the Exchange ↔ Nail mapping, EWS retirement notes.
- **0076 (WP94):** the Exchange UI (`ui/src/features/exchange/`).
- **0077:** M3c integration (the Exchange engine task in `nail-core/src/exchange/`: mail per
  folder, replay, Exchange queue, settings and rule placement, the `exchange:<account>` PIM
  source with meetings through the engines' meeting APIs, free/busy and rooms, `focus:`,
  Autodiscover in `DiscoverAccount`, per-protocol OAuth scopes; demo-multi Graph / EWS accounts
  and mock Autodiscover; `exchange.spec.ts`).

For M3b:

- **0065:** the M3b contract (invitations and scheduling: iTIP methods, the invitation card on
  `MessageBody.invitation`, answers / counters / organizer updates and cancellations, free/busy;
  Gmail accounts as a `gmail` node in `accounts.kdl` with labels ↔ folders / tags, thread ids,
  categories, label management; `MailApiEngine` / `AdaptivePoll` / `FreeBusyProvider`; store v11;
  the `nail-itip` / `nail-proto-gmail` stubs (mock Gmail server: `--features testing`); slots).
  Texts built in Rust: `locales/<lang>/core-scheduling.ftl`, `core-gmail.ftl`.
- **0066:** `nail-itip` — iTIP / iMIP (cards, applying, building, iMIP mail, conflicts,
  RFC 6638 delivery); texts in `locales/<lang>/itip.ftl`
- **0067:** `nail-proto-gmail` — the Gmail API engine and the mock Gmail server
- **0068:** CalDAV scheduling and free/busy in `nail-dav`; the scheduling mock DAV server
- **0069:** the invitations / scheduling / Gmail UI (`ui/src/features/{scheduling,gmail}/`)
- **0070:** M3b integration (`nail-core/src/scheduling.rs`, `gmail.rs`: cards, arrival
  processing, iMIP through the outbox as prepared mail, server scheduling, free/busy, the
  Gmail engine loop / replay / label queue / sending, Gmail discovery, `category:`;
  demo-multi Gmail account + DAV principals)

For M3a:

- **0058:** the M3a contract (calendars, events with recurrence / overrides / edit scopes,
  contacts, groups, merge links, tasks, reminders, `.ics` / `.vcf` import, read-only MCP
  payloads; PIM sources as `calendar-subscription` / `caldav` / `carddav` nodes in
  `accounts.kdl`; store v10; the `nail-ical` (calcard) / `nail-dav` stubs; slots). Texts built
  in Rust: `locales/<lang>/core-pim.ftl`, `core-ical.ftl`, `core-dav.ftl`.
- **0059:** `nail-ical` — own lossless content-line layer and patching writers, own
  recurrence engine (`recur.rs`, `expand.rs`, edit scopes in `edit.rs`), zones (IANA /
  Windows / custom `VTIMEZONE`), alarms, feeds, import previews; the calendar corpus
  (`crates/nail-ical/tests/corpus`, byte-exact: nested `.gitattributes -text`)
- **0060:** `nail-dav` — CalDAV / CardDAV client (discovery, sync-collection with ctag /
  ETag fallback, conditional writes, server wins), `MockDavServer` in `nail-testkit-net`,
  Radicale in `scripts/interop`
- **0061:** PIM store APIs (items + PIM queue, instance cache, contacts / duplicates /
  merge / Collected / autocomplete ranking, reminders), `PimIndex`, the reminder scheduler
- **0062:** the PIM UI (`ui/src/features/pim/`)
- **0063:** M3a integration (`nail-core/src/pim/`: requests, source engines, reminders,
  search; MCP PIM resources / tools; plug-in `calendar.read`; `nail serve --launch`;
  default layout windows; demo-multi DAV / ICS mocks)
