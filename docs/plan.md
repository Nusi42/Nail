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

## 6. Later milestones (outline; each gets a detailed section like M0 before it starts)

- **M1 Daily-driver mail:** full IMAP (IDLE, CONDSTORE/QRESYNC, MOVE, flags write), SMTP submission, POP3, OAuth2 (Gmail API path, Microsoft), autoconfig, multi-account + virtual accounts, Tantivy search + query language, threading (JWZ), composer (TipTap HTML, CodeMirror Markdown, comrak rendering, CSS inlining), client-side rules, read-only MCP server (`rmcp`), UI inspector, write-back of UI changes into `layout.kdl`, merge dialog, plug-in API preview.
- **M2 Security, rules, polish:** OpenPGP (Sequoia), Autocrypt, S/MIME, Sieve/ManageSieve, spam classifier, snooze/send later/templates, accessibility pass, remote web UI with pairing + passkeys + hardening, basic NSIS installer build.
- **M3 Exchange and PIM:** Graph + EWS engines, CalDAV/CardDAV, calendar views, iTIP/iMIP, contacts, tasks, interop suite.
- **M4 Agents and release:** MCP write tools with grants/confirm/audit, full plug-in system (QuickJS), MAPI provider, MSI + winget + updater, EV signing, import/export, backup.

## 7. How M0 is executed (orchestration)

Agents: Opus 5.5 at medium effort, coordinated by a workflow.

1. **Scaffold** (1 agent, sequential): WP0 incl. contract; commits.
2. **Build** (4 agents in parallel, disjoint directories, separate cargo target dirs, no git writes): WP1 · WP2 · WP3 · WP4.
3. **Integrate** (1 agent): WP5 + WP6 + WP7; runs the full test suites; commits.
4. **Review** (2 agents in parallel): correctness/security review and design-conformance/test-coverage review against this plan.
5. **Fix** (1 agent): fixes confirmed findings, re-runs everything, commits.
6. **Verify** (1 agent): clean full run of all suites and the M0 gate checklist; reports status.
