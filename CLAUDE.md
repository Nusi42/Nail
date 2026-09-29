# Nail — project guide for Claude

Nail (working name) is an email + PIM client: Rust core, TypeScript/React UI, Tauri 2 desktop shell, a web-server mode that serves the same UI, and MCP access to all data.

- What to build: `docs/design.md` (exported from the living doc linked in its header).
- How and in which order: `docs/plan.md` (milestones M0–M4, work packages, gates).
- Decisions and deviations: `docs/adr/NNNN-title.md`.
- Project memory: `.claude/memory/` (see "Memory" below).

## Non-negotiable rules

- **Tests from day 1.** Every change ships with unit tests; every user-visible behaviour with a Playwright scenario. Never report work as done while any suite is red or tests were skipped.
- **Docs split.** Internal docs → `docs/`. User-facing manuals and references → `userDocs/<lang>/` (`en` written, `de` empty for now).
- **Languages.** No literal user-visible strings in React or Rust. Strings live in Fluent catalogs `locales/en/*.ftl` (complete) and `locales/de/*.ftl` (same files, empty). Missing keys fall back to English per key.
- **Everything themeable.** React components never use inline styles or literal class names. They emit `data-part` attributes; styling comes only from the compiled theme (`theme.kdl`). Structure comes only from `layout.kdl`.
- **The core owns data.** The UI talks only to `NailClient`; business logic lives in Rust. Every write goes through the operation queue.
- **Windows first.** macOS, Linux and mobile come later.
- **Record decisions.** New library choices or deviations from the design get an ADR.
- **Keep this file current.** When commands, layout or conventions change, update this file in the same commit.

## Tooling facts

- Rust 1.97 (MSVC), Node 26 + npm 11. **npm workspaces, not pnpm** (pnpm is not installed). No docker locally: integration tests use in-process mock servers from `crates/nail-testkit`.
- The repo lives in OneDrive. Build output must stay outside it: a local, gitignored `.cargo/config.toml` sets `target-dir` to `%LOCALAPPDATA%\nail-build\target`. When running several cargo processes in parallel, give each its own `CARGO_TARGET_DIR`.
- Config files are KDL v2.

## Layout

```
crates/nail-api        shared types + Request/Response/Event protocol (ts-rs → ui/src/api/generated);
                       engine traits (SecretStore, OutgoingTransport, AccessTokenProvider; Rust only);
                       M1b: rich drafts, rules, mcp, context (UI context), customize (ADR 0022)
crates/nail-config     theme.kdl / layout.kdl / accounts.kdl parsing, defaults, schema, watcher,
                       format-preserving accounts.kdl writer (AccountsDocument); widget kinds per
                       feature in src/widgets/; rules.kdl in src/rules.rs (WP22)
crates/nail-store      SQLite metadata (v4), blob store, per-account operation queue with sync-side
                       merge, threading (JWZ), drafts/outbox + attachments, search filters (ADR 0015);
                       v4: collected addresses, rule runs (undo), MCP clients/grants/audit, saved searches
crates/nail-mime       MIME parsing, HTML sanitizing, top-level header list (rules)
crates/nail-proto-imap IMAP client (IDLE/watcher, CONDSTORE/QRESYNC, writes, OAuth2, COMPRESS; ADR 0018)
crates/nail-proto-smtp SMTP submission (lettre) → OutgoingTransport (ADR 0017)
crates/nail-proto-pop3 POP3 client (in-house) + leave-on-server policies (ADR 0017)
crates/nail-auth       OAuth2 (PKCE, loopback), token manager, keychain SecretStore (ADR 0017)
crates/nail-discovery  autoconfig / ISPDB / SRV / Autodiscover (ADR 0017)
crates/nail-search     Tantivy index + query language parser (ADR 0016)
crates/nail-testkit    mock IMAP server, .eml corpus, demo seed
crates/nail-testkit-net mock SMTP / POP3 / OAuth2 / HTTP / DNS servers
crates/nail-compose    composer: MIME (mail-builder), Markdown (comrak), CSS inlining, quoting (ADR 0023)
crates/nail-rules      rules engine: conditions, actions → planned queue ops with inverses, dry runs (ADR 0024)
crates/nail-mcp        MCP server (rmcp): Streamable HTTP on loopback, grants, audit; McpData trait +
                       CoreData adapter, admin requests, `nail mcp` stdio bridge (ADR 0025)
crates/nail-core       Service: routes requests; owns store, search index, accounts.kdl, per-account
                       engines (IMAP IDLE / POP3), op replay, outbox, OAuth2, virtual accounts (ADR 0020);
                       M1b: composer.rs (NewDraft…, server Drafts copy), rules.rs + rule_mail.rs
                       (rules.kdl, arrival / on-send / schedule / manual runs, undo), mcp.rs
                       (start_mcp_server, McpRunning), customize.rs (write-back, merge) (ADR 0027)
crates/nail-server     axum: UI bundle, WebSocket API, blobs, test controls (demo-multi only)
crates/nail-cli        `nail` binary
app/src-tauri          Tauri 2 desktop shell (starts the MCP server; main window created in code)
ui/                    React renderer; M1b feature slots in ui/src/features/<name>/ (README there)
e2e/                   Playwright tests (support/desktop.ts: WebView2 CDP smoke test; support/mcp.ts)
defaults/              shipped theme.kdl + layout.kdl
locales/               Fluent catalogs (en, de)
```

