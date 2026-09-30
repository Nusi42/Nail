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
                       M1b: rich drafts, rules, mcp, context (UI context), customize (ADR 0022);
                       M2a: security (message security, keys, recipient keys, SendCrypto,
                       auth results, warnings, vault, settings; ADR 0029)
                       M2b: remote (remote-UI settings / status, users, passkeys, devices,
                       sessions, pairing codes, RemoteGrant, audit, the POST /api/auth login
                       protocol), remote_content (allow-lists, nail-proxy: URLs) (ADR 0036);
                       launch (LaunchRequest / MailtoLink: mailto: and opened files; ADR 0041)
                       M2c: localized (LocalizedMessage = Fluent id + args; every English
                       diagnostic has an optional `localized` sibling), sieve (ManageSieve
                       settings, rule placement, vacation), triage (junk, snooze,
                       notifications), sync (folder sync policy, body state), menus
                       (context menus resolved from the layout) (ADR 0043)
crates/nail-config     theme.kdl / layout.kdl / accounts.kdl parsing, defaults, schema, watcher,
                       format-preserving accounts.kdl writer (AccountsDocument); widget kinds per
                       feature in src/widgets/; rules.kdl in src/rules.rs (WP22);
                       M2c: `sieve` node of an IMAP account, reset-toolbar / reset-key-binding
                       write-back, src/l10n.rs (fluent-bundle `Catalog` over every catalog, English
                       fallback; every ConfigError / rule issue carries a LocalizedMessage) (ADRs 0043, 0046)
crates/nail-store      SQLite metadata (v6), blob store, per-account operation queue with sync-side
                       merge, threading (JWZ), drafts/outbox + attachments, search filters (ADR 0015);
                       v4: collected addresses, rule runs (undo), MCP clients/grants/audit, saved searches;
                       v5: keyring, trust decisions, Autocrypt peers, vault state, message security,
                       app settings (src/crypto.rs, ADR 0029); seal.rs: DataCipher, nail_seal /
                       nail_open SQL functions, SEALED_COLUMNS, convert_fields, compact; BlobStore
                       set_cipher / convert / hashes / remove_temp_files (ADR 0032);
                       v6: remote.rs — remote users (Argon2id hash / keychain TOTP ref only),
                       passkeys, devices, sessions (token hashes, rotation), pairing codes,
                       throttle, remote audit, remote-content allow-lists (ADR 0036);
                       v7: triage.rs — messages.body_state, snoozes, junk classifier (hashed
                       tokens; untrain_junk), notification overrides, Sieve state + vacation
                       (clear_sieve_upload) (ADR 0043); bodies.rs (headers-only merge, attach /
                       detach bodies, fetch targets), lock.rs DataDirLock (ADR 0046)
crates/nail-mime       MIME parsing, HTML sanitizing, top-level header list (rules); domain.rs,
                       auth_results.rs (RFC 8601, trusted authserv-id), links.rs (link / attachment
                       warnings) (ADR 0032); remote_content.rs: ProxyRewriter, parse_message_with
                       (remote references counted, rewritten to nail-proxy: URLs; ADR 0041);
                       markdown.rs: text/markdown alternatives as bodies (comrak; ADR 0046)
crates/nail-proto-imap IMAP client (IDLE/watcher, CONDSTORE/QRESYNC, writes, OAuth2, COMPRESS; ADR 0018);
                       bodies.rs (BodyFetch, fetch_batch_with, fetch_body), search.rs (SearchKey,
                       search_messages) (ADR 0046)
crates/nail-proto-smtp SMTP submission (lettre) → OutgoingTransport (ADR 0017)
crates/nail-proto-pop3 POP3 client (in-house) + leave-on-server policies (ADR 0017)
crates/nail-auth       OAuth2 (PKCE, loopback), token manager, keychain SecretStore (ADR 0017)
crates/nail-discovery  autoconfig / ISPDB / SRV / Autodiscover (ADR 0017)
crates/nail-search     Tantivy index + query language parser (ADR 0016); open_with(DirectoryWrapper)
                       for the sealed index, IndexPolicy (decrypted indexing opt-in) (ADR 0032);
                       imap.rs: query → IMAP SEARCH key (needs_server, server_search) (ADR 0046)
