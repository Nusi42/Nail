# Nail — Implementation Plan

Source of truth for *what* to build: [`design.md`](design.md). This file says *how* and *in which order*.

## 1. Ground rules (apply to every work package)

- **Tests from day 1.** Every work package lands with unit tests. Every user-visible behaviour lands with at least one Playwright scenario. A work package is not done while `cargo test --workspace`, `npm test` or `npm run e2e` is red.
- **Docs split.** Internal material goes to `docs/` (this plan, the design, ADRs in `docs/adr/`, dev notes). User-facing manuals and references go to `userDocs/<lang>/` (`userDocs/en/` written, `userDocs/de/` empty for now).
- **Languages.** No literal user-visible strings in React or Rust. Strings live in Fluent catalogs `locales/en/*.ftl` (complete) and `locales/de/*.ftl` (same files, empty). Missing keys fall back to English per key.
- **Everything themeable.** React components never use inline styles or hard-coded class names. They emit `data-part` attributes; all styling comes from the compiled theme. Enforced by an ESLint rule and a unit test.
- **The core owns data.** The UI talks only to `NailClient`; business logic lives in Rust.
- **Windows first.** Develop and test on Windows 11 (x64). macOS/Linux/mobile later.
- **Decisions are recorded.** Any library choice or deviation from `design.md` gets a short ADR in `docs/adr/NNNN-title.md`.

## 2. Environment facts

| Tool | Version | Note |
| --- | --- | --- |
| Rust | 1.97.1, `x86_64-pc-windows-msvc` | Builds and links fine |
| Node / npm | 26.7.0 / 11.19.0 | **pnpm is not installed** → npm workspaces instead of pnpm (ADR 0001) |
| git | 2.55 | Repo initialized on `main` |
| docker | not installed | Integration tests use in-process mock servers (`nail-testkit`); real Dovecot/Stalwart/Radicale containers run in CI later |
| gh | not installed | No GitHub remote yet |

The repo lives in a OneDrive folder. A local, gitignored `.cargo/config.toml` points `target-dir` to `%LOCALAPPDATA%\nail-build\target` so build output is not synced.

## 3. Repository layout

```
Cargo.toml                 # workspace; default-members exclude the Tauri app
package.json               # npm workspaces: ui, e2e
crates/
  nail-api/                # shared request/response/event + domain types (serde + ts-rs → ui/src/api/generated)
  nail-config/             # KDL theme/layout parsing, validation, defaults, JSON Schema, watcher
  nail-store/              # SQLite metadata, content-addressed blob store, operation queue
  nail-mime/               # MIME parsing → summaries/bodies, HTML sanitizing
  nail-proto-imap/         # IMAP client (read-only in M0)
  nail-testkit/            # mock IMAP server, .eml corpus, fixtures, demo seed
  nail-core/               # service: routes Requests, owns store/config/sync, emits Events
  nail-server/             # axum: serves UI bundle, WebSocket API, blob endpoint
  nail-cli/                # `nail` binary: serve, (later) mcp, plugin
app/src-tauri/             # Tauri 2 desktop shell (one `nail_call` command + event bridge)
ui/                        # React + TS + Vite renderer
e2e/                       # Playwright tests (web mode + later packaged app)
defaults/theme.kdl, defaults/layout.kdl
locales/en/*.ftl, locales/de/*.ftl
docs/ (internal)  userDocs/en, userDocs/de (user-facing)
.github/workflows/ci.yml
```

## 4. Key technical decisions for M0

| Topic | Decision | Why |
| --- | --- | --- |
| RPC shape | One transport-agnostic JSON protocol: `Request` enum → `Response`, plus `Event` stream. Tauri exposes it as a single `nail_call` command + `nail://event`; the server exposes it over one WebSocket | The UI is identical in the desktop app and the browser; one code path to test |
| TS types | `ts-rs` generates TypeScript from `nail-api` into `ui/src/api/generated/` (checked in; a test fails when stale) | Transport-agnostic, unlike tauri-specta |
| Config format | KDL v2 (`kdl` crate), typed models via serde-like mapping, JSON Schema via `schemars` | Decided in design review |
| Store | `rusqlite` (bundled SQLite, WAL), hand-rolled numbered migrations, blobs = sha256-addressed zstd files | Simple, no ORM |
| IMAP | Agent picks `async-imap` or `imap-next` and records an ADR; must support TLS (rustls) and plaintext (tests) | |
| Server | `axum` + `tokio-tungstenite`, bound to 127.0.0.1 only in M0 (auth arrives with remote-UI work in M2) | |
| UI stack | React 19, Vite, TanStack Query + Zustand, TanStack Virtual, Fluent (`@fluent/bundle`), Vitest + Testing Library, ESLint | Per design |
| E2E | Playwright against `nail serve --seed demo` (web mode) | Same UI as desktop; runs headless in CI |

## 5. Milestone M0 — Foundations (detailed)

Gate: the default UI is driven entirely by `defaults/theme.kdl` + `defaults/layout.kdl`; unit and Playwright tests run in CI.

### WP0 — Scaffold and contract
- Cargo workspace with all crates above (compiling stubs), npm workspaces (`ui`, `e2e`), `rustfmt.toml`, clippy config, ESLint/Prettier, `.github/workflows/ci.yml` skeleton.
- `nail-api`: domain types (`AccountId`, `Account`, `Folder` with role, `MessageId`, `MessageSummary`, `MessageBody` {text, sanitized_html, attachments}, `Flags`), `ResolvedTheme`, `ResolvedLayout` (opaque JSON trees in M0), `Request`/`Response`/`Event` enums, error type; ts-rs export + staleness test.
- ADR 0001 (npm workspaces), ADR 0002 (RPC protocol).
- **Done when:** `cargo test --workspace`, `npm test` pass on the empty skeleton.

### WP1 — nail-config (theme + layout)
- Parse `theme.kdl` (tokens, modes light/dark, parts with states and data conditions, icons, message section) and `layout.kdl` (windows, containers split/stack/tabs, widgets with options, row templates, actions, keys, toolbars, menus, breakpoints).
- Sparse files with `extends="default"`: merge onto shipped defaults; token references `$color.accent` resolved.
- Validation errors with file, line, column and a readable message; invalid file → keep last valid config.
- JSON Schema export (`nail config schema`) and resolved JSON for the UI.
- File watcher (`notify`) → `Event::ConfigChanged`.
- Ship `defaults/theme.kdl` and `defaults/layout.kdl` describing the three-pane mail UI.
- **Tests:** parsing every section, merge semantics, error positions, round-trip of defaults, schema snapshot.

### WP2 — nail-store
- SQLite schema v1: accounts, folders (role, uidvalidity, uidnext), messages (envelope, flags, thread key, blob hash), op_queue; migrations table.
- Blob store: put/get by sha256, zstd, dedup.
- Operation queue: enqueue → apply locally → pending → replayed via a `Replayer` trait → done/failed; survives restart.
- **Tests:** migrations from empty, CRUD, blob dedup, queue ordering and restart recovery (tempdir).

### WP3 — nail-mime, nail-proto-imap, nail-testkit
- `nail-mime`: parse raw RFC 5322 into `MessageSummary` + `MessageBody`; HTML sanitized with `ammonia` (no scripts, remote images stripped/flagged).
- `nail-proto-imap`: connect (implicit TLS / STARTTLS / plaintext-for-tests), LOGIN + AUTHENTICATE PLAIN, CAPABILITY, LIST with SPECIAL-USE roles, EXAMINE, UID FETCH of flags/envelope/BODY.PEEK[], incremental fetch via UIDVALIDITY/UIDNEXT. Read-only.
- `nail-testkit`: in-process mock IMAP server over a folder of `.eml` files; a corpus of normal, HTML, multipart, malformed and hostile messages; `seed_demo(store)` for the UI.
- **Tests:** MIME corpus snapshots, sanitizer blocks script/tracking pixels, IMAP client against the mock server (list, fetch, incremental, bad credentials).

### WP4 — UI renderer
- `NailClient` interface with `TauriTransport`, `WebSocketTransport`, `MockTransport`.
- Layout engine: resolved layout JSON → React tree via a widget registry; containers `split` (resizable), `stack`, `tabs`; breakpoints by window width.
- Theme compiler: tokens → CSS custom properties; parts → one generated stylesheet keyed on `[data-part]` + state attributes; light/dark mode switch.
- M0 widgets: `account-switcher`, `folder-tree`, `message-list` (virtualized, row templates), `message-view` (headers + sandboxed `iframe srcdoc` with CSP), `toolbar`, `status-bar`, `text`, `spacer`.
- Actions registry + keybindings from the layout (`mail.next`, `mail.previous`, `view.toggle`).
- i18n: Fluent loader, `en` complete, `de` empty, per-key fallback; locale from OS/browser with override.
- ESLint rules: no `style` prop, no literal `className`, no JSX string literals (`react/jsx-no-literals`).
- **Tests (Vitest):** layout engine, theme compiler, template language, each widget with MockTransport, i18n fallback, lint rule self-test.

### WP5 — Core wiring, server, CLI, desktop shell
- `nail-core`: `Service` owning store + config + sync; handles every `Request`; broadcasts `Event`s; sync engine pulls IMAP → store (read-only).
- `nail-server`: axum on 127.0.0.1; serves `ui/dist`, `/api/ws` (RPC + events), `/api/blob/:hash`.
- `nail-cli`: `nail serve [--port] [--data-dir] [--seed demo] [--config-dir]`.
- `app/src-tauri`: Tauri 2 app hosting `Service`; `nail_call` command and event forwarding; loads the same UI.
- **Tests:** Service request handling with testkit, server WebSocket round-trip, CLI arg parsing.

### WP6 — E2E and CI
- Playwright (`e2e/`): `webServer` runs `nail serve --seed demo` on a free port with a temp data dir.
- Scenarios: default three-pane layout renders; folders and messages from seed appear; selecting a message shows its body inside a sandboxed iframe; keyboard navigation; editing the theme file hot-reloads a CSS variable; editing the layout file moves a pane; German locale falls back to English strings; axe accessibility scan passes.
- CI (`windows-latest`): fmt, clippy `-D warnings`, `cargo test`, `npm ci`, lint, Vitest, UI build, Playwright.

