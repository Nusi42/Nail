# Nail — Implementation Plan

Status: 2026-09-29 · Source of truth for *what* to build: [`design.md`](design.md). This file says *how* and *in which order*.

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

## 7. Later milestones (outline; each gets a detailed section before it starts)

- **M2 Security, rules, polish:** OpenPGP (Sequoia), Autocrypt, S/MIME, Sieve/ManageSieve, spam classifier, snooze/templates polish, accessibility pass, remote web UI with pairing + passkeys + hardening, plug-in preview (commands, dialogs, read access), basic NSIS installer build, the deferred M1 follow-ups (see .claude/memory), and localizing user-facing strings built in Rust (config error messages, MCP denial hints) via Fluent.
- **M3 Exchange, Gmail API and PIM:** Graph + EWS engines, Gmail API engine, CalDAV/CardDAV, calendar views, iTIP/iMIP, contacts, tasks, interop suite.
- **M4 Agents and release:** MCP write tools with grants/confirm/audit, full plug-in system (QuickJS), MAPI provider, MSI + winget + updater, EV signing, import/export, backup.

## 8. How milestones are executed (orchestration)

Agents: Opus 5.5 at medium effort, coordinated by a workflow. M0 ran as the sequence below; each M1 wave uses the same shape (contract, then parallel packages in disjoint directories, then integrate, two reviews, fix and an independent verify).

1. **Scaffold** (1 agent, sequential): WP0 incl. contract; commits.
2. **Build** (4 agents in parallel, disjoint directories, separate cargo target dirs, no git writes): WP1 · WP2 · WP3 · WP4.
3. **Integrate** (1 agent): WP5 + WP6 + WP7; runs the full test suites; commits.
4. **Review** (2 agents in parallel): correctness/security review and design-conformance/test-coverage review against this plan.
5. **Fix** (1 agent): fixes confirmed findings, re-runs everything, commits.
6. **Verify** (1 agent): clean full run of all suites and the M0 gate checklist; reports status.