## Commands

Full details in `docs/dev-setup.md`; keep both in sync.

```
npm ci                                     # npm workspaces: ui, e2e
npm run build                              # ui/dist — required before any `cargo … --workspace` (Tauri embeds it)
cargo test                                 # default members = all crates except app/src-tauri
cargo test --workspace                     # incl. the Tauri shell (after npm run build)
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
npm test                                   # Vitest (ui), incl. the lint-rule self-test
npm run lint                               # ESLint + tsc + Prettier (ui), tsc + Prettier (e2e)
npm run e2e                                # builds ui + nail-cli + desktop app, then Playwright (docs/dev-setup.md)
npm run e2e:only                           # Playwright against what is already built
npm run build:desktop                      # nail-app (custom-protocol) for the desktop smoke test, own target dir
npm run gen:api                            # regenerate ui/src/api/generated from nail-api (a test fails when stale)
NAIL_UPDATE_SNAPSHOTS=1 cargo test -p nail-config --test schema   # regenerate config JSON Schemas (theme, layout, accounts)
NAIL_UPDATE_SNAPSHOTS=1 cargo test -p nail-config --test rules_schema   # rules.kdl JSON Schema
NAIL_UPDATE_SNAPSHOTS=1 cargo test -p nail-config --test defaults  # nail-api fixtures after editing defaults/*.kdl (then prettier)
cargo run -p nail-cli -- serve --seed demo --open                 # web mode with demo data (127.0.0.1:4780)
cargo run -p nail-cli -- serve --seed demo-imap                   # demo corpus synced from an in-process mock IMAP server
cargo run -p nail-cli -- serve --seed demo-multi                  # IMAP + POP3 + SMTP mock accounts (test controls at /api/test/…)
cargo run -p nail-app --features tauri/custom-protocol -- --seed demo   # desktop app on the built ui/dist (temp dirs)
NAIL_MCP_TOKEN=<token> nail mcp --client <name> [--url URL] [--data-dir DIR]   # stdio bridge to the running app's MCP server
npm run tauri dev                          # desktop app with Vite hot reload
```

- The contract lives in `crates/nail-api`; its fixtures `fixtures/default-{theme,layout}.json` describe the default UI. See ADRs 0002 (protocol) and 0003 (config models, part/DOM conventions).
- `nail-core::Service` is the single entry point for every transport (ADR 0009): add a `Request` variant → handle it in `Service::dispatch` → `npm run gen:api`.
- `nail serve` prints only its URL on stdout; logs go to stderr (`NAIL_LOG`). Data dir: `nail.sqlite3` + `blobs/`; config dir: `theme.kdl`, `layout.kdl`, `accounts.kdl`. With `--seed` and no `--data-dir` / `--config-dir` it uses a temporary directory, never the real profile.
- M1a contract: ADR 0012 (requests, events, engine traits in `nail_api::engine`, expected crate shapes); `accounts.kdl`: ADR 0013 (secrets only as `keychain:` references); libraries: ADR 0014 (TLS = rustls + `ring` only, no aws-lc).
- Every M1a request is implemented by the core (ADR 0020); `unsupported` remains for client certificates. The UI `MockTransport` implements the same contract (ADR 0019).
- Tests and demos never touch the OS keychain: pass `ServiceOptions::secrets` (a `MemorySecretStore`); `Service::from_parts` does so itself. Engine timing for tests: `EngineTiming::fast()`.
- M1a decisions: ADRs 0015 (store v2), 0016 (search), 0017 (SMTP/POP3/auth/discovery), 0018 (IMAP), 0019 (UI), 0020 (core integration), 0021 (review fixes: UIDVALIDITY-safe replay, Sent copies via the queue, discovery warnings, query limits, `ChooseIdentity`).
- M1b contract: ADR 0022 (requests, store v4, crate stubs, feature slots: `nail-core/src/<pkg>.rs`,
  `nail-config/src/widgets/<pkg>.rs`, `ui/src/features/<pkg>/`, `locales/{en,de}/<pkg>.ftl`). Packages:
  ADRs 0023 (composer), 0024 (rules), 0025 (MCP), 0026 (customization); integration ADR 0027. Every M1b
  request is implemented by the core and the UI mock.
- `nail serve` and the desktop app start the MCP server (`--mcp-port`, `--no-mcp` for serve); the
  endpoint is in `<data-dir>/mcp-endpoint.json`. Seeded runs never open an editor (`open_editor`).
- Python on Windows writes CRLF in text mode: when scripting file edits, write with `newline=''`
  (the repo is LF, `.gitattributes`); the `nail-compose` golden `.eml` files are CRLF on purpose.
- `tauri-build` static VC runtime is limited to release builds (ADR 0004), otherwise workspace doctests fail to link.

## Git

- Branch `main`. Commit only when asked, or as part of an agreed workflow stage.
- End commit messages with the Co-Authored-By / Claude-Session lines required by the session.

## Memory

Project memory lives in the repo at `.claude/memory/` (one fact per file, index in `.claude/memory/MEMORY.md`). Don't store project memory anywhere else. Update or delete entries that turn out to be wrong.