### WP7 — Docs
- `docs/adr/*`, `docs/dev-setup.md` (build, test, run).
- `userDocs/en/`: getting started, theme reference and layout reference (M0 subset), `userDocs/de/` placeholder.

## 6. Milestone M1 — Daily-driver mail (detailed)

Status of M0: done (gate passed). M0 follow-ups are folded into the work packages below (marked *carry-over*).

**Automated gate:** all suites green; the Playwright scenarios listed in WP14 and WP25 pass against mock IMAP / POP3 / SMTP servers.
**Human gate (after M1b):** the owner uses Nail as the only mail client for two weeks.

**Status:** M1a and M1b done; the automated gate passed on an independent verify run — `cargo test --workspace` 965 tests, Vitest 416 (stable under CPU load after a timing fix), Playwright 39 scenarios incl. every WP14 and WP25 scenario and the desktop WebView2 smoke test, clippy / fmt / ESLint / tsc / Prettier clean. Review findings fixed (ADR 0028). WP24's toolbar / key binding / column editing UI is deferred (see "M1b follow-ups" below). Next: the human gate (two weeks as the only mail client), which needs the owner tasks below.

**Scope decisions for M1**
- Gmail works in M1 through IMAP + SMTP with OAuth2 (XOAUTH2). The Gmail API engine (decided in the design review) moves to M3, next to the other proprietary-API engine (Exchange); both need OAuth app registration and mock REST servers.
- The plug-in preview moves to M2, as the design's plug-in section says.
- OAuth needs registered client ids for Google and Microsoft. Nail reads them from configuration; registering the apps is an owner task (see "Owner tasks"). Tests use a mock OAuth server.

M1 runs as two waves (two workflows), each with its own review, fix and verify stages.

### Wave M1a — Mail engine

#### WP10 — Contract and accounts config (first, sequential)
- `nail-api`: account configuration types (incoming IMAP/POP3, outgoing SMTP, auth method password / OAuth2 / client certificate, TLS mode, sync policy per folder), `Identity`, `VirtualAccount` (members, folder mapping, default identity, per-domain identity rules), `ThreadSummary`, `SearchQuery` / `SearchHit`, `Draft` (plain-text body in M1a; HTML/Markdown fields reserved for M1b), op results.
- New requests: `DiscoverAccount{email}`, `TestAccount{config}`, `AddAccount`, `UpdateAccount`, `RemoveAccount`, `ListIdentities`, virtual-account CRUD, `UpdateFlags`, `MoveMessages`, `DeleteMessages`, `ListThreads`, `GetThread`, `Search`, `SaveDraft`, `SendDraft{delay}`, `CancelSend`, `ListOutbox`. New events: `OpStatus`, `SendStatus`, `NewMail`, `AccountStatus` (online / offline / auth error).
- `accounts.kdl` in the config dir (accounts, identities, virtual accounts; secrets never in the file, only keychain references), parsed in `nail-config` with the same error/position machinery, plus JSON Schema.
- ADRs: accounts file format, protocol additions.

#### WP11 — IMAP engine, full (parallel)
- IDLE with reconnect and 29-minute re-issue; NOOP polling fallback; IDLE on Inbox by default, other folders configurable.
- CONDSTORE/QRESYNC: flag changes and expunges (`VANISHED`) since `HIGHESTMODSEQ`; fallback full `UID FETCH 1:* FLAGS` diff when unsupported (*carry-over: sync picks up flag changes and deletions*).
- Writes: `UID STORE` flags, `MOVE` (fallback `COPY` + `\Deleted` + `UID EXPUNGE`), `APPEND` (drafts, sent copies) with `UIDPLUS` results, folder `CREATE` / `RENAME` / `DELETE`.
- SASL `XOAUTH2` and `OAUTHBEARER`; `COMPRESS=DEFLATE` when offered; `ID`.
- Extend the testkit mock server: IDLE push, MODSEQ / CONDSTORE / QRESYNC, STORE, MOVE, COPY, APPEND, EXPUNGE, UIDPLUS, XOAUTH2, fault injection (drop connection mid-IDLE, slow server).

#### WP12 — Outgoing, POP3, auth, discovery (parallel)
- `nail-proto-smtp`: submission via `lettre` (implicit TLS 465, STARTTLS 587, PLAIN / LOGIN / XOAUTH2), size check, optional DSN; testkit mock SMTP server that records messages.
- `nail-proto-pop3`: in-house client (STLS, USER/PASS, APOP, SASL PLAIN, CAPA, UIDL, TOP, RETR, DELE, QUIT); leave-on-server policies (delete after N days, delete when deleted locally); testkit mock POP3 server.
- `nail-auth`: OAuth2 authorization code + PKCE with a loopback redirect, token refresh, provider registry (Google, Microsoft; client ids from configuration); secret storage in the OS keychain (`keyring`) behind a trait, with an in-memory backend for tests.
- `nail-discovery`: Mozilla ISPDB, `autoconfig.<domain>` and `/.well-known/autoconfig`, RFC 6186 SRV records, Microsoft Autodiscover lookup, common-host guessing; ranked results returned for confirmation. Tests against mock HTTP and DNS layers.

#### WP13 — Store v2, threading, search (parallel)
- Store migration v2: identities, virtual accounts, message tags (IMAP keywords), thread ids, `message_id` / `references` columns, per-folder `highestmodseq`, outbox table.
- Operation queue fixes (*carry-over*): per-account ordering and passes; sync-side merge that keeps local flags while a pending op for that message exists.
- Threading: JWZ over Message-ID / In-Reply-To / References with subject fallback; incremental updates; cross-account threads for virtual accounts.
- `nail-search`: Tantivy index (headers, decoded text, attachment names), incremental add/remove, and the design's query language (`from: to: subject: has:attachment is:unread is:flagged tag: account: folder: before: after: size>`, quotes, AND / OR / NOT, parentheses) with a hand-written parser and precise error positions.

#### WP14 — Integration: accounts, sync, send, virtual accounts, UI
Split in execution: the UI part (WP14-UI) runs in parallel with WP11–WP13 against the WP10 contract and `MockTransport`; the core part and the Playwright scenarios (WP14-core) run afterwards, sequentially.
- `nail-core`: account manager (load/save `accounts.kdl`, keychain), per-account sync engines (IMAP IDLE loop + polling, POP3 schedule), op replayers (IMAP flags/move/delete/append; POP3 delete policies), outbox with undo-send delay and send-later, Sent copy via APPEND (skipped for servers that save it automatically, e.g. Gmail), virtual-account routing (folder mapping by role, Message-ID dedupe, reply identity selection), search and thread requests, `AccountStatus` events.
- UI: account setup wizard (email → discover → confirm → test → save; password or OAuth), account and identity settings, virtual-account editor, unified inbox (built-in "All accounts" virtual account), thread view, flag / move / delete / archive actions with keybindings, search bar with query-language hints, a minimal plain-text composer (upgraded in M1b), outbox view, account status indicators.
- Every new string in `locales/en`; every widget themeable; default layout updated.
- **Playwright:** add an account via discovery against mock servers; unified inbox shows two accounts; flag / move / delete reach the mock IMAP server; a flag change on the server appears in the UI (IDLE); search with the query language; thread view; send a plain-text mail to the mock SMTP server, once cancelled by undo-send and once for real; a POP3 account receives mail; the offline queue replays after reconnect; a virtual account merges folders and dedupes a mailing-list message.
- `userDocs/en`: accounts, virtual accounts, search syntax.

### Wave M1b — Authoring, rules, agents, customization

#### WP20 — Contract for M1b (first, sequential)
Draft rich bodies (HTML, Markdown source, attachments, inline images, reply/forward context), rules model and requests (list / save / test / run / undo run), MCP client grants and audit entries, config write-back requests (layout patches), merge-dialog requests (preview / apply), UI-context plumbing, saved searches.

#### WP21 — Composer (parallel)
- `nail-compose`: MIME building (`mail-builder`): `multipart/alternative` (text + HTML), `multipart/related` for `cid:` images, attachments, optional `text/markdown` part; Markdown rendering with `comrak` (CommonMark + GFM + footnotes + highlighted code); per-identity mail stylesheet inlined with `css-inline`; outgoing HTML sanitized; HTML → text; reply/forward quoting (HTML blockquote, Markdown `>`), top/bottom posting and localized quote header per identity; signatures (HTML and Markdown variants); templates with variables.
- UI: TipTap HTML editor (tables, images, links, source view), CodeMirror 6 Markdown editor with side-by-side preview rendered by the core, plain-text mode; mode switching with a lossy-conversion warning; drafts autosave (local + server Drafts); attachment reminder; identity picker; address autocomplete from collected addresses.

#### WP22 — Rules engine (parallel)
- `rules.kdl` (parsed in `nail-config`) and `nail-rules`: triggers (arrival, on send, schedule, manual), nested all / any / not conditions (headers, regex, query language, attachments, size, date, flags/tags, account, auth results, list-id), actions (move, copy, delete, archive, tag, flag, mark read, forward / redirect, reply with template, notify / suppress, stop); dry-run "test rule"; every run goes through the op queue and can be undone as one unit.
- UI: rule list and visual editor, text view of the same rule, "create rule from message", test-results preview.

#### WP23 — MCP server, read-only (parallel)
- `nail-mcp` with `rmcp`: Streamable HTTP on 127.0.0.1 with per-client bearer tokens; `nail mcp --client <name>` stdio bridge to the running app over a named pipe; the design's resources (accounts, folders, messages incl. raw and attachments, threads, rules, `ui/context`) and read tools (`search_messages`, `get_message`, `get_thread`, `list_folders`, `get_ui_context`); resource subscriptions for new mail and UI context.
- Grants: new clients have **no access until granted** (decided); per-account scope; separate grants for raw source, attachments, encrypted content and UI context; audit log; message content marked as untrusted data.
- UI: MCP client settings (create token, grant scopes, revoke, audit log view); an on-screen indicator while an agent reads the UI context.
- Tests with an MCP client (`rmcp` client) against the server.