crates/nail-testkit    mock IMAP server (SEARCH incl. TEXT / BODY / OR / NOT), .eml corpus, demo seed;
                       corpus/crypto/{pgp,smime}: end-to-end
                       test vectors (not demo mail; `crypto_fixture`; each has generate.sh + README;
                       smime/ada-{work,home}.p12 are the demo-multi certificates)
crates/nail-testkit-net mock SMTP / POP3 / OAuth2 / HTTP / DNS servers; managesieve.rs:
                       MockSieveServer + validate_script (ADR 0044)
crates/nail-sieve      compiler (rules + vacation → one Sieve script, placement + localized reasons),
                       parse / decompile round-trip harness, ManageSieve client (SieveSession,
                       SieveAuth), upload / remove of the managed script (ADR 0044)
crates/nail-triage     pure logic: tokenizer, junk classifier, snooze scheduler (timer wheel),
                       notification policy + batcher, local times; writes as TriageOp (ADR 0045)
crates/nail-compose    composer: MIME (mail-builder), Markdown (comrak), CSS inlining, quoting (ADR 0023)
crates/nail-rules      rules engine: conditions, actions → planned queue ops with inverses, dry runs (ADR 0024)
crates/nail-mcp        MCP server (rmcp): Streamable HTTP on loopback, grants, audit; McpData trait +
                       CoreData adapter, admin requests, `nail mcp` stdio bridge (ADR 0025);
                       messages.rs (localized texts), configured_port (ADR 0046)
crates/nail-pgp        OpenPGP (rPGP): keyring (S2K secrets: passphrase or keychain:nail/openpgp/<FPR>),
                       PGP/MIME sign/encrypt/decrypt/verify, inline read, header protection, Autocrypt
                       L1, WKD + keys.openpgp.org via injected HttpClient (ADR 0030)
crates/nail-smime      S/MIME (RustCrypto cms/x509-cert): sign/encrypt/verify/decrypt, chain validation
                       (TrustAnchors), PKCS#12, keyring, Windows cert store + CNG keys (ADR 0031)
crates/nail-vault      encryption at rest: sealed format (XChaCha20-Poly1305), data key in the keychain,
                       SealedDirectory (tantivy), Vault migration plain <-> encrypted (ADR 0032)
crates/nail-remote-auth remote-UI auth: passkeys (webauthn_rp), Argon2id + TOTP, pairing codes,
                       device-bound sessions, rate limits, exhaustive required_access, axum login
                       route / middleware (ADR 0037)
