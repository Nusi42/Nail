# ADR 0027: M1b integration — core wiring, defaults, e2e and the desktop smoke test

Status: accepted · 2026-09-29 · M1b WP25

## Context

WP21–WP24 (ADRs 0023–0026) built the composer engine and UI, the rules
engine, the MCP server and the customization tooling in their slots
(ADR 0022). Their core requests still answered `unsupported`, the default
layout and theme did not place or style anything new, and several shared
changes were left to the integrator as "API requests". WP25 wires it all,
adds the Playwright scenarios of plan WP25 (including the carried-over
desktop smoke test) and closes the M1a follow-ups.

## Decisions

### Composer in the core (`composer.rs`, `outbox.rs`)

- `nail-compose` builds every outgoing message (`build_draft_message`):
  threading headers from the replied message, the identity's style,
  attachment bytes read from the blob store before building.
  `nail-core/src/mime.rs` is gone. Plain text now goes out as
  `format=flowed` and display names are quoted (`"Ada Lovelace" <…>`).
- `SaveDraft` accepts rich drafts: HTML is sanitized on save, attachments
  must be uploaded blobs (`not_found` otherwise). `UploadAttachment`
  limits files to 25 MB (`too_large`). `NewDraft` chooses the identity like
  `ChooseIdentity`, builds the original from the stored body (forwards copy
  the attachments into the blob store) and localizes the quote header and
  forward texts from Fluent (`strings.rs` now reads `app.ftl`,
  `compose.ftl` and `composer.ftl` and fills `{ $var }` placeables).
- **Contract:** `Identity` gains `posting: top | bottom` and
  `mail_style {css, code_theme}`; `accounts.kdl` writes them as
  `posting="top"` and a `mail-style css=… code-theme=…` child (reader,
  writer, JSON Schema, reference docs).
- **Server Drafts copy (autosave):** a save schedules an upload after
  `EngineTiming::draft_upload` (5 s; 300 ms in tests), replacing a pending
  one. The upload stores the built message as a local message in the
  account's Drafts folder (id derived from `draft-copy:<Message-ID>`), files
  it on the server with `AppendMessage` and deletes the previous copy with a
  permanent `DeleteMessage` — both through the operation queue, so offline
  saves replay later. The copy is recorded in the v4 `server_*` columns;
  sending or deleting the draft removes it. IMAP accounts only.
- **Contract:** `ListDrafts` → `Drafts {drafts}` (saved drafts not in the
  outbox). The outbox window lists them under "Drafts" with "Continue
  writing" / "Discard", so a draft survives a restart.
- Addresses for autocomplete are recorded on send (recipients) and on
  ingest (senders of new mail; recipients of mail in Sent; nothing from
  Junk, Trash or Drafts).

### Rules in the core (`rules.rs`, `rule_mail.rs`, `watch.rs`)

- `RulesManager` owns `rules.kdl` like the account manager owns
  `accounts.kdl`: edits go through `RulesDocument` (format-preserving),
  hand edits made meanwhile are picked up first, a broken file refuses
  edits (`config`) and keeps the last valid rules. One watcher now handles
  `accounts.kdl` and `rules.kdl`; both report every error
  (`ConfigErrors`) and clear the list when a file becomes valid again.
- Messages reach `nail-rules` as `MessageData` (stored summary, the decoded
  top-level header list from the new `nail_mime::header_fields`, body text,
  attachments) and accounts / folders as a `StaticMailbox`.
- **Arrival:** the sync engines (IMAP and POP3) put new messages into the
  store's rule inbox right before they emit `NewMail`; the rules task wakes
  on events (a lagged receiver just processes the inbox) and on a tick
  (`EngineTiming::rule_tick`, 60 s; 1 s in tests), which also runs due
  scheduled rules. **On send:** the Sent copy after filing it.
  **Manual / scheduled:** the named messages, the folder, or the Inboxes
  (plus folders named by a folder scope), newest first, at most 2000.
- A run is recorded per rule and batch. Queue effects are enqueued and
  recorded with their inverse, so `UndoRuleRun` queues the inverses in
  reverse order. Copies (a local copy + `AppendMessage`), forwards and
  template replies (drafts sent through the outbox without delay) and
  redirects (the original bytes with `Resent-*` headers, submitted
  directly) are recorded without an operation id: undo leaves them alone
  and the rest of the run stays undoable.
- **Contract:** `RuleMatch.explanation: ConditionHit[]` ("why it matched").
- `notify` is planned but has no effect: the UI shows no new-mail
  notifications yet.