#### WP24 — Customization tooling (parallel)
- Inspector: element-picker overlay showing part path, states, matching theme rules and the layout node, with "open file at line".
- Write-back: split sizes, column order, toolbar order and keybinding edits made in the UI are written into `layout.kdl`, with comments and formatting preserved.
- Merge dialog for applying a shared theme or layout (per-section keep mine / take theirs, live preview, conflict flags, backup + revert).
- All config errors shown at once (*carry-over*): an error list per file instead of a single slot; `nail-config` reports every error in a file.
- Saved searches as smart folders placeable in the layout.

#### WP25 — Integration for M1b (sequential)
Wire everything, update the default layout, write docs (`userDocs/en`: composer, Markdown mail, rules, MCP setup, inspector and customization), and add the Playwright scenarios below.
- **Playwright:** compose and send HTML and Markdown mails (assert the MIME parts received by the mock SMTP server); reply quoting in both modes; draft autosave and restore; a rule moves arriving mail and its run can be undone; test-rule preview; an MCP client lists messages only after a grant; the inspector shows a part path; dragging a divider writes `layout.kdl`; the merge dialog applies a theme; the config error list shows two broken files; **desktop app smoke test over WebView2 CDP** (*carry-over*).
- *Done (ADR 0027):* all of the above in `e2e/tests/{composer,rules,mcp,customization,desktop,a11y}.spec.ts`. Additions: `ListDrafts` (drafts are continued from the outbox window, which is how "restore" works after a reload), per-identity `posting` / `mail-style` in `accounts.kdl`, "Retry" for failed writes, axe scans of the demo-multi windows. The desktop smoke test runs locally and is gated behind `NAIL_E2E_DESKTOP=1` in CI.

#### M1b follow-ups (deviations and deferrals)
Recorded by the M1b review (ADR 0028); each becomes an M2 work item unless noted.
- **WP24 editing UI for toolbar order, key bindings and column order** — deferred to M2. The `layout.kdl` write-back for all three exists and is tested (ADR 0026, `ui/src/features/customization/writeBack.ts`); only split sizes are edited from the UI (dragging a divider). Needs a toolbar reorder, a key-binding editor and column reordering in the Customize tab.
- **`nail mcp` bridge over loopback HTTP instead of a named pipe** — kept (ADR 0025: the pipe would need its own auth; the loopback server already checks tokens, Host and Origin).
- **Headless start of `nail mcp`** when the app is not running — deferred to M2 (needs a single-instance lock on the data directory; ADR 0025).
- **`notify` / `suppress` rule actions have no effect** — until new-mail notifications exist (M2 polish).
- **"Create rule from message"** is built in the UI from the summary, without `List-Id` — M2: expose the core's `rule_from_message`.
- **`nail-mime` lists a received `text/markdown` alternative as an attachment** — M2 fix.
- Smaller items from ADR 0027 (right-click menus, alt text of inline images in reopened HTML drafts, lazy-loaded editors, a fixed MCP port setting) — M2 polish.

### Owner tasks (outside the code)
- Register OAuth apps for Google (Gmail IMAP/SMTP scope) and Microsoft (Outlook.com / Microsoft 365 IMAP/SMTP scopes) and provide the client ids.
- Provide real test accounts for the manual two-week gate.

## 7. Milestone M2 — Security, remote UI, server-side rules, polish, plug-in preview (detailed)

**Automated gate:** all suites green, including the Playwright scenarios listed per wave and the new CI jobs (installer build, desktop smoke test, real-server interop).
*Status: M2 automated gate PASSED on the independent M2d verify* — `cargo test --workspace` 1905 tests, Vitest ui 877 + SDK 50 + template 21 (stable over 5 runs, including under CPU load), Playwright 72 scenarios (incl. the desktop smoke test and every wave's scenarios), clippy / fmt / ESLint / tsc / Prettier clean on Windows. Its two findings are fixed post-M2 (`nail plugin dev` reloads a plug-in's catalogs on every hot reload, also when only a locale file changed; host log lines name dev plug-ins by their resolved name). Still unproven until the first CI run on GitHub: the installer smoke job, the desktop smoke job (`NAIL_E2E_DESKTOP=1`), `cargo deny` with advisories and the interop job against real Dovecot / Stalwart servers (offline runs against the mocks pass). Open human gates: the security review sign-off (incl. the plug-in sandbox), the external pentest before internet mode, the owner's decision on the encryption-at-rest default, and OAuth client ids + real test accounts for the M1 two-week gate.
**Human gates:** (1) a security review of crypto, the HTML sandbox and the remote UI (the design's M2 gate; the agents' security reviews prepare it, a human signs it off); (2) an external penetration test before the remote UI's internet mode can be unlocked — that stays locked in M2.

**Scope decisions for M2**
- **Licences.** Nail is closed source for now, so M2 adds no GPL/LGPL/AGPL dependency, and `cargo-deny` enforces a licence allowlist in CI. Consequence: OpenPGP uses **rPGP** (`pgp` crate, MIT/Apache) instead of Sequoia (LGPL), unless the owner decides otherwise. MPL-2.0 is acceptable (file-level copyleft, unmodified use).
- **No OpenSSL build dependency** unless an ADR shows there is no pure-Rust or Windows-native alternative (S/MIME: RustCrypto `cms` + Windows certificate store; encryption at rest: evaluated in WP33).
- **Encryption at rest** is implemented as a setting. Its default (on or off) is still an open owner decision; until then it ships **off**, with the recommendation to turn it on recorded in the UI and docs.
- **Internet mode for the remote UI** is built (ACME, hardening) but locked behind a setting that the UI refuses to enable until the pentest gate is recorded.
- The plug-in preview is the design's preview scope: commands, UI contributions, dialogs, read access, notifications, storage. Write access, hooks and the full extension-point list stay in M4.

M2 runs as four waves (four workflows), each shaped like the M1 waves: contract → four parallel packages in disjoint directories → integrate → two reviews (one always security) → fix → independent verify.

### Wave M2a — End-to-end encryption and data protection

**Status:** WP30–WP35 done (ADRs 0029–0034); automated suites green (cargo, Vitest, Playwright incl. `e2e/tests/security.spec.ts`). Review findings fixed (ADR 0035): Autocrypt keys bound to their header's address with per-address trust, signers bound to the displayed `From`, the MCP `encrypted` gate follows the reader, drafts of mail that goes out encrypted (defaults, Autocrypt) stay off the server and earlier plain copies are removed, `authserv-id` trusted only when configured (new `SuggestAuthservId` + account settings "Sender checks"). Next: independent verify; the M2 human security review gate stays open.