crates/nail-proxy      SSRF-safe remote-content proxy: signed nail-proxy: tokens, private-range
                       blocking, check-then-pin fetcher, size / type limits; `test-support` feature
                       (enabled by nail-core's `demo`) for one loopback port (ADR 0038)
crates/nail-core       Service: routes requests; owns store, search index, accounts.kdl, per-account
                       engines (IMAP IDLE / POP3), op replay, outbox, OAuth2, virtual accounts (ADR 0020);
                       M1b: composer.rs (NewDraft…, server Drafts copy), rules.rs + rule_mail.rs
                       (rules.kdl, arrival / on-send / schedule / manual runs, undo), mcp.rs
                       (start_mcp_server, McpRunning), customize.rs (write-back, merge) (ADR 0027);
                       M2a: security.rs (every security request), crypto.rs (keyrings, decrypt/verify
                       on read, sign/encrypt + Autocrypt on send, arrivals, configured authserv-ids +
                       suggestion, message_is_encrypted = the MCP `encrypted` gate), vault.rs
                       (VaultCtl, migration with the swappable SearchSlot) (ADR 0034, fixes ADR 0035);
                       M2b: remote.rs (requests, RemoteState: RemoteAuth, lazy proxy Signer / Proxy,
                       RemoteUiCommand host link), remote_access.rs (handle_remote: authorize, grant
                       narrowing, encrypted gate, render_url), launch.rs (launch requests,
                       ImportMessageFile) (ADRs 0036, 0041);
                       M2c (ADR 0048): sieve.rs (upload task, placements, vacation), junk.rs
                       (classifier on the store, MarkJunk, arrivals, TriageOp → queue), snooze.rs
                       (scheduler + wake task, hidden Snoozed folder), notify.rs (policy +
                       batching → NotificationRequested), bodies.rs (on-demand fetch), headers-only
                       sync in sync.rs / ingest.rs, server search in search.rs, replay connection
                       in imap_engine.rs, clock.rs AdjustableClock (ServiceOptions::clock),
                       paths::lock_data_dir, strings.rs over nail_config::l10n
crates/nail-server     axum: UI bundle, WebSocket API, blobs, test controls (demo-multi only);
                       M2b: bind_with (TLS rustls + ring, rcgen certs, instant-acme, headers,
                       sandbox origin, proxy route, `auth`), remote_ui.rs RemoteUiHost (ADRs 0038, 0041);
                       M2c: render tokens bound to the session (sandbox.rs), proxy_limit.rs (ADR 0048)
crates/nail-cli        `nail` binary; messages.rs (localized stderr), headless `nail mcp`
                       (`--config-dir`, `--no-start`, `--mcp-port`), serve holds the data-dir lock
app/src-tauri          Tauri 2 desktop shell (starts the MCP server on the settings' port and
                       restarts it on change, and the remote-UI host; data-dir lock; OS notifications
                       for NotificationRequested; main window created in code; nail-proxy protocol;
                       single instance); launch.rs,
                       portable.rs, setup_helper.rs; installer/ (NSIS template copy + hooks; ADR 0040)
ui/                    React renderer; feature slots in ui/src/features/<name>/ (README there;
                       M2a: security/; M2b: remote/, launch/; M2c: sieve/ (status, vacation),
                       triage/ (junk, snooze, notifications), sync/ (policies, bodies), menus/
                       (context menus, resolve.ts), customization/Editors.tsx); RemoteGate.tsx
                       (login gate), api/reauth.ts, theme/trustedHtml.ts (Trusted Types for srcdoc),
                       theme/BootTheme.tsx (themed boot / login pages), src/a11y.test.tsx (axe in
                       Vitest); vite.config.ts splits vendor / catalogs chunks, TipTap / CodeMirror
                       load lazily
e2e/                   Playwright tests (support/desktop.ts: WebView2 CDP smoke test; support/mcp.ts)
defaults/              shipped theme.kdl + layout.kdl; themes/ (example themes: high-contrast.kdl)
locales/               Fluent catalogs (en, de); core*.ftl = texts built in Rust (M2c)
scripts/               build-installer.mjs, sbom.mjs, installer-smoke.ps1, test/ (node:test);
                       interop/ = separate Cargo workspace (copy the root Cargo.lock in first) (ADR 0040)
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
node --test "scripts/test/*.test.mjs"      # installer / release script tests
cargo deny check                           # licences, bans (openssl, aws-lc), sources, advisories (deny.toml)
node scripts/build-installer.mjs [--skip-ui]   # NSIS installer, portable zip, SBOMs → build dir (never the repo)
powershell -File scripts/installer-smoke.ps1   # install to a temp dir, start, uninstall
node scripts/sbom.mjs                      # CycloneDX SBOMs
NAIL_UPDATE_SNAPSHOTS=1 cargo test -p nail-config --test schema   # regenerate config JSON Schemas (theme, layout, accounts)
NAIL_UPDATE_SNAPSHOTS=1 cargo test -p nail-config --test rules_schema   # rules.kdl JSON Schema
NAIL_UPDATE_SNAPSHOTS=1 cargo test -p nail-config --test defaults  # nail-api fixtures after editing defaults/*.kdl (then prettier)
bash crates/nail-testkit/corpus/crypto/pgp/generate.sh     # OpenPGP interop vectors (GnuPG 2.4 = Git for Windows /usr/bin/gpg)
bash crates/nail-testkit/corpus/crypto/smime/generate.sh   # S/MIME test CA + vectors (OpenSSL 3.4+ = /mingw64/bin/openssl)
cargo run -p nail-cli -- serve --seed demo --open                 # web mode with demo data (127.0.0.1:4780)
cargo run -p nail-cli -- serve --seed demo-imap                   # demo corpus synced from an in-process mock IMAP server
cargo run -p nail-cli -- serve --seed demo-multi                  # IMAP + POP3 + SMTP mock accounts (test controls at /api/test/…)
cargo run -p nail-app --features tauri/custom-protocol -- --seed demo   # desktop app on the built ui/dist (temp dirs)
NAIL_MCP_TOKEN=<token> nail mcp --client <name> [--url URL] [--data-dir DIR]   # stdio bridge to the running app's MCP server
nail mcp --client <name> [--config-dir DIR] [--mcp-port N] [--no-start]   # starts a headless Nail when none runs
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
  ADRs 0023 (composer), 0024 (rules), 0025 (MCP), 0026 (customization); integration ADR 0027; review
  fixes ADR 0028 (rule mail loop marks, redirects via the outbox, autosave races, MCP grant leaks, undo).
  Every M1b request is implemented by the core and the UI mock.
- M2a contract: ADR 0029 (security protocol, identity `crypto` in `accounts.kdl`, store v5, crate
  stubs `nail-pgp` / `nail-smime` / `nail-vault`, slots). Licences: no GPL/LGPL/AGPL; no OpenSSL
  (`cargo tree -i openssl-sys` must stay empty). The crypto crates share rPGP's RustCrypto
  generation (digest 0.10; workspace `sha2-legacy` = sha2 0.10, `sha2` = 0.11 for ids).
  Packages: ADRs 0030 (OpenPGP), 0031 (S/MIME), 0032 (data protection), 0033 (security UI);
  integration ADR 0034. Sending never falls back to plain text (`no_key` / `locked`); decrypted
  content is never persisted (only `message_security` status; search index only with the
  per-account opt-in); MCP needs the `encrypted` scope for it. Seeded runs use
  `nail_core::demo::demo_options` (memory secrets, test CA anchor, offline key lookup);
  demo-multi accounts get keys via `DemoMulti::seed_security`. `Service::search_index()`
  returns an `Arc` (the vault migration swaps the index; never hold it across long work).
- M2b contract: ADR 0036 (`nail_api::remote` / `remote_content`, store v6 with hashed or
  `keychain:`-referenced secrets only, crate stubs `nail-remote-auth` / `nail-proxy`, TLS libs in
  `nail-server`, slots incl. WP44's `tauri.conf.json` bundle section, `app/src-tauri/installer/`,
  `.github/workflows/`, `deny.toml`, `scripts/`). Passkeys use `webauthn_rp`, not `webauthn-rs`
  (OpenSSL). `cargo tree -i aws-lc-rs` must stay empty too (TLS = ring). Internet mode is locked
  (`nail_api::remote::internet_mode_lock()`); the core refuses it with `forbidden`. Remote
  sessions carry a `RemoteGrant` (MCP scopes + `admin`), always within device and user.
- M2b integration (ADR 0041): packages ADRs 0037 (auth), 0038 (transport, proxy), 0039 (UI),
  0040 (installer / CI). `nail_server::RemoteUiHost` runs the remote UI from the core's settings
  (off by default; TLS + login); the desktop's own UI (loopback `nail serve`, Tauri) has no login.
  Remote sessions go through `Service::handle_remote` (authorize, grant narrowing, `encrypted`
  gate). The proxy key (`keychain:nail/proxy/signing-key`) is created lazily; seeded runs'
  proxy resolves no names (demo-multi: `images.nail.example` → its mock HTTP server,
  `private.nail.example` → refused). `srcdoc` goes through the `nail-reader` Trusted Types
  policy (`theme/trustedHtml.ts`); the remote UI frames mail from the sandbox origin
  (`MessageBody.render_url`). Launch requests (`mailto:`, `.eml`) go through the core
  (`TakeLaunchRequests`, `ImportMessageFile`; desktop only). E2E remote tests trust the
  self-signed certificate by SPKI (`trustingBrowser`) where WebAuthn needs it.
- M2b review fixes (ADR 0042): `Service::check_remote_request` is an exhaustive match (a new
  `Request` must say what it names); drafts need their identity's account and a readable original
  (`encrypted` gate); remote search filters before paging; remote sockets get
  `Service::remote_event`; outside internet mode only local peers connect
  (`TransportOptions::public_peers`, `nail_api::remote::is_local_address`); the portable zip
  carries `Nail/ui/`.
- M2c contract: ADR 0043 (`LocalizedMessage` + `localized` siblings on every diagnostic; Sieve /
  vacation, junk / snooze / notifications, folder sync policies + `headers_only`, context menus,
  `reset-toolbar` / `reset-key-binding`, MCP settings, `RuleFromMessage`; store v7; stub crates
  `nail-sieve` / `nail-triage`; slots incl. `nail-config/src/l10n.rs`,
  `nail-testkit-net/src/managesieve.rs`, `locales/*/core{,-sieve,-triage}.ftl`). The core serves
  the store-backed requests; `SyncSieve`, `PreviewSieveScript`, `MarkJunk`, `(Un)SnoozeMessages`
  answer `unsupported` until WP55. `nail-api/tests/catalog.rs` checks contract Fluent ids.
- `nail serve` and the desktop app start the MCP server (the settings' fixed port; `--mcp-port`,
  `--no-mcp` for serve); the endpoint is in `<data-dir>/mcp-endpoint.json`. `nail serve` and seeded
  runs never open an editor (`open_editor`). One process per data directory (`DataDirLock`: serve,
  desktop, headless `nail mcp`).
- M2c integration (ADR 0048; packages ADRs 0044 Sieve, 0045 triage, 0046 engine polish, 0047 UI):
  Sieve uploads run from a core task (digest of the inputs; server rules are skipped on arrival);
  arrivals go security → junk → rules → notifications; snooze = `$Snoozed` + move to the hidden
  `Snoozed` folder (created by the first move) + local wake timers; triage time comes from
  `ServiceOptions::clock`. `--seed demo-multi` adds a mock ManageSieve server (the `sieve` node
  starts switched off: the mock stores but does not run scripts), a Junk folder, a headers-only
  folder `Newsletters`, and test controls `sieve-scripts`, `clock`, `clock-set {at}`,
  `clock-advance {seconds}`.
- E2E: the harness passes `--ui-dir <repo>/ui/dist` to `nail serve`, because building `nail-app`
  copies Tauri's resources into `target/debug/ui`, which `nail serve` would prefer (stale bundle).
- Python on Windows writes CRLF in text mode: when scripting file edits, write with `newline=''`
  (the repo is LF, `.gitattributes`); the `nail-compose` golden `.eml` files are CRLF on purpose.
- `tauri-build` static VC runtime is limited to release builds (ADR 0004), otherwise workspace doctests fail to link.

## Git

- Branch `main`. Commit only when asked, or as part of an agreed workflow stage.
- End commit messages with the Co-Authored-By / Claude-Session lines required by the session.

## Memory

Project memory lives in the repo at `.claude/memory/` (one fact per file, index in `.claude/memory/MEMORY.md`). Don't store project memory anywhere else. Update or delete entries that turn out to be wrong.