### MCP in the core (`mcp.rs`)

- `McpCore(Service)` implements `nail_mcp::CoreAccess`;
  `Service::start_mcp_server(port, endpoint_file)` returns `McpRunning`,
  which the transport keeps (`nail serve`, the desktop shell). The server
  is deliberately **not** stored in the service: it holds the service as its
  data provider, and the service owning it would be a reference cycle.
  The service only keeps its `McpServerInfo` for `GetMcpServerInfo`.
- A forwarding task feeds core events to the subscriptions and wakes
  `nail://ui/context` subscribers after each `ReportUiContext` (a
  `watch` channel bumped by the report).
- `nail serve --mcp-port <port>` / `--no-mcp`; the desktop app always uses a
  free port. Agents find it through `<data-dir>/mcp-endpoint.json`.
- MCP audit entries and finished rule runs older than 90 days are purged
  when the engines start (`HISTORY_RETENTION_DAYS`).

### Customization in the core (`customize.rs`, `service.rs`)

- The requests call `ConfigManager` and broadcast the returned updates;
  `OpenConfigFile` spawns the editor only when `ServiceOptions::open_editor`
  (off for seeded runs and tests, which must never open windows).
- `config_error_events()` (sent on connect and resync) adds one
  `ConfigErrors` per file — theme, layout, accounts, rules — with every
  error or an empty list, after the M1a `ConfigError`s.

### Defaults and UI wiring

- `layout.kdl`: `config-errors` (hidden while valid) above the panes,
  `smart-folders` under the folder tree, `mcp-context-indicator` under the
  status bar, a `rules` window, settings tabs "MCP clients" and "Customize"
  (a `customize` toolbar with merge, errors and inspector, plus the error
  list), actions for every feature, `Ctrl+Shift+I` (inspector) and
  `Ctrl+Shift+A` (attach), "Filter rules" on the main toolbar and "Create
  rule from message" on the reader toolbar and the row menu.
- `theme.kdl`: parts for all four features; `list-style` and
  `border-collapse` (outside the supported property subset) live in the
  structural base CSS of `ui/src/theme/compile.ts` like the existing
  lists. The split divider becomes a 5 px grab area with a 1 px line: at
  1 px it could hardly be grabbed (found by the divider e2e scenario).
- The built-in `mail.*` / `composer.*` / `outbox.undo` actions run the rich
  flows. When the core cannot choose an identity (a demo account without
  one), `NewDraft` fails with `invalid_request` and the plain composer of
  M1a opens instead, as for `unsupported`.
- The status bar has an accessible name ("Status"), since the agent
  indicator is a second status region; failed writes get "Retry"
  (`RetryOp`, M1a follow-up).
- The UI layout normalizer kept theme rule positions but dropped widget
  positions; the inspector now gets them.
- `nail serve --seed demo` and `nail-app --seed demo` rebuild the search
  index after seeding (`Service::rebuild_search_index`): the seed goes
  straight into the store, and search found nothing before.

### Desktop app and the smoke test

- `nail-app --seed demo` without directories runs in a temporary directory
  (M1a follow-up), and the main window is now created in code
  (`"create": false` in `tauri.conf.json`) so a seeded run also keeps its
  WebView2 profile there. That keeps the smoke test independent of a real
  instance of the app.
- `npm run build:desktop` builds `nail-app` with `tauri/custom-protocol`
  into its own target directory (`nail-build/desktop-target`) so the
  feature does not trigger rebuilds in the shared one; `npm run e2e` runs it.
- `e2e/tests/desktop.spec.ts` starts the app with
  `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=<port>` in
  the child's environment only and attaches with
  `chromium.connectOverCDP`. It runs locally on Windows and is skipped in
  CI unless `NAIL_E2E_DESKTOP=1`.

## Consequences

- Every M1b request is answered by the core; the UI mock implements the
  same contract (including `ListDrafts` and rich drafts in the shared
  outbox mock).
- Follow-ups: the UI has no right-click menus yet (the layout defines
  them); inline images of reopened HTML drafts show their alt text (needs a
  blob URL or request); "create rule from message" is built in the UI from
  the summary (the core's `rule_from_message`, which knows `List-Id`, is
  not exposed); new-mail notifications (and with them the `notify` rule
  action); editing UI for toolbars, keys and columns (write-back exists);
  lazy-loading the TipTap / CodeMirror editors (main chunk ~1.4 MB);
  `nail-mime` lists a received `text/markdown` alternative as an
  attachment; a fixed MCP port setting for the desktop app; CI does not run
  the desktop smoke test yet.