#### WP30 — Contract (sequential)
*Done (ADR 0029):* protocol module `nail_api::security` (+ optional fields on `MessageBody`, `Identity`, `Draft`, `SendDraft`; error codes `locked`, `no_key`, `crypto`), identity `crypto` node in `accounts.kdl`, store v5 with tested accessors (keyring, trust, Autocrypt peers, vault state, message security, settings), stub crates with library smoke tests, slots for WP31–WP34 (incl. `nail-mime` modules and `corpus/crypto/{pgp,smime}`). The core serves the security settings and vault status; other security requests answer `unsupported`, and sign / encrypt on send is refused until WP35.
Per-message `SecurityStatus` (signed / encrypted / verified, signer identity, key or certificate id, trust level, chain summary, header-protection state); `AuthResults` (DKIM / SPF / DMARC / ARC from `Authentication-Results`); key and certificate management requests (list, import, export public, generate OpenPGP key, delete, set trust, look up by address via WKD / keys.openpgp.org, import PKCS#12, list Windows-store certificates); per-identity crypto settings (default sign / encrypt, preferred technology, Autocrypt prefer-encrypt); compose-time recipient key status; `SendDraft` sign/encrypt options; encryption-at-rest settings and status; stub crates `nail-pgp`, `nail-smime`, `nail-vault` (at-rest encryption).

#### WP31 — OpenPGP and Autocrypt (parallel)
rPGP-based `nail-pgp`: keyring (public keys + secret keys encrypted with a passphrase or a random key kept in the OS keychain), generate (Ed25519/X25519 default, RSA import), import / export, PGP/MIME sign, encrypt, sign+encrypt, decrypt, verify (RFC 3156); inline PGP read-only; header protection for encrypted mail (encrypted subject shown as "..." on the wire); Autocrypt Level 1 (parse headers, peer state, prefer-encrypt, setup message); key discovery via WKD and keys.openpgp.org behind injectable HTTP; trust model (unknown / verified / autocrypt-gossip). Tests with interoperability vectors (messages produced by GnuPG and Thunderbird checked into the corpus).
*Done (ADR 0030):* rPGP 0.20; GnuPG 2.4 vectors in Thunderbird's MIME layout (`corpus/crypto/pgp/generate.sh`); GnuPG verifies and decrypts Nail's output. Not done: OpenPGP v6 keys / SEIPD v2 output, designated revokers, removing the RFC 9788 legacy-display part.

#### WP32 — S/MIME (parallel)
`nail-smime`: CMS SignedData / EnvelopedData via RustCrypto (`cms`, `x509-cert`, `rsa`, `p256` …); S/MIME 4.0 (RFC 8551) sign (detached `multipart/signed` and opaque), encrypt (AES-GCM / AES-CBC for compatibility), decrypt, verify; certificate import from PKCS#12 and from the Windows certificate store (`windows` crate: CryptoAPI / CNG), chain building and validation against the Windows trust store, expiry and key-usage checks, revocation marked as "not checked" in M2; smartcards (PKCS#11) deferred to M4. Test vectors from OpenSSL-generated fixtures (generated once, checked in).
*Done (ADR 0031):* RustCrypto `cms` / `x509-cert` 0.2, own chain validation, `p12-keystore`, CNG keys that stay in Windows; OpenSSL 3.5 vectors both ways. Not done: RSA-OAEP output, name / policy constraints (such chains are `invalid`), AIA fetching, S/MIME header protection.

#### WP33 — Data protection (parallel)
`nail-vault` + store/search integration hooks: encryption at rest for the database, blob store and search index (evaluate SQLCipher-compatible options without an OpenSSL build dependency, application-level encryption of blobs with XChaCha20-Poly1305, an encrypted Tantivy `Directory`), key held in the OS keychain, migration between plain and encrypted data with crash safety; decrypted-content indexing policy (off by default, opt-in per account, index then counts as sensitive); `Authentication-Results` parsing in `nail-mime` (DKIM / SPF / DMARC / ARC, trusted-authserv-id per account); link safety analysis (display vs target mismatch, punycode look-alikes, dangerous attachment types) exposed as message warnings.
*Done (ADR 0032):* SQLCipher rejected (OpenSSL on Windows); sealed columns (`messages.preview`, `drafts.payload`), sealed blobs and an encrypting tantivy `Directory` with XChaCha20-Poly1305, a crash-safe three-phase migration. Subjects, addresses, dates and flags stay plain in SQLite (documented). Follow-up: full-file encryption via SQLite3MultipleCiphers (needs a `libsqlite3-sys` patch), a recovery key for the data key.

#### WP34 — Security UI (parallel)
Key and certificate manager (OpenPGP keys, S/MIME certificates, trust editing, import/export, key lookup), per-identity crypto settings, composer sign/encrypt toggles with live recipient key status, message header security badge with a details popover (signer, key id, trust, chain), auth-results badge, link and attachment warnings in the reader, encryption-at-rest settings with progress, Autocrypt setup message flow.
*Done (ADR 0033):* seven widgets and a dialog in `ui/src/features/security/`, a full UI mock.

#### WP35 — Integration (sequential)
Core wiring: decrypt/verify on read (never persist decrypted bodies unless the indexing opt-in is on), sign/encrypt on send, Autocrypt headers on outgoing mail and peer-state updates on incoming mail, key lookup when composing, auth results and link warnings in `MessageBody`, at-rest encryption switch with migration, MCP: encrypted content only with the separate grant. **Playwright:** send a signed + encrypted OpenPGP mail between two demo accounts and read it with a valid-signature badge; tampered signature shows as invalid; S/MIME sign and encrypt round trip with a test certificate; Autocrypt enables encryption after one exchange; header protection hides the subject on the wire (assert on the mock SMTP); DKIM/DMARC badge from a seeded `Authentication-Results`; a look-alike link shows a warning; encryption at rest on/off with data intact; MCP cannot read encrypted content without the grant.
*Done (ADR 0034):* all of the above in `e2e/tests/security.spec.ts` (demo-multi accounts seeded with an OpenPGP key and a test-CA S/MIME certificate each), plus core integration tests (`nail-core/tests/security.rs`). Contract additions: `AccountConfig.authserv_ids` (`authserv-id` in `accounts.kdl`), `AutomaticMail::AutocryptSetup`. Replies to encrypted mail start encrypted; rules never forward or answer encrypted mail; encrypted drafts stay off the server's Drafts folder; Bcc recipients get their own encrypted copy.

#### M2a follow-ups (deviations and deferrals)
- **cargo-deny exception** for RUSTSEC-2023-0071 (`rsa` 0.9, Marvin timing side channel; no fixed release in rPGP's RustCrypto generation; decryption is blinded) — record it when WP44 adds `cargo-deny`.
- **S/MIME inside `multipart/mixed`** (a list footer after a signed entity) is not unwrapped; S/MIME header protection is neither produced nor read.
- **Attachments of encrypted mail over MCP** are served from the raw (encrypted) message even with the `encrypted` grant.
- **Header-protected subjects** stay `...` in the message list (decrypted text is never stored); showing them needs an opt-in cache like decrypted indexing.
- **Autocrypt learns only from mail that arrives after a folder's first sync** (the rule inbox); an initial import of old mail does not build peer state.
- **Partial sends**: when a Bcc copy fails after the visible copy went out, a retry sends the visible copy again.
- Encryption-at-rest default (on / off) is still the owner's decision; it ships off.
- **`authserv-id` suggestions** (ADR 0035) can still name the receiving server's own domain when the provider adds no `Authentication-Results` and does not strip forged ones; the user confirms by saving, and the docs say to check the message source first.

### Wave M2b — Remote UI hardening, installer and CI

**Status:** WP40–WP45 done (ADRs 0036–0041); automated suites green (cargo, Vitest, Playwright incl. `remote.spec.ts`, `remote-content.spec.ts` and the desktop smoke test with a `mailto:` start). Review findings fixed (ADR 0042): every remote request is checked against the grant (exhaustive, incl. drafts, the composer's original message, identities, outbox, operations, rules, remote-content allows, the UI context) with the `encrypted` gate on drafts and searches over decrypted text; remote sockets get only the events their grant covers (admin events only for admins); outside internet mode only local peers may connect and allowed networks must be local; the portable zip ships the UI files and the installed CLI finds them; the global pairing count has a one-hour window and a successful pairing clears it. Next: independent verify; the first CI run of the `installer`, `deny` and `interop` jobs; the M2 human security review gate stays open, internet mode stays locked.

#### WP40 — Contract (sequential)
Remote-UI settings (enabled, bind address, port, TLS mode self-signed/ACME, internet mode locked flag), users and credentials (password hash, TOTP secret, passkeys), sessions and devices, pairing codes, scopes per session, remote-content policy (per message / sender / domain allow-lists), audit entries for logins.
*Done (ADR 0036):* `nail_api::remote` (settings with shared validation and the internet-mode lock, `RemoteGrant` = MCP grant + `admin`, users / passkeys / devices / sessions / pairing codes / audit / alerts, the `POST /api/auth` login protocol) and `nail_api::remote_content` (allow-lists, `nail-proxy:` URLs, optional `MessageBody.remote_content`); error codes `forbidden`, `reauth_required`, `rate_limited`; store v6 with hashed or keychain-referenced secrets only and tested invariants; the core serves the store-backed requests, the rest answers `unsupported` until WP45. Stub crates `nail-remote-auth`, `nail-proxy` and TLS libraries in `nail-server` with smoke tests. Deviation: passkeys use `webauthn_rp` (pure Rust) because `webauthn-rs` requires OpenSSL.

#### WP41 — Authentication and sessions (parallel)
Passkeys via `webauthn-rs` (MPL-2.0, unmodified), password (Argon2id) + TOTP (`totp-rs`), one-time pairing codes approved on the desktop, device-bound short-lived sessions in `HttpOnly; Secure; SameSite=Strict` cookies with rotation, re-authentication for sensitive actions, per-IP and per-account rate limits with exponential backoff and lockout notification, session list and revoke, new-device alert, audit log. Tests incl. brute-force and replay attempts.
*Done (ADR 0037):* `nail-remote-auth` on `webauthn_rp` (ADR 0036 deviation), CSRF tokens bound to the device (`x-nail-csrf` response header after pairing), a 7-day throttle window (see follow-ups), the exhaustive `required_access`, axum login route and session middleware.

#### WP42 — Transport and browser hardening (parallel)
TLS 1.3 (1.2 only with forward-secret AEAD) with `rustls`; self-signed certificate generation (`rcgen`) with fingerprint shown for pinning at pairing; ACME (`instant-acme`) wired but only usable in the locked internet mode; HSTS, strict CSP without inline script, Trusted Types, COOP/COEP, `frame-ancestors 'none'`; Origin/Host checks; a separate sandbox origin for mail HTML; the remote-content proxy for desktop and web: signed URLs derived from messages, blocks private / link-local / loopback ranges, size and type limits, never an open proxy; loading remote content per message / sender / domain. Tests incl. SSRF attempts and DNS rebinding.
*Done (ADR 0038):* `Server::bind_with` (TLS, host names, allowed networks, header profiles, sandbox origin, proxy route, ACME challenges), `nail-proxy` (check-then-pin fetcher with the OS resolver instead of hickory). The `nail-mime` rewriting hook moved to WP45.

#### WP43 — Remote-UI and privacy UI (parallel)
Login (passkey first, password + TOTP fallback), pairing flow on both sides (desktop shows code and fingerprint, browser enters code), sessions and devices management, remote-UI settings on the desktop (enable, bind address, TLS mode, internet mode shown as locked with the reason), "load remote content" bar in the reader with per-sender/domain choices, security audit view.
*Done (ADR 0039):* six widgets, the pairing dialog, login / re-authentication pages over `POST /api/auth`, alert toasts, full UI mocks (`@simplewebauthn/browser`, `uqr`; both MIT).

#### WP44 — Installer and CI (parallel)
Tauri NSIS installer per the design (per-user by default, WebView2 bootstrapper, `mailto:` / `webcal:` protocol handlers and `.eml` / `.ics` / `.vcf` associations via RegisteredApplications + Capabilities, optional start with Windows, optional `nail` CLI on `PATH`, uninstall keeps data unless the user opts in), portable zip; `cargo-deny` with the licence allowlist and advisories; SBOM (CycloneDX) per build; CI jobs: installer build artifact, desktop smoke test on `windows-latest`, and a Linux interop job running the engine tests against real Dovecot and Stalwart containers (IMAP, POP3, SMTP, ManageSieve).
*Done (ADR 0040):* NSIS template copy (installs to `%LOCALAPPDATA%\Programs\Nail`), per-user registration hooks, portable zip, `deny.toml` (RUSTSEC-2023-0071 exception), CycloneDX SBOMs, CI jobs `desktop`, `installer`, `deny`, `interop` (`scripts/interop/`, own workspace; ManageSieve deferred to M2c). The advisories check and the interop job have not run yet (no network / Docker locally).

#### WP45 — Integration (sequential)
Wire the remote UI with auth into `nail serve` and the desktop app; migrate existing loopback-only behaviour; **Playwright:** pairing a second browser context with a code; passkey login via Playwright's virtual authenticator; password + TOTP login; brute force triggers backoff; session revoke logs the browser out; remote images blocked, loaded through the proxy after "allow sender", a proxy request to 127.0.0.1 or a private IP is refused; CSP blocks an injected inline script; internet mode cannot be enabled; installer build smoke (install to a temp dir, start, uninstall) in CI.
*Done (ADR 0041):* `nail_server::RemoteUiHost` runs the remote UI from the core's settings (off by default; TLS + login; the desktop's own UI stays loopback without login), per-request authorization and grant narrowing in the core (`Service::handle_remote`), remote content rewritten through the `nail-mime` hook and served by the proxy (web route, sandbox origin, desktop `nail-proxy` protocol), mail HTML of the remote UI on the sandbox origin (`MessageBody.render_url`), Trusted Types for `srcdoc`, launch requests in the contract (`mailto:` → composer, `.eml` → "Opened files"; single instance), the M2a restart test with encryption at rest, `nail` portable mode. All listed Playwright scenarios pass (`remote.spec.ts`, `remote-content.spec.ts`, `desktop.spec.ts`); the installer smoke runs in CI.

#### M2b follow-ups (deviations and deferrals)
- **Throttle window**: 7 days because the store resets a count after the window from the *first* failure; a v7 change (window from the last failure) would allow hours.
- **Passkeys** are discoverable-only (no `allowCredentials` for non-resident security keys); `credProtect` is requested, not enforced by the browser (ADR 0041).
- **Login page** is unstyled (runs before the theme); serve the default theme without a session.
- **Render tokens** are bearer capabilities for one message for an hour. *Done in M2c (ADR 0048): bound to the remote session.* Events on remote sockets are filtered per grant since the review fixes (ADR 0042).
- **Remote grants narrowed to some accounts** see no address suggestions (collected addresses are not per account), no rules (rules span every account, like MCP) and a search `total` that counts only the hits found for the page (ADR 0042).
- **Remote content**: the `TargetCheck` re-reads the allow-lists but not whether the message still references the URL. *Done in M2c (ADR 0048): a "load once" scope and a proxy rate limit (per session, or per client address on the sandbox origin).*
- **CSRF key** is per process: browsers fetch a new token after a restart (handled by the UI).
- **Audit growth**: remote audit entries are purged after 90 days with the other history. *Done in M2c (ADR 0048): a retention setting (`audit_retention_days`) and a daily purge.*
- **Installer / CI**: first runs of `deny` (advisories) and `interop` (pin Stalwart / Dovecot images afterwards); offline WebView2 variant, German installer strings, tray and hidden autostart, per-machine MSI (M4), signing.
- **Launch requests**: `webcal:`, `.ics` and `.vcf` answer "not supported yet" until M3; `in-reply-to` of `mailto:` is ignored; a "Choose Nail in Default apps" button (`ms-settings:defaultapps?registeredAppUser=Nail`) is not built.
- **ACME** is wired but untested end to end (needs the network and port 80; internet mode stays locked).

### Wave M2c — Server-side rules and polish

**Status:** WP50–WP55 done (ADRs 0043–0048); review fixes done (ADR 0049: Sieve location and server rules in store v8, renamed / moved / switched-off / deleted scripts removed where they were uploaded, atomic snooze wake + sweep of stranded mail, marked Snoozed folder, one wake notification per account and folder, search paging with capped `from_server` hits and grant-filtered server search, localized audit / import texts, `nail mcp` lock-first, bounded junk training, ManageSieve interop tests, wider a11y scans). Automated suites green (`cargo test --workspace` 1729 tests, Vitest 768, Playwright 65 scenarios incl. `m2c.spec.ts`, the extended `a11y.spec.ts` (all 21 settings tabs, move window, message menu, high-contrast mode; it found and fixed a contrast bug of the selected key's trust badge), the login page scan in `remote.spec.ts` and the desktop smoke test; clippy / fmt / ESLint / tsc / Prettier clean; `cargo deny` not run offline). The M2b follow-ups on render tokens, "load once", the proxy rate limit and audit retention are closed. Next: an independent verify; the interop job against real ManageSieve servers (CI).

#### WP50 — Contract (sequential)
Sieve upload status per rule (server / client / not expressible), vacation settings, junk training requests and classifier status, snooze (until time, wake folder), notification settings (per account / folder / rule, quiet hours), sync policy per folder (full / headers-only / recent N days) and on-demand body fetch, UI context-menu model, toolbar / key / column edit requests, fixed MCP port setting, localized core messages (error codes + Fluent ids + args instead of English text), `RuleFromMessage` request.
*Done (ADR 0043):* `nail_api::localized` (`LocalizedMessage` = Fluent id + args; an optional `localized` sibling on every English diagnostic of the contract, boxed in `ApiError` / `Response::Error`), `sieve` (ManageSieve settings as the `sieve` node of an IMAP account in `accounts.kdl`, rule placement, account state, vacation with validation), `triage` (junk settings / status / verdicts, snooze, notification settings with overrides and quiet hours, `NotificationRequested`), `sync` (folder sync policy view, `MessageSummary.headers_only`, `FetchMessageBodies`, `SearchResults.server`), `menus` (context menus resolved from `layout.kdl`, UI port `features/menus/resolve.ts`), `reset-toolbar` / `reset-key-binding` layout patches, `McpSettings`, `RuleFromMessage`; store v7 (body state, snoozes, junk classifier with hashed tokens, notification overrides, Sieve state); the core serves the store-backed requests (`SyncSieve`, `PreviewSieveScript`, `MarkJunk`, `(Un)SnoozeMessages` answer `unsupported` until WP55); stub crates `nail-sieve` / `nail-triage`, `fluent-bundle` (nail-config `l10n` slot) and `tauri-plugin-notification` built on Windows MSVC; slots for WP51–WP54 incl. per-producer catalogs `core.ftl` / `core-sieve.ftl` / `core-triage.ftl`. Deviation: sync policies stay in `accounts.kdl`; the store keeps the per-message body state.

#### WP51 — Sieve and vacation (parallel)
Compile Sieve-expressible rules to Sieve (RFC 5228 + fileinto, imap4flags, vacation, regex, variables where supported) with a round-trip test harness; ManageSieve client (RFC 5804: STARTTLS, AUTHENTICATE PLAIN / XOAUTH2, PUTSCRIPT, SETACTIVE, CHECKSCRIPT, capability-aware); mock ManageSieve server in `nail-testkit-net`; vacation replies via Sieve `vacation`.
*Done (ADR 0044):* `nail-sieve` compiler with per-rule placement and localized reasons (`core-sieve.ftl`), POSIX ERE translation, parser + decompiler round-trip harness, ManageSieve client (STARTTLS without plaintext fallback, PLAIN / XOAUTH2 / OAUTHBEARER, every RFC 5804 command), one managed script per account that never overwrites the user's scripts (previous script recorded and included), vacation with dates via `date` or applied by Nail; `MockSieveServer` + `validate_script`.

#### WP52 — Spam, snooze, notifications (parallel)
Local Bayesian classifier per account (tokenizer over headers + text, trained by "junk / not junk", honours server Junk flags, never auto-deletes); snooze via IMAP keyword + a hidden "Snoozed" folder with local wake timers (works offline, survives restart); new-mail notifications (Tauri notification plugin on desktop, Web Notifications in the remote UI) with per-account / folder / rule settings and quiet hours, which also makes the rules `notify` / `suppress` actions real.
*Done (ADR 0045):* `nail-triage` (pure logic): tokenizer, Robinson / Fisher classifier with an unsure band and undo, arrival and mark plans that never delete, timer wheel, snooze scheduler (restart catch-up, rebinding by Message-ID), DST-safe local times, notification policy with quiet hours and burst batching; every write returned as a `TriageOp`.

#### WP53 — Engine polish (parallel)
Sync policy `bodies` (headers-only / recent N days) with on-demand body fetch; IMAP `SEARCH` fallback merged into search results for messages without local bodies; a separate replay connection per IMAP account; `text/markdown` alternatives treated as body alternatives in `nail-mime`; Rust-built user-facing strings (config errors, MCP hints, CLI output where user-facing) turned into codes + Fluent ids; fixed MCP port setting; headless `nail mcp` start with a single-instance lock on the data directory; `RuleFromMessage` using the full headers (List-Id).
*Done (ADR 0046):* `nail_config::l10n` (fluent-bundle over every catalog) and localized `ConfigError` / rule issues / MCP / CLI texts; `BodyFetch`, `fetch_body`, `SearchKey` + `search_messages` in `nail-proto-imap`; store body-state accessors and `DataDirLock`; `nail_search::imap` (query → IMAP key); Markdown alternatives; `configured_port`; headless `nail mcp` (`--config-dir`, `--no-start`, `--mcp-port`); `rule_from_message` in `suggest.rs`.

#### WP54 — UI polish and accessibility (parallel)
Right-click context menus rendered from `layout.kdl`; toolbar-order, key-binding and column-order editors in the Customize tab (using the existing write-back); lazy-loaded TipTap / CodeMirror; inline images in reopened HTML drafts; UI for vacation, junk, snooze, notifications, sync policy; accessibility pass (axe on every window and dialog, full keyboard navigation, focus management, screen-reader labels, a high-contrast theme shipped as an example theme, reduced motion).
*Done (ADR 0047):* `features/menus` (right-click, Shift+F10 / menu key through `menu.context`), Customize editors (`toolbar-editor`, `key-binding-editor`, `column-editor`), triage / Sieve / sync features and widgets, lazy editor chunks (main chunk ~474 kB), inline images via `GetDraftAttachment`, themed login page, axe in Vitest, `defaults/themes/high-contrast.kdl`, reduced motion.

#### WP55 — Integration (sequential)
Wire all of the above; **Playwright:** a rule shows as "server" and its Sieve script lands on the mock ManageSieve server; vacation reply script uploaded; marking junk trains the classifier and the next similar message lands in Junk; snooze hides a message and it returns at the wake time (clock controlled); a new-mail notification fires and quiet hours suppress it; headers-only folder fetches the body on open; server-side search finds a message without a local body; right-click menu on a message works; rebinding a key writes `layout.kdl` and works; axe clean on every window.
*Done (ADR 0048):* Sieve upload task (digest of inputs, server rules skipped on arrival, removal when switched off, recompile for vacation dates), junk on arrival and `MarkJunk` (`Store::untrain_junk`), snooze scheduler + wake task + hidden Snoozed folder (created by the first move), notification policy + batching (`NotificationRequested`; OS notifications in the desktop shell, Web Notifications in the browser), headers-only sync + on-demand bodies + trimmed aged bodies, server search fallback, a separate replay connection, localized core messages end to end, desktop MCP port from the settings + data-dir lock, `GetDraftAttachment`, M2b follow-ups (session-bound render tokens, "load once", proxy rate limit, audit retention); an adjustable clock and demo-multi controls (`clock-*`, `sieve-scripts`) for the Playwright scenarios (`m2c.spec.ts`, extended `a11y.spec.ts`, `remote-content.spec.ts`). Status: integrated, all suites green; review fixes in ADR 0049; verify pending.

#### M2c follow-ups (deviations and deferrals)
- **Sieve**: the ManageSieve interop tests exist (ADR 0049; offline against the mock) but have not run against Dovecot Pigeonhole and Stalwart yet (the mock does not execute scripts; `:regex` and capability re-issue after login may differ; the Pigeonhole config is unverified); the status digest is in memory (an account shows `pending` after a restart until the start-up upload; the server rules themselves are in the store); a rule edited while uploads fail runs in its old form on the server; `include :personal` names are written without the `${` escape; a dedicated reason for non-ASCII case-insensitive regexes; per-rule placement badges in the rule manager.
- **Junk**: unsure verdicts are stored as `junk: false` with a score; `ham_cutoff` is not a user setting; no NFKC normalization of tokens; training rows of v7 keyed by the bare `Message-ID` are not re-keyed.
- **Snooze**: snoozes are per installation; two Nail installations on one account whose hidden folders share a name return each other's snoozed mail to the Inbox (sweep).
- **Notifications**: quiet hours use the OS time zone (the remote UI may want a per-user zone); the WebView2 behaviour of `contextmenu` after Shift+F10 and OS notification look on the installed app belong to the desktop smoke test.
- **Sync**: the server search does not filter inexact hits (`has:attachment`, `filename:`) locally; replay and sync stay sequenced in one loop (the replay connection only keeps the main connection's mailbox).
- **Localized messages**: library texts (KDL parser, OS errors, regex errors, search parser) stay English `detail` arguments; MCP audit entries map English details back (a store column would be cleaner); headless `nail mcp` does not read the Windows UI language; German catalogs are empty.
- **Remote**: the proxy limit is per session on the app origin but per client address on the sandbox origin; "load once" lapses after 15 minutes.
- **UI**: `folder-tree.account` has no page element; the application menu (`menu "app"`) is unused in the web UI; forwards of HTML mail with inline images do not load previews yet.

### Wave M2d — Plug-in preview

**Status:** WP60–WP65 done (ADRs 0050–0055). The plug-in preview runs end to end: packages previewed and installed with the approved hash (kept packed in `<data-dir>/plugins`, verified at every start), one sandboxed QuickJS runtime per plug-in inside the core behind the permission gate, commands in toolbars / menus / keys, dialogs, notifications (OS ones on the desktop, respecting quiet hours), panels, storage, HTTP through the core, placements written to `layout.kdl` through the merge flow, the plug-in manager, `nail plugin dev` with hot reload; M2c carry-overs (turn-bounded UI tests, dates in the UI locale, localized crypto / message-file texts, the a11y sweep, snooze restarts). Suites green (`cargo test --workspace` 1893, Vitest 877 + SDK 49 + template/example 21, Playwright 72 scenarios incl. `plugins.spec.ts`, `a11y-windows.spec.ts` and the desktop smoke test). Reviews done; review fixes (ADR 0056): plug-in search is header-only without `mail.read.body` and never finds encrypted mail while the index holds decrypted text; a disabled / uninstalled plug-in is interrupted at once and runs nothing still queued; every log line a plug-in causes is capped per call and per minute, the core's log queue is bounded; `nail-pgp` problems have their own texts (`pgp-problem-*`); OS notifications name the plug-in first; `nail plugin dev` connect errors localized. Independent verify passed (the M2 automated gate, section 7); its two findings are fixed post-M2.

#### WP60 — Contract (sequential)
Plug-in manifest (id, name, version, apiVersion, main, permissions, contributes: commands, placements, keys, settings schema, locales), permission names for the preview (`mail.read`, `mail.read.body`, `calendar.read` placeholder, `ui.dialogs`, `notifications`, `storage`, `network:<domains>`), declarative UI component set (text, markdown, input, textarea, select, checkbox, date, contact picker, folder picker, message list, table, button, progress, image) as typed trees, plug-in admin requests (install from file/URL, enable, disable, uninstall, list, grants, logs).

*Status:* done — ADR 0050. `nail_api::plugins` (manifest + validation, permissions and grants, component trees with `validate_tree`, dialogs, notifications, command contexts, the plug-in mail view), admin and runtime requests / events (also dev loading, settings, placements, panels), store v9 (plug-ins, grants, settings, KV storage with quotas, logs; sealed at rest), `plugin-panel` layout widget, stub crate `nail-plugins` (rquickjs, zip), slots for WP61–WP63, `packages/*` npm workspaces with generated plug-in types. The core serves the store-backed administration; host requests answer `unsupported` until WP64.

#### WP61 — Plug-in host (parallel)
`nail-plugins`: one QuickJS runtime per plug-in via `rquickjs` (memory limit, interrupt-based CPU budget, no filesystem / process / sockets), permission gate in front of the Client API, API bindings for the preview (`nail.mail` read, `nail.ui.showDialog` / `notify` / `registerCommand`, `nail.storage`, `nail.http.fetch` restricted to declared domains through the core, `nail.settings`, `nail.log`), `.nailplugin` archive format (manifest.json + bundled JS + assets + locales), install / uninstall / enable, `nail plugin dev <folder>` with hot reload and a log console. Tests for isolation, limits and denied permissions.

*Done (ADR 0051):* `PluginHost` (thread + runtime per plug-in, CPU budget per turn, 64 MiB heap, crash isolation), the gate (`mail.read` per account and re-filtered, `mail.read.body` never for encrypted mail, `network:` rules), `prelude.js`, packages kept packed and hash-checked, dev reload, plug-in catalogs.

#### WP62 — Plug-in UI (parallel)
React renderer for the declarative component set (all parts themeable, e.g. `plugin.<id>.*`), plug-in dialogs (modal and non-modal, returning values), toolbar / menu / key placements, plug-in panels as layout widgets (`plugin-panel "<id>.<name>"`), install dialog showing permissions, plug-in manager (enable/disable, grants, logs), placements proposed on install through the merge dialog.

*Done (ADR 0052):* tree renderer (no plug-in HTML; Markdown parsed into elements), modal / non-modal dialogs per `client_id`, plug-in commands as layout actions (`usePluginLayout`), `plugin-panel`, toasts, install dialog with Allow / Deny and account limits, placements step, plug-in manager, mock host.

#### WP63 — SDK and example (parallel)
`@nail/plugin-api` TypeScript package (types generated from the Rust contract, a tiny JSX runtime producing the component trees), a starter template package (`npm create nail-plugin` equivalent inside the repo: `packages/create-nail-plugin`), the design's "Follow up" example plug-in (adapted to read-only: a button + dialog that stores a reminder in plug-in storage and shows a notification), `userDocs/en/plugins/` (getting started, API reference generated from the types, manifest reference, permissions).

*Done (ADR 0053):* `@nail/plugin-api` (generated types, `nail` wrapper, `definePlugin`, JSX runtime, `createTestHost`, `checkManifest` / `packPlugin` / `nail-plugin`), `create-nail-plugin`, the "Follow up" example, `userDocs/en/plugins/` (reference sections generated).

#### WP65 — M2c carry-overs (parallel)
Deterministic full-app UI tests (turn-bounded waits), dates in the UI locale everywhere (`i18n/format.ts`), localized OpenPGP and message-file texts (`core-crypto.ftl`), an axe sweep of every window / tab / dialog in three modes, snooze across restarts.

*Done (ADR 0054).*

#### WP64 — Integration (sequential)
Wire host + UI + SDK; **Playwright:** install the example plug-in (permissions dialog), its toolbar button appears in the reader, clicking opens its dialog, submitting stores data and shows a notification, the placement is written to `layout.kdl`; a plug-in without `mail.read.body` gets a permission error; a runaway plug-in (infinite loop) is stopped and flagged while the UI stays responsive; `nail plugin dev` hot reload works.

*Done (ADR 0055):* `nail-core` `plugin_host.rs` (one host, `PluginServices` over store / search, never decrypted bodies, ordered log writer, crash errors recorded, quiet hours for `system` notifications, dev manifests followed), `plugin_http.rs` (SSRF-safe fetch, also for URL installs), every plug-in request (commands / UI actions answer `Done` at once); `nail-config` placement writer + installed plug-ins in merge conflicts, `plugin-manager` widget, pre-M2d `plugin.<id>.<name>` refused; defaults (Plug-ins settings tab, actions, plug-in theme parts); `nail plugin dev` over `nail serve`'s WebSocket (`ui-endpoint.json`); OS notifications for `system` plug-in notifications; host fix: rejections awaited in the same turn are not logged as unhandled. Playwright `e2e/tests/plugins.spec.ts` (all scenarios above; the example is built by the global setup outside the repo) and the install dialog in `a11y-windows.spec.ts`.

#### M2d follow-ups (deviations and deferrals)
- **`nail plugin dev`** reaches only `nail serve` (its `ui-endpoint.json` / `--url`); the desktop app loads development folders from the plug-in manager (no local endpoint for the CLI yet).
- **Host limits**: the CPU budget is per turn; an async chain that yields between turns can use many budgets; no cumulative quota or wall-clock limit for promises left pending (only `activate` has 30 s). A stopped runtime is interrupted at once (ADR 0056); API calls it started before the stop still finish in the core (their results are dropped).
- **Search** (ADR 0056): with decrypted indexing on, plug-in searches leave out every encrypted message, even hits on its headers; header-only search cannot tell `from:` display names from addresses any better than the UI search.
- **API gaps**: `nail.http.fetch` bodies are UTF-8 text (no binary / streaming); no runtime API for a plug-in's own catalogs (`nail.l10n`), so the example's run-time texts are English literals; `Cc` only in `nail.mail.get` (lists and searches carry none); `nail.mail.search` is local-index only.
- **Panel actions**: values sent by panel buttons are not filtered against the last rendered panel tree (dialog values are).
- **Routing**: plug-in dialog / notification events are broadcast to every connected UI (full-grant remote sessions included) and filtered by `client_id` in the UI, not in the transport.
- **Packages** stay packed and are read into memory (≤ 20 MiB per plug-in) instead of being unpacked (ADR 0051 deviation from the stub's sketch).
- **SDK**: `@nail/plugin-api` is private (scaffolds outside the repo use a `file:` path); `src/api.ts` mirrors `prelude.js` by hand; `checkManifest` / `checkTree` cover the main rules only (the host stays authoritative).
- **UI**: Markdown in plug-in trees is a CommonMark subset (no tables or nested lists); the merge dialog cannot start from a given text, so the placements step reuses its requests; how `window.open` of Markdown links behaves in the Tauri webview is unverified; native date inputs follow the browser / OS language.
- **Tests**: the full-app suites other than `App.flows.test.tsx` still use 1 s `findBy*` waits; `cargo deny` advisories need the network (not run).

### Carried items not in M2
PKCS#11 smartcards (M4), revocation checking for S/MIME (later), DNSSEC-validated SRV and a public-suffix list for discovery (later), atomic cross-account moves (later; current behaviour can duplicate, never loses).

## 8. Milestone M3 — PIM, Gmail API and Exchange (detailed)

**Automated gate:** all suites green, including the Playwright scenarios listed per wave, and the interop CI job extended with a real CalDAV/CardDAV server (Radicale, plus Stalwart's DAV if available) next to Dovecot and Stalwart mail.
**Human gate (design: "interop suite passes on Dovecot, Stalwart, Radicale, Exchange Online and on-prem"):** the Exchange Online, Exchange Server on-premises and Google parts need real tenants and accounts, so they are owner tasks; the automated suite covers them with recorded fixtures and mock servers.

**Scope decisions for M3**
- Same licence rule as M2 (no GPL/LGPL/AGPL; check every new crate, e.g. iCalendar/vCard parsers).
- **No non-loopback sockets** (ADR from the firewall fix): every new protocol engine and mock server binds loopback only; DNS goes through the OS resolver.
- **Push for Graph and Gmail on a desktop client:** both providers deliver push through webhooks / Pub/Sub, which a desktop app cannot receive without a server. M3 uses delta queries / history polling with adaptive intervals (and EWS streaming notifications, which do work from a client). A push relay stays a later option.
- **Exchange on-premises authentication** uses Windows SSPI (NTLM / Kerberos / Negotiate) through the `windows` crate, so no NTLM implementation of our own; Basic over TLS and OAuth2 for hybrid remain options.
- **Google calendar and contacts** stay on CalDAV/CardDAV (decided in the design review); only mail moves to the Gmail API.
- MCP gets read-only calendar, contact and task resources and tools in M3; writes stay in M4.

M3 runs as three waves on the existing wave workflow (`.claude/workflows/nail-wave.js`).

### Wave M3a — Calendar, contacts and tasks (local, ICS, CalDAV/CardDAV)

#### WP70 — Contract (sequential)
Calendar, event (incl. recurrence rules, exceptions, overrides, attendees, alarms, time zones, conferencing links, attachments), contact (vCard 3/4 fields, groups, photos, multiple addresses), task (VTODO: due, priority, status, subtasks, recurrence) models; calendar/contact/task sources (local, ICS subscription, CalDAV, CardDAV); requests: list/create/update/delete for each, expand event instances in a range, search contacts, merge contacts, import `.ics` / `.vcf`, create event / task from email, reminders (snooze, dismiss); events for changes and reminders. Store migration for calendars, events, instances cache, contacts, groups, tasks, sources and sync state. Feature slots for UI.

*Status:* done — ADR 0058. `nail_api::pim` (calendars, events as series with overrides and edit scopes, recurrence, attendees, alarms, time zones, conferencing links, attachments; contacts with vCard 3/4 fields, groups, photos, merge links; tasks; reminders; import; read-only MCP payloads), 52 requests and 9 events, sources in `accounts.kdl` (`calendar-subscription`, `caldav`, `carddav`; read / write / schema in nail-config), store v10, stub crates `nail-ical` (calcard) and `nail-dav`, slots for WP71–WP74. The core answers every PIM request `unsupported` until WP75.

#### WP71 — iCalendar, vCard and recurrence (parallel)
`nail-ical`: parse and serialize iCalendar (RFC 5545) and vCard 3.0/4.0 losslessly (unknown properties kept), recurrence expansion (RRULE, RDATE, EXDATE, RECURRENCE-ID overrides; a maintained crate with an acceptable licence, or our own), time zones (VTIMEZONE + IANA via `chrono-tz`, DST edge cases), floating and all-day events, VALARM computation; ICS feed parsing for subscriptions. Property tests and a corpus of real-world calendar files (Google, Outlook, Apple, Thunderbird exports).

*Status:* done — ADR 0059. `nail-ical` with its own lossless content-line layer and patching writers (calcard only for the Windows zone table and `VTIMEZONE` generation), its own recurrence engine (every `BY*` part, `BYSETPOS`, `BYWEEKNO` / `WKST`; RFC 5545 examples as tests, property-compared with calcard's iterator), instances with overrides and `THISANDFUTURE`, edit scopes, zones (IANA / Windows / CDO / custom `VTIMEZONE` matching), alarms, feeds, import previews; a synthesized corpus of Google / Outlook / Apple / Thunderbird exports.

#### WP72 — CalDAV / CardDAV client (parallel)
`nail-dav`: discovery (well-known URLs, RFC 6764 SRV/TXT through the OS resolver, principal and home sets), PROPFIND, REPORT (calendar-query, calendar-multiget, addressbook-query/multiget), `sync-collection` (RFC 6578) with ctag/etag fallback, conditional PUT/DELETE with ETags, conflict handling (server wins, keep the local copy as a conflict), auth (Basic over TLS, OAuth2 bearer for Google); a mock CalDAV/CardDAV server in `nail-testkit-net`; Radicale added to the interop CI job.

*Status:* done — ADR 0060. `nail-dav` as specified (`DavClient` with Basic over TLS / bearer / OAuth2 with one refresh, https-or-loopback URLs, own redirects; discovery; PROPFIND / REPORTs; `sync_collection` with token reset and ctag / ETag fallback; conditional writes, server wins; MKCALENDAR / MKCOL / PROPPATCH); `MockDavServer` in `nail-testkit-net`; Radicale in `scripts/interop` and the CI job (verified locally against Radicale 3.8.1). Discovery uses SRV + `/.well-known/` only (no TXT), per ADR 0058.

#### WP73 — PIM store, sync engine pieces and reminders (parallel)
Store APIs for all PIM tables, instance cache for fast range queries, PIM search (Tantivy fields for events, contacts, tasks), the reminder scheduler (alarms across time zones and restarts, using the triage scheduler's clock injection), collected-addresses book, duplicate detection and merge with links to sources, address autocomplete ranking from contacts + collected + recent.

*Status:* done — ADR 0061. `nail-store` `src/pim/` (sources, collections, items with the PIM queue in one transaction, sync merge with conflict copies, instance cache in 90-day chunks, contacts / duplicates / merge / Collected book / `suggest_recipients`, reminders), `nail-search` `PimIndex`, `nail-triage` `reminders.rs` (planning across zones and DST, scheduler with restart catch-up, snooze / dismiss). No migration needed.

#### WP74 — PIM UI (parallel)
Calendar views (day, multi-day, week, work week, month, year, agenda, free-time timeline) as layout widgets, mini-month and today's agenda widgets, event editor (recurrence editor incl. "this / this and following / all", attendees, alarms, time zones, secondary zone), contacts list / card / editor / groups / merge dialog, task list widget and editor, drag to reschedule, keyboard navigation; all themeable, all strings in Fluent catalogs, axe clean.

*Status:* done — ADR 0062. `ui/src/features/pim/`: the eight calendar views, mini-month, agenda, `calendar-list`, event editor (React Aria date / time fields in the UI language, recurrence, scopes, attendees, reminders, zones), contacts list / card / editor / groups / merge, task tree and editor, reminder toasts, import and sources dialogs; mock of all 52 requests; `PIM_BASE_CSS`. Not built: drag across empty hours to create, recent mail on the contact card.

#### WP75 — Integration (sequential)
Core wiring (sources, CalDAV/CardDAV sync engines with polling, local calendars, ICS subscriptions, reminders through the notification path, import from launch arguments `.ics` / `.vcf` / `webcal:`, "create event / task from email", address autocomplete in the composer from contacts), MCP read resources/tools for calendars, events, contacts, tasks (with grants), plug-in `calendar.read` permission made real, default layout with a calendar window. **Playwright:** create a recurring event and edit "this and following"; event synced to the mock CalDAV server and a server-side change appears; ICS subscription shows events read-only; reminder fires at the controlled clock time; contact created, synced to mock CardDAV, used in composer autocomplete; duplicate contacts merged; task with subtasks; open a `.ics` file from the launch argument; MCP client lists events after a grant.

*Status:* done — ADR 0063. `nail-core/src/pim/` serves all 52 requests (edit scopes through `nail-ical::edit`, raw text patched on every write, validation incl. zones); source engines per enabled subscription / DAV source (polling, wake on writes and "Sync", backoff while offline, `auth-error`), ICS with conditional GET, CalDAV / CardDAV discovery + sync + PIM queue replay (412 → server wins, conflict copy); reminders on the triage clock through `ReminderDue` and the notification path (quiet hours: no sound); in-memory PIM index; imports (desktop file or upload, update by `UID`); event / task from email; composer autocomplete from contacts + Collected + recent (sent mail fills the Collected book). MCP resources / tools for calendars, events, contacts, tasks with grants; plug-in `calendar.read` (`nail.calendar.events`); `nail serve --launch` (parser moved to `nail_core::launch_args`) and the UI's import / subscribe dialogs for launch requests; default layout with `calendar` / `contacts` / `tasks` windows, toolbar and keys; demo-multi with a mock CalDAV / CardDAV server, ICS feeds and controls. Playwright: `e2e/tests/pim.spec.ts` (all nine scenarios) and the a11y sweep over the PIM windows and dialogs. Contract additions: `EventOverride.this_and_future`, `PluginCalendarEvent`. The automated gate's interop part (Radicale in CI) is configured; the job itself is unproven until the first CI run.

*Review fixes (ADR 0064):* `nail-ical` keeps at most 32 nesting levels (deep `BEGIN`s crashed the process on drop); recurrence iteration jumps ahead to the range (exact `COUNT`s where every period yields one occurrence), `MINUTELY` / `SECONDLY` rules yield at most 2,000 occurrences, a scan budget per request and per reminder pass, the instance cache expands outside the store lock; iCalendar / vCard parsing is linear (hash-map dedupe, `VTIMEZONE` index) and feeds parse in `spawn_blocking`; `DavClient` sends the login only to the requested origin (`RedirectNeedsLogin` otherwise); subscriptions are fetched SSRF-safe (`pim/feed_http.rs`: https only, public addresses only, pinned, redirects followed and checked, size capped while streaming; loopback `http:` only with `ServiceOptions::loopback_feeds`); reminders: none for sub-hourly series, 3 late + 20 ahead per item, more than 3 at once = one summary notification; changed subscriptions (`alarms`, name, colour, URL) take effect on the next fetch; the feed's refresh interval is used; Collected contacts from sent mail are indexed and announced. New core tests cover the 412 conflict path with `ResolvePimConflict` both ways.

**M3a follow-ups / deferrals** (ADR 0063, 0064): a subscription does not store the URL a feed redirected to (it keeps the configured one); the feed's refresh interval is remembered per run only (after a restart until the next full fetch); per-source narrowing of PIM answers for remote sessions (they still need a grant for every account); RFC 9074 snooze alarms written to the server (snooze is local; dismiss writes `ACKNOWLEDGED`); replanning reminders on an OS time-zone change (hourly today); a conflict choice in the event editor (the contact card has one); `RDATE;VALUE=PERIOD` ends; DNS TXT discovery paths (needs a TXT lookup in `nail-discovery`); creating events by dragging over empty hours; recent mail on the contact card; context menus for calendar / contact / task items; German translations of `pim.ftl`; Google CalDAV / CardDAV need the calendar and contacts scopes on the Google OAuth client (owner task); a sealed on-disk PIM index if the in-memory one becomes too slow to build.

### Wave M3b — Invitations, scheduling and the Gmail API engine

#### WP80 — Contract (sequential)
iTIP methods and attendee states, invitation card data on messages, scheduling requests (respond, counter, send update, cancel, forward invite), free/busy queries, Gmail account kind and Gmail-specific fields (label ids, thread ids, categories), provider-neutral sync engine trait additions needed by API engines.

*Status:* done — ADR 0065. `nail_api::scheduling` (iTIP methods, the invitation card on `MessageBody.invitation` with role / state / my status / sequence / conflicts / actions / issues, answers with comments, counter-proposals, refresh, forward, organizer send / cancel / decline counter, apply, scheduling settings, free/busy with sources and merged busy periods) and `nail_api::gmail` (the `gmail` account kind in `accounts.kdl`, labels ↔ folders / tags, thread ids, categories, label management); `nail_api::engine` gains `MailApiEngine`, `AdaptivePoll`, `FreeBusyProvider` and `EngineError::retry_after_secs`. 16 requests, 3 events. Store v11 (remote ids, scheduling state, received / sent iTIP messages, Gmail sync state / labels / per-message labels / label queue) with store APIs. Stub crates `nail-itip` and `nail-proto-gmail` (with the loopback mock server under the `testing` feature); slots for WP81–WP84. The core answers every M3b request `unsupported` until WP85; a Gmail account's engine reports `unsupported`.

#### WP81 — iTIP / iMIP (parallel)
`nail-itip`: iTIP (RFC 5546) processing — REQUEST, REPLY, CANCEL, COUNTER, DECLINECOUNTER, REFRESH, ADD; iMIP (RFC 6047) MIME building and parsing; organizer/attendee logic, sequence and DTSTAMP handling, conflict detection against the user's calendars; CalDAV auto-scheduling (RFC 6638) detection so the client does not send duplicate mail when the server schedules.

#### WP82 — Gmail API engine (parallel)
`nail-proto-gmail`: OAuth (restricted scopes), `users.messages` list/get (raw MIME) / modify / trash / send, `users.history.list` incremental sync with history-id reset handling, labels ↔ Nail tags and folders, threads, batch requests, quotas with backoff, attachments; adaptive polling (no Pub/Sub, see scope decisions); a mock Gmail REST server in `nail-testkit-net` with recorded-response fixtures.

#### WP83 — Free/busy and scheduling transport (parallel)
CalDAV scheduling (outbox POST for free/busy, schedule-inbox processing), VFREEBUSY building/parsing, free/busy aggregation across the user's own calendars, room/resource placeholders for Exchange (filled in M3c).

#### WP84 — Invitations and Gmail UI (parallel)
Invitation card in the reader (Accept / Tentative / Decline / Propose new time, conflicts preview, RSVP comment), attendee picker with free/busy timeline, update/cancel flows for organizers, Gmail label management and label-based views, Gmail categories as filterable property.

#### WP85 — Integration (sequential)
Wire iTIP (incoming invites update calendars, replies update attendee status, outgoing invites via iMIP or server scheduling), Gmail accounts end to end (setup via OAuth with the Gmail API, sync, send, labels, search), free/busy in the event editor. **Playwright:** receive an invitation and accept it (reply on the mock SMTP, event in the calendar); organizer sends an update and the attendee's event changes; cancel removes the event; counter-proposal flow; free/busy shows busy blocks of another demo account on the mock CalDAV server; a Gmail account (mock Gmail server) syncs, applies a label, archives, sends, and picks up a server-side change via history polling.

### Wave M3c — Microsoft Exchange (Graph and EWS)

#### WP90 — Contract (sequential)
Exchange account kind (Online via Graph, on-premises via EWS, IMAP fallback), shared mailboxes and delegate access, categories ↔ tags mapping, Focused Inbox property, server-side inbox rules and automatic replies (out of office) for Exchange, meeting rooms and room lists, Microsoft To Do task lists.

#### WP91 — Microsoft Graph engine (parallel)
`nail-proto-graph`: OAuth (Microsoft identity platform), mail (delta queries per folder, MIME via `$value`, send via MIME, move/flag/categories, attachments), calendar (events + delta, instances, iTIP via Graph's meeting APIs), contacts, To Do tasks, mailbox settings (automatic replies), inbox rules, Focused Inbox, shared mailboxes; throttling (429 + Retry-After), batching; a mock Graph server with recorded fixtures.

#### WP92 — EWS engine (parallel)
`nail-proto-ews`: SOAP client (`quick-xml`), Autodiscover (POX + v2 JSON), SSPI Negotiate/NTLM/Kerberos via the `windows` crate, Basic over TLS, OAuth2 for hybrid; `SyncFolderHierarchy` / `SyncFolderItems`, `GetItem` with `MimeContent`, `CreateItem` / `UpdateItem` / `MoveItem` / `DeleteItem`, streaming notifications for push, calendar (CalendarView, meeting responses), contacts, tasks, OOF settings, inbox rules, room lists / rooms, delegates and shared mailboxes; a mock EWS server with recorded fixtures.

#### WP93 — Exchange common layer (parallel)
Mapping between Exchange concepts and Nail's (categories, Focused Inbox, rules ↔ Nail rules where expressible, OOF ↔ vacation settings, To Do ↔ tasks, rooms in the free/busy picker), account setup via Autodiscover with engine choice (Graph for Exchange Online, EWS for on-premises), migration notes from the design (EWS retirement in Exchange Online).

#### WP94 — Exchange UI (parallel)
Shared mailboxes and delegate folders in the folder tree, room finder in the event editor, out-of-office editor for Exchange accounts, Exchange rule placement badges, Focused Inbox toggle/view, To Do lists in the task widget.

#### WP95 — Integration (sequential)
Wire both engines into the core's account manager, sync engines, op replayers, PIM sync and rules; MCP coverage for Exchange data. **Playwright (against the mock Graph and mock EWS servers):** add an Exchange Online account and an on-premises account via Autodiscover; mail sync, flag, move, categories as tags, send; calendar event created and a meeting invitation answered; a shared mailbox appears; out-of-office set and read back; a Nail rule placed as an Exchange inbox rule; room booked through the room finder; To Do task synced. Update the interop job documentation for the owner-run Exchange/Google checks.

### Owner tasks for M3
- An Exchange Online test tenant (Microsoft 365 developer program or similar) and access to an Exchange Server on-premises (2019 / Subscription Edition) for the human interop gate.
- A Google test account for Gmail API and Google CalDAV/CardDAV, and the Google OAuth client with the restricted Gmail scope.

## 9. Later milestones (outline; each gets a detailed section before it starts)

- **M2:** detailed in section 7.
- **M3:** detailed in section 8.
- **M4 Agents and release:** MCP write tools with grants/confirm/audit, full plug-in system (QuickJS), MAPI provider, MSI + winget + updater, EV signing, import/export, backup.

## 10. How milestones are executed (orchestration)

Agents: Opus 5.5 at medium effort, coordinated by a workflow. M0 ran as the sequence below; each M1 wave uses the same shape (contract, then parallel packages in disjoint directories, then integrate, two reviews, fix and an independent verify).

1. **Scaffold** (1 agent, sequential): WP0 incl. contract; commits.
2. **Build** (4 agents in parallel, disjoint directories, separate cargo target dirs, no git writes): WP1 · WP2 · WP3 · WP4.
3. **Integrate** (1 agent): WP5 + WP6 + WP7; runs the full test suites; commits.
4. **Review** (2 agents in parallel): correctness/security review and design-conformance/test-coverage review against this plan.
5. **Fix** (1 agent): fixes confirmed findings, re-runs everything, commits.
6. **Verify** (1 agent): clean full run of all suites and the M0 gate checklist; reports status.
