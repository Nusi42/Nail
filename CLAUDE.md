# Nail — project guide for Claude

Nail (working name) is an email + PIM client: Rust core, TypeScript/React UI, Tauri 2 desktop shell, a web-server mode that serves the same UI, and MCP access to all data.

- What to build: `docs/design.md` (exported from the living doc linked in its header).
- How and in which order: `docs/plan.md` (milestones M0–M5, work packages, gates).
- Decisions and deviations: `docs/adr/NNNN-title.md`.
- Project memory: `.claude/memory/` (see "Memory" below).
- Where to work: the work folders, never the master (see "Work folders" below).

## Non-negotiable rules

- **Work folders (ADR 0132).** `<master>` is the master repository: it only receives
  pushes and `git merge --ff-only work-1`. Never edit, build, test, run, install or commit anything there (the npm entry
  points, `prepare`, the pre-commit hook and its Cargo rustc wrapper refuse). Work in `<work root>\nail-work-1`
  (branch `work-1`, the lead) or, for a parallel package, `nail-work-<N>` (branch `work-<N>`, reset to `origin/main`, push the
  branch when done; never push to `main`). New folder: `node scripts/work-folders.mjs create <n>`.
  Details: `docs/dev-setup.md` "Work folders". Hooks: pre-commit (per-file checks), pre-push (`npm run check`), and
  the Claude Code post-edit hook (per-file checks on each edited file; fix what it reports).
- **Tests from day 1.** Every change ships with unit tests; every user-visible behaviour with a Playwright scenario. Never report work as done while any suite is red or tests were skipped.
- **Docs split.** Internal docs → `docs/`. User-facing manuals and references → `userDocs/<lang>/` (`en` written, `de` empty for now).
- **Languages.** No literal user-visible strings in React or Rust. Strings live in Fluent catalogs `locales/en/*.ftl` (complete) and `locales/de/*.ftl` (same files, empty). Missing keys fall back to English per key.
- **Everything themeable.** React components never use inline styles or literal class names. They emit `data-part` attributes; styling comes only from the compiled theme (`theme.kdl`). Structure comes only from `layout.kdl`.
- **The core owns data.** The UI talks only to `NailClient`; business logic lives in Rust. Every write goes through the operation queue.
- **Windows first.** macOS, Linux and mobile come later.
- **No fixed development ports.** Take a free port or fall back to one, so Nail runs next to other projects (`scripts/dev.mjs`: Vite on `NAIL_DEV_PORT` / 5791 / a free port + Tauri `devUrl`; `nail serve` falls back from 4780; tests bind port 0).
- **No non-loopback sockets.** Never open sockets on non-loopback/unspecified addresses (TCP or UDP), except the user-enabled remote UI listener; use the OS DNS API; tests bind 127.0.0.1 (ADR 0057; `scripts/check-socket-binds.mjs` runs in `npm test`).
- **Zero warnings (Q1, ADR 0123).** 0 warnings, 0 errors; never relax lint levels or `-D warnings` (no new `allow`
  in `[workspace.lints]`, no ESLint rule switched off, no `--no-verify`). An unavoidable exception needs
  `#[expect(lint, reason = "…")]` (Rust; `#[allow]` is denied) / `// eslint-disable-next-line rule -- reason` at the
  narrowest scope. `npm run check` runs every formatter and linter; the pre-commit hook checks staged files.
  Conventions (ADRs 0124–0129): every integration-test crate root (`tests/*.rs`, `tests/*/main.rs`) starts with
  `#![cfg(test)]` (so `clippy.toml`'s unwrap / expect exemption covers its helpers; no exception attribute); a test
  crate that uses part of `tests/common` declares `#[expect(dead_code, reason = "…")] mod common;`; no `#[allow`
  anywhere under `crates/` / `app/`; `let _ = x;` → `drop(x)` or a logged error; casts via `try_from`; German or
  deliberately misspelled text sits behind `spellchecker:disable-line` / `spellchecker:off` … `on` markers. UI:
  `must()` in tests instead of `!`, `charCount` / `nonEmpty` / `hasText` (`ui/src/text`); scripts: `readJson` /
  `parseJson` (`scripts/lib/json.mjs`), `defined()` in e2e. Feature barrels (`features/*/index.ts`) never re-export
  mock modules: the mock core must stay out of the app bundle (`npm run build` fails on any Vite / Rolldown
  warning, `ui/build/strictBuild.ts`). Non-Windows `cfg` branches are linted by CI's `quality-linux` job (ADR 0130);
  clap help text has no backticks.
- **Record decisions.** New library choices or deviations from the design get an ADR.
- **Keep this file current.** When commands, layout or conventions change, update this file in the same commit.

## Tooling facts

- Rust stable (1.99; `rust-version` stays 1.97), MSVC, Node 26 + npm 11. **npm workspaces, not pnpm** (pnpm is not installed). No docker locally: integration tests use in-process mock servers from `crates/nail-testkit`.
- Work folders live outside OneDrive (ADR 0132): Cargo builds into the folder's `target/`, test and build output goes to its
  gitignored `.nail-build/` (`NAIL_BUILD_DIR` overrides; the development update key stays in `%LOCALAPPDATA%\nail-dev\keys`).
  When running several cargo processes in one folder, give each its own `CARGO_TARGET_DIR` under `.nail-build/`.
- Config files are KDL v2.
- SQLite is SQLite3 Multiple Ciphers (M4c, ADR 0095) through the root `[patch.crates-io]` stand-in
  `crates/nail-store/libsqlite3-sys-sqlite3mc`; the first build compiles its amalgamation (about 80 s in debug).

## Layout

```text
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
                       M2d: plugins (manifest + validation, permissions / grants, the
                       declarative UI component set `PluginUiNode` + validate_tree, dialogs,
                       notifications, command contexts, the plug-in mail view, admin types);
                       export_plugin_typescript → packages/plugin-api/src/generated (ADR 0050)
                       M3a: pim (PimTime, calendars / events as series + overrides + EditScope,
                       recurrence, attendees, alarms, contacts vCard 3/4, groups, merge links,
                       tasks, reminders, sources = PimSourceConfig in AccountsConfig.pim_sources,
                       import, read-only MCP payloads; issues() + pim-issue-* ids) (ADR 0058);
                       WP75: EventOverride.this_and_future, plugins::PluginCalendarEvent (ADR 0063)
                       M3b: scheduling (ItipMethod, Invitation card on MessageBody.invitation, targets,
                       answers / counters / organizer requests, SchedulingSettings, free/busy +
                       BusyPeriod::merge), gmail (IncomingServer::Gmail + GmailOptions, GmailFolder /
                       LabelChange mapping, categories, labels, MessageSummary.gmail); engine.rs:
                       MailApiEngine, AdaptivePoll, FreeBusyProvider, EngineError::retry_after_secs (ADR 0065);
                       WP85: AutomaticMail::Itip (prepared iMIP mail), DiscoverySource::Provider (ADR 0070)
                       M3c: exchange (IncomingServer::Exchange + ExchangeOptions engine graph / ews,
                       AuthConfig::Windows + SspiPackage, MailProtocol graph / ews, shared / delegate
                       mailboxes, categories + CategoryColor, Focused Inbox on MessageSummary.exchange,
                       rules.rs inbox rules, oof.rs automatic replies, rooms, Calendar / AddressBook
                       `.exchange`, PimSourceKind::Exchange `exchange:<account>`, status, ExchangeChanged);
                       exchange/engine.rs ExchangeEngine (mail per folder + settings + directory + PIM +
                       free/busy + sending) and NegotiateAuth; exchange/pim.rs provider-neutral Exchange
                       item shapes (ADR 0072)
                       M4a: agents (ActionKind, ActionPolicy ask / allow — destructive kinds always ask,
                       Confirmation + NewConfirmation, ConfirmationSource nail / elicitation / system),
                       mcp/tools.rs (MCP_TOOLS table, write-tool argument structs, McpWriteResult;
                       feature `json-schema` = schemars JsonSchema, enabled by nail-mcp), mcp/prompts.rs
                       (McpPrompt), McpRateLimits on McpClient; plugins: write / hook permissions,
                       hooks.rs (hook defs, results, vetoes, banners / badges, stats), extensions.rs
                       (composer actions, Markdown blocks, previews, attachment / link actions, MCP tools,
                       ComposerEdit), jobs.rs (CronSchedule, events, job status, PluginActivity) (ADR 0079)
                       M4b: desktop (InstallationInfo + registry facts, UpdateSettings / UpdateState /
                       UpdateStatus::compose (policy wins), DesktopSettings, DefaultAppsStatus, FirstRunState,
                       UiEndpointFile + dev-endpoint allow-list, DesktopHost trait + DesktopNotifier), mapi
                       (MapiMessage / MapiResult codes, pipe name + frames, registry, DLL paths);
                       LaunchRequest::ComposeDraft (ADR 0087)
                       M4c (ADR 0094): labels.rs (`labeled_enum!`: wire name + Fluent id + ALL),
                       portability (import sources / export targets, jobs, JobProgress / JobReport, ItemIssue,
                       Thunderbird AccountProposal / RuleProposal, LOCAL_FOLDERS_ACCOUNT_ID), backup (items,
                       `.nailbackup` format constants, manifest, RestorePlan / RestoreReport), recovery (recovery-key
                       text form), smartcard (PKCS#11 settings, tokens, PinPrompt, PinSource), palette (recent
                       actions), print (PrintTarget / PrintOptions / PrintJob), diagnostics (items, redactions,
                       manifest); desktop PathPick + DesktopHost pick_path / print_to_pdf / print_view_ready;
                       accounts SendAs / SendingMailbox; OutgoingTransport::send_with_progress
                       M5a (ADR 0102): appearance.rs (ThemeInfo / kinds / sources, AppearanceSettings = the
                       user's theme.kdl `extends` + `appearance` node, ThemeOverrides font-size / radius / accent,
                       active_theme rule, Density, token schema token_schema(), SystemAppearanceSource +
                       ManualSystemAppearance), icons.rs (Lucide BUNDLED_ICONS, LEGACY_ICON_NAMES, lucide_name;
                       fixtures/icons.json); ResolvedTheme.density, token variant `compact`
                       M5b (ADR 0109): config/layout.rs views (ViewDef: a view shows a layout window), default_view,
                       rail (RailItem, BadgeSource unread / confirmations), settings (SettingsGroup / SettingsSection
                       with a node tree), toolbar overflow (ToolbarItem::Action rank / display, ToolbarItem::More,
                       toolbar_settings hide_disabled / overflow), MenuItem::Header (+ ResolvedMenuEntry::Header);
                       views.rs (view ids, widget kinds nav-rail / view-host / settings-page / list-header, view /
                       settings / list / folder actions, ListFilter, FavoriteFolder, FolderPaneSectionKind,
                       folder_pane_sections, account_color_slot, overflow_order, step_view, UI-state keys); ui_state.rs
                       (`ui-state.json` keys / limits, UiStateIssue); GetUiState / SetUiState → UiState, UiStateChanged
                       M5c (ADR 0116): windows.rs (window ids `<kind>[:<instance>]`, WindowSpec modal / resizable /
                       sizes / pop_out / instances, DEFAULT_ / CONFIRMATION_WINDOW_SPEC, WindowFrame = `window` node
                       properties → ResolvedLayout.window_frames + window_spec(), WindowState / WindowGeometry per kind,
                       WindowStateFile = `window-state.json` v1, PopOut / PopOutChange / PopOutOpen / PopOutReport, issues);
                       GetWindowStates / SetWindowState / ResetWindowStates → WindowStates, WindowStateChanged; ListPopOuts /
                       PopOutWindow / ReturnPopOut / ClosePopOut / FocusPopOut → PopOuts, PopOutChanged; DesktopHost
                       open_pop_out / close_pop_out / focus_pop_out, DesktopChange::PopOut (no longer Copy);
                       M5c review (ADR 0122): pop_out is opt-in (DEFAULT_WINDOW_SPEC.pop_out = false, layout.kdl
                       `pop-out=#true`), PopOutWindow / PopOutOpen size + min_size (pop_out_sizes)
                       M5d (ADR 0131): editing.rs (LayoutPlacement into / beside + DockSide, NewWidget, ToolbarSlot bar /
                       more, ThemeEditTarget user / theme, ThemePatch set-token / set-part-property / reset-part,
                       ThemeDetails + ValueOrigin, HistoryScope layout / appearance, EditKind, EditIssue, EditEntry /
                       EditHistory, config_revision = FNV-1a 64); LayoutPatch move-pane / swap-panes / add-widget /
                       remove-node / set-hidden / apply-preset / reset-window / move- / add- / remove-toolbar-item;
                       M5d integration (ADR 0137): EditIssue::NotReady removed; menus.rs context part `nav-rail` (View menu)
                       (+ kind / kind_of); LayoutNode.hidden (is_hidden, at_path), LayoutPreset + PresetNode =
                       ResolvedLayout.presets; UpdateLayout.base_revision; GetEditHistory / UndoEdit / RedoEdit →
                       EditHistory, EditHistoryChanged; GetThemeDetails, EditTheme, DuplicateTheme / RenameTheme →
                       ThemeSaved, DeleteTheme
                       P3 (ADR 0164): mail_actions.rs (MessageTarget message / conversation = ConversationRef thread +
                       listed folder, MailAction, ConversationReach listed / whole / newest = MailAction::reach,
                       MailActionKind / MailActionIssue, MailActionResult + MailBatchId undo); ApplyMailAction /
                       ResolveMailTargets / UndoMailAction; MCP `conversations` args; remote_content FRAME_HEIGHT_MESSAGE /
                       RENDER_PARENT_PARAM / MAX_FRAME_HEIGHT
                       P2 (ADR 0148): domain ThreadParticipant ({name, email, me, unread}, address flattened) in
                       ThreadSummary.participants, ThreadSummary.latest_from / latest_message, ConversationMessage
                       (MessageSummary flattened + from_me); GetConversation {thread_id, folder_id?} → Conversation
crates/nail-config     theme.kdl / layout.kdl / accounts.kdl parsing, defaults, schema, watcher,
                       format-preserving accounts.kdl writer (AccountsDocument); widget kinds per
                       feature in src/widgets/; rules.kdl in src/rules.rs (WP22);
                       M2c: `sieve` node of an IMAP account, reset-toolbar / reset-key-binding
                       write-back, src/l10n.rs (fluent-bundle `Catalog` over every catalog, English
                       fallback; every ConfigError / rule issue carries a LocalizedMessage) (ADRs 0043, 0046);
                       M2d: `plugin-panel "<plugin>.<panel>"` + `plugin-manager` widgets (widgets/plugins.rs;
                       the pre-M2d `plugin.<id>.<name>` kind is refused), src/plugins.rs placement writer
                       (`placement_layout`, format preserving) + `InstalledPlugins` for merge conflicts
                       (`preview_merge_with`, `ConfigManager::set_installed_plugins / preview_placements`)
                       (ADRs 0050, 0055);
                       M3a: accounts.kdl `calendar-subscription` / `caldav` / `carddav` nodes (read, write,
                       schema; upsert/remove_pim_source), widgets/pim.rs (typed options of the PIM widgets
                       incl. `calendar-list`) (ADRs 0058, 0062);
                       M3b: account `gmail` node (oauth2 only, no smtp, plaintext only on loopback),
                       widgets/{scheduling,gmail}.rs (WP84 slots) (ADR 0065);
                       M3c: account `exchange` node (engine, host / url, polling, streaming, server-rules;
                       `auth "windows"` only there), widgets/exchange.rs (WP94 slot) (ADR 0072);
                       M4a: widgets/agents.rs (confirmation-center show-history, confirmation-indicator
                       hide-when-empty; ADRs 0079, 0084); M4b: widgets/system.rs (update-settings
                       show-installation, desktop-settings show-dev-endpoint, default-apps show-mapi;
                       ADRs 0087, 0092); M4c: identity `send-as=` / `sender=`, widgets/portability.rs + security
                       future widgets security-recovery-key / security-smartcards (ADR 0094);
                       M5a: themes.rs (embedded defaults/themes/*.kdl + `ThemeCatalog` over <config>/themes/*.kdl,
                       extends chains, ThemeProblem), appearance_edit.rs (comment-preserving writes of the
                       `appearance` node / override tokens), ConfigManager appearance / set_theme /
                       set_theme_preferences / set_system_appearance / reload_themes, ConfigUpdate::ThemeChanged
                       (before Changed), css::MODES + `compact`; tests/theme_contrast.rs (WCAG per theme, the
                       pair table every default part rule must stay within), tests/appearance.rs (ADRs 0102–0104);
                       M5b (ADR 0109): layout.rs `views` / `rail` / `settings-page` sections (replace the base as a
                       whole; checks: view window exists without a view-host, default / rail views exist, shell widgets
                       need their section), toolbar `hide-disabled=` / `overflow=`, item `rank=` / `display=`, `more {}`,
                       menu `header`; widgets/shell.rs (nav-rail, view-host, settings-page, list-header),
                       OptionType::Words (folder-tree sections / favorites, list-header chips); ui_state.rs
                       (ConfigManager::ui_state / set_ui_state, atomic `ui-state.json`); ConfigManager::appearance_for
                       (appearance + theme for another OS state, per remote session); render / write-back keep the M5b
                       toolbar properties
                       M5c (ADR 0116): `window` node properties size= / min-size= / modal= / resizable= / pop-out= /
                       instances= (LayoutFile.window_frames, merged field by field; render / merge / write-back keep them);
                       window_state.rs (ConfigManager::window_states / set_window_state / update_window_state /
                       reset_window_states, atomic `window-state.json`); tests/m5c_windows.rs
                       M5d (ADR 0131): `hidden=` on every layout node, `presets` section (preset / split / stack / tabs /
                       pane; replaces the base as a whole; panes a breakpoint lacks are no error), widgets/editors.rs
                       (layout-editor-settings, toolbar-editor-settings, theme-editor); edit_history.rs (undo history
                       per HistoryScope recorded by every theme / layout write, undo / redo with conflict check + force,
                       update_layout_checked base revision); slots of WP184: layout_edit.rs (the M5d patches, routed
                       from write_back.rs) and theme_edit.rs (theme_details / edit_theme / duplicate / rename /
                       delete_theme); tests/m5d_contract.rs
                       M5d (ADR 0136 WP184): layout_edit.rs (move / swap / dock / add / remove / hide, presets, reset-window,
                       toolbar bar<->More; marker properties `__nail_edit_*` never reach the file), theme_edit.rs (EditTheme
                       tokens / modes / parts, Duplicate / Rename (selection and extending themes follow) / Delete (fallback
                       by kind), GetThemeDetails origins, id_from_name = NFKD without marks like the UI's themeIdFromName),
                       kdl_edit detach / put / reindent / replace_with / set_arg; golden files
                       tests/golden/{layout_edit,theme_edit} (NAIL_UPDATE_GOLDEN=1)
crates/nail-store      SQLite metadata (v17), blob store, per-account operation queue with sync-side
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
                       detach bodies, fetch targets), lock.rs DataDirLock (ADR 0046);
                       v8: sieve_state.location / server_rules (SieveUpload), folder_marks
                       (the hidden Snoozed folder), wake_snoozed (records + queue in one
                       transaction), orphaned_in_folder, bounded junk training (ADR 0049);
                       v9: plugins.rs — installed plug-ins, grants, settings, per-plug-in KV storage
                       with quotas (QuotaExceeded), bounded logs; storage values + log messages
                       sealed at rest (ADR 0050);
                       v10: PIM — sources (local row), collections, raw objects (sealed), events /
                       tasks / contacts (data sealed), groups, links, instance cache, reminders,
                       pim_ops queue (ADR 0058); src/pim/ (WP73): collections, items (user writes + PIM
                       queue in one transaction, server merge with conflict copies), ops (replay), instances
                       (InstanceExpander, 90-day chunks, note_zone_data), contacts (duplicates, merge links,
                       Collected book, suggest_recipients), reminders, time (ADR 0061);
                       v11 (ADR 0065): messages.remote_id (MessageSource.remote_id), scheduling.rs (itip_state
                       moves forward only, received / sent iTIP messages, settings), gmail.rs (history id,
                       labels ↔ tags / folders, per-message labels + category, label op queue); summaries
                       carry `gmail` (attach_tags), MessageFilter::GmailCategory (ADR 0070);
                       src/pim/freebusy.rs (WP83 slot);
                       v12 (ADR 0072): pim_sources rebuilt (kind exchange + account_id; schema.rs
                       Migration::rebuild runs a step with foreign keys off + foreign_key_check), exchange.rs
                       (cursors per folder / mailbox / PIM collection, remote ids + change keys, mailboxes,
                       categories, automatic replies + exchange_ops payload sealed, rules, rooms, the Exchange
                       operation queue); Calendar / AddressBook `.exchange` read from exchange_pim_collections;
                       v13 (ADR 0078): exchange_sync.rs (full listings kept across passes: exchange_listed +
                       exchange_pim_collections.full_sync_page; exchange_removed = removals kept until the mailbox
                       was synced (moves by other clients); exchange_meeting_answers, comment sealed);
                       v14 (ADR 0079): agents.rs (confirmations: view + payload sealed, decided once, idempotency
                       key per actor, history 500; action_policies, `allow` refused for destructive kinds),
                       mcp.rs rate limits + mcp_idempotency (sealed), plugin_activity.rs (jobs, hook stats, CPU
                       per minute)
                       v15 (ADR 0094): crypto_keys rebuilt for `smartcard` keys (secret_ref = SmartcardKeyRef JSON);
                       messages.remote_thread_id (MessageMeta.remote_thread_id, set_message_remote_thread_id:
                       `remote-thread:<account>:<id>` in thread_refs);
                       M4c WP122 (ADR 0095): dbfile.rs full-file encryption (SQLite3 Multiple Ciphers via the root
                       `[patch.crates-io]` libsqlite3-sys = crates/nail-store/libsqlite3-sys-sqlite3mc, a stand-in with
                       the original crate's API over `sqlite3mc-src`), Store::open_with_key / rewrite_file (VACUUM INTO
                       + rename, crash safe) / file_encryption; thread/mod.rs subject fallback uses the partial index
                       (SUBJECT_REPLY_SQL / SUBJECT_ORIGINAL_SQL, EXPLAIN test; WP123 found the quadratic scan);
                       post-M4c (ADR 0101): BlobStore removal_mark / removed_since, sync lookups (message_id_by_uid,
                       message_exists, uidless_message_by_message_id with an EXPLAIN test, folder_uid_flags), one cached
                       upsert per message (row + subject key + meta);
                       P2 WP201 (ADR 0149): thread/conversation.rs (ConversationFolders threaded / singles / sent +
                       retain, OwnAddresses, list_conversations / conversation / summarize_conversation, single-<id> rows
                       for Drafts); v16 covering index messages_folder_thread(folder_id, thread_id, date_sort);
                       v17 (P3, ADR 0164): thread_activity (per folder + thread order key incl. the Sent join;
                       thread/activity.rs PAGE_SQL / COUNT_SQL; WP-perf maintains it from v18), mail_batches /
                       mail_batch_ops (mail actions with inverses for undo; mail_batches.rs)
                       v18 (P3 WP-perf, ADR 0166): five triggers keep thread_activity equal to the backfill
                       (activity.rs MAINTENANCE_SQL; a later migration that rebuilds messages / folders must recreate
                       them); ListThreads pages are index ranges on the table, one batched read per page
                       (listed_page_sql), narrowed remote Sent grants fall back to GROUPING_PAGE_SQL; activity_tests.rs
                       random-write consistency; P3 WP-actions-core (ADR 0165): thread/targets.rs conversation_targets
                       (every stored copy per reach), mail_batches.rs apply_mail_batch / undo_mail_batch (one
                       transaction, purged after 1 h / beyond 50, actor-owned), queue.rs group keys (replay hands
                       consecutive same-key operations to an engine at once; tests/queue_groups.rs)
crates/nail-mime       MIME parsing, HTML sanitizing, top-level header list (rules); message_file.rs
                       (read_message_file, localized MessageFileError, MAX_MESSAGE_FILE; ADR 0054); domain.rs,
                       auth_results.rs (RFC 8601, trusted authserv-id), links.rs (link / attachment
                       warnings) (ADR 0032); remote_content.rs: ProxyRewriter, parse_message_with
                       (remote references counted, rewritten to nail-proxy: URLs; ADR 0041);
                       markdown.rs: text/markdown alternatives as bodies (comrak; ADR 0046);
                       P1 (ADR 0140): sanitize_mail_html for mail bodies (keeps class / id and `<style>` blocks;
                       sanitize_html unchanged for plug-ins / quoting), document.rs (html5ever tokenizer: style blocks,
                       body attributes → `body` rules), css.rs (style-sheet filter: only @media / @supports, remote url()
                       counted / proxied, no position fixed / content text), cid: images → capped data: URLs (2 / 8 MiB)
                       only in ParsedMessage / MessageBody.reader_html (sanitized_html keeps cid: for quotes, MCP,
                       plug-ins; sanitize_mail_html_portable); no `{` / `}` in kept declarations (ADR 0145)
crates/nail-proto-imap IMAP client (IDLE/watcher, CONDSTORE/QRESYNC, writes, OAuth2, COMPRESS; ADR 0018);
                       bodies.rs (BodyFetch, fetch_batch_with, fetch_body), search.rs (SearchKey,
                       search_messages) (ADR 0046)
crates/nail-proto-smtp SMTP submission (lettre) → OutgoingTransport (ADR 0017)
crates/nail-proto-pop3 POP3 client (in-house) + leave-on-server policies (ADR 0017)
crates/nail-auth       OAuth2 (PKCE, loopback), token manager, keychain SecretStore (ADR 0017)
crates/nail-discovery  autoconfig / ISPDB / SRV / Autodiscover (ADR 0017); system_dns.rs SystemDns
                       (DnsQuery_W on Windows, no DNS sockets; mock DNS bound to loopback; ADR 0057);
                       M3b: Gmail API candidate first (DiscoverySource::Provider) for gmail.com / googlemail.com /
                       Google MX (ADR 0070)
crates/nail-search     Tantivy index + query language parser (ADR 0016); open_with(DirectoryWrapper)
                       for the sealed index, IndexPolicy (decrypted indexing opt-in) (ADR 0032);
                       imap.rs: query → IMAP SEARCH key (needs_server, server_search) (ADR 0046);
                       pim.rs: PimIndex (events, tasks, contacts, groups; the core keeps it in memory)
                       (ADRs 0061, 0063)
crates/nail-testkit    mock IMAP server (SEARCH incl. TEXT / BODY / OR / NOT), .eml corpus, demo seed;
                       corpus/crypto/{pgp,smime}: end-to-end
                       test vectors (not demo mail; `crypto_fixture`; each has generate.sh + README;
                       smime/ada-{work,home}.p12 are the demo-multi certificates);
                       bin nail-fake-installer (M4b: the updater smoke's "package"; records its command line
                       to NAIL_FAKE_INSTALLER_RECORD instead of installing; ADR 0092);
                       conversations/ = P2 demo conversations (ADR 0152: demo_corpus_with_conversations,
                       conversation_corpus, MailStore::demo_with_conversations, DEMO_CONVERSATIONS; shown by the seeds of
                       nail serve through nail_core::demo::DemoMail; demo_corpus() / MailStore::demo() unchanged)
crates/nail-testkit-net mock SMTP / POP3 / OAuth2 (add_refresh_token: start signed in) / HTTP / DNS servers; managesieve.rs:
                       MockSieveServer + validate_script (ADR 0044); dav.rs: MockDavServer (CalDAV / CardDAV,
                       sync tokens, TLS, server-side changes for tests) (ADR 0060); dav/schedule.rs + ical.rs:
                       several principals, schedule inbox / outbox, free/busy POST, switchable
                       auto-scheduling (ADR 0068)
crates/nail-sieve      compiler (rules + vacation → one Sieve script, placement + localized reasons),
                       parse / decompile round-trip harness, ManageSieve client (SieveSession,
                       SieveAuth), upload / remove of the managed script (ADR 0044)
crates/nail-triage     pure logic: tokenizer, junk classifier, snooze scheduler (timer wheel),
                       notification policy + batcher, local times; writes as TriageOp (ADR 0045);
                       reminders.rs: plan_alarms, ReminderScheduler (restart catch-up, snooze / dismiss
                       as ReminderOps), reminder notifications (ADR 0061)
crates/nail-plugins    plug-in host (M2d WP61): PluginHost (one thread + QuickJS runtime per plug-in, CPU
                       budget per call, memory limit, crash isolation), gate.rs permission gate, services.rs
                       PluginServices (implemented by nail-core), prelude.js (`nail.*` API), package.rs
                       (.nailplugin checks, PackageStore keeps approved archives packed and verified by
                       hash), dev.rs (hot reload), l10n.rs (PluginCatalogs) (ADR 0051); M3a:
                       `nail.calendar.events` behind `calendar.read` (ADR 0063); M4a (WP102, ADR 0081):
                       writes.rs (PluginWrite = the MCP tool argument types, through PluginServices::write),
                       hooks.rs (arrival / beforeSend / open with budgets, banner buttons), extensions.rs
                       (composer edits, previews, attachment and link actions, Markdown blocks, MCP tools),
                       jobs.rs (cron on the core's clock, events, CPU quota), nail.l10n, binary fetch bodies
                       ($nailBytes markers via hostPutBytes / hostTakeBytes), panel actions checked against
                       the rendered tree; post-M4c (ADR 0101): reloads serialized, watchers in their own map,
                       PluginHost::subscribe_reloads (PluginReload notices; tests wait on them)
crates/nail-ical       iCalendar / vCard (own lossless content-line layer + patching writers; calcard
                       only for the Windows zone table and VTIMEZONE generation), own recurrence engine
                       (recur.rs, expand.rs instances, edit.rs scopes), tz.rs (IANA/Windows/custom VTIMEZONE
                       matching), alarm.rs, feed.rs (webcal), import.rs previews, issues.rs (ical-*/vcard-*
                       in core-ical.ftl); tests/corpus (synthesized Google/Outlook/Apple/Thunderbird exports,
                       nested .gitattributes -text: edit byte-preserving) (ADR 0059)
crates/nail-dav        CalDAV / CardDAV client (M3a WP72): DavClient (Basic over TLS / OAuth2 bearer with one
                       refresh, https-or-loopback URL policy, own redirect handling), discovery (configured
                       URL, _caldavs/_carddavs SRV via DnsLookup, /.well-known/, principal + home sets, Google
                       endpoints), PROPFIND / REPORT (calendar-query + time-range, multiget, addressbook-query,
                       sync-collection), sync_collection (RFC 6578 with paging and token reset, ctag/ETag
                       fallback), write_object / delete_object (If-Match / If-None-Match, 412 → server wins),
                       MKCALENDAR / MKCOL / PROPPATCH; DavError → ErrorCode + dav-* texts (ADR 0060);
                       M3b (WP83, ADR 0068): scheduling.rs (discover_scheduling, outbox free/busy POST,
                       CalDavFreeBusy, schedule inbox, SCHEDULE-AGENT / SCHEDULE-STATUS), freebusy.rs
                       (VFREEBUSY build / parse, own_busy_periods), icalendar.rs (content lines, patch_lines)
crates/nail-itip       iTIP / iMIP (WP81, ADR 0066): object (METHOD wins, per-component reading), address,
                       order (SEQUENCE/DTSTAMP), card (role, spoofing, state, actions), apply, build
                       (REPLY/COUNTER/REFRESH/CANCEL/REQUEST/DECLINECOUNTER/delegation), imip (find_itip,
                       OutgoingImip), texts (itip.ftl via Localizer), conflicts, server (RFC 6638
                       SCHEDULE-AGENT); tests/fixtures = synthesized Google/Outlook/Apple/Thunderbird .eml
crates/nail-proto-gmail Gmail API engine (WP82, ADR 0067): client.rs (GmailClient, OAuth bearer with one refresh,
                       RetryPolicy backoff / Retry-After), messages / batch / history / labels / threads /
                       mapping, engine.rs GmailEngine (MailApiEngine + OutgoingTransport, for_account), poll.rs
                       PollTimer, labels::LABEL_PALETTE; feature `testing` = MockGmailServer (in-memory Gmail,
                       injected errors, quota, request log) + fixtures/ (DEMO_MAILBOX, recorded responses)
crates/nail-proto-graph Microsoft Graph engine (WP91, ADR 0073): client.rs (GraphClient: OAuth bearer with one forced
                       refresh, 429 / 503 / 504 with Retry-After, long waits → RateLimited, no repeat of sends / creates
                       after a 5xx, paging / delta links checked against the API origin), batch.rs ($batch, 20 per call),
                       mail / settings / directory / pim modules, engine.rs GraphEngine (ExchangeEngine, for_account;
                       extra: copy_message, attachments, folders, find_meeting_times, …), poll.rs (PollTimer, FolderPoll);
                       feature `testing` = MockGraphServer (in-memory tenant, delta tokens + expiry, $batch, injected
                       errors, request budget, request log) + fixtures (DEMO_TENANT, recorded errors)
crates/nail-proto-ews  EWS engine (WP92, ADR 0074): soap.rs (Exchange2016, falls back to Exchange2013_SP1; faults incl.
                       ErrorServerBusy with back-off), client.rs (EwsClient, lanes = one HTTP/1.1 connection each, Basic /
                       OAuth2 bearer / injected NegotiateAuth bound to the connection, RetryPolicy), mail.rs
                       (SyncFolderHierarchy / SyncFolderItems, MimeContent, Create/Update/Move/Copy/DeleteItem), notify.rs
                       (streaming subscriptions, StreamingWatcher), calendar / recurrence / tasks / contacts.rs
                       (nail_api::exchange::pim shapes), settings.rs (OOF, inbox rules, CategoryList), directory.rs (rooms,
                       GetUserAvailability, ResolveNames, GetDelegate), engine.rs EwsEngine (ExchangeEngine, for_account);
                       feature `testing` = MockEwsServer (in-memory organization, scripted Negotiate, faults, streams,
                       recorded answers in testing::fixtures)
crates/nail-exchange   Exchange common layer (WP93, ADR 0075): autodiscover/ (v2 + POX + HTTP method + SRV, ConfirmRedirect /
                       NeedsLogin, engine choice), http.rs (ExchangeHttp, ReqwestHttp::route_all_to), negotiate.rs (401
                       handshake, Basic over TLS), sspi.rs (SspiNegotiate: NegotiateAuth over SSPI; real in-process
                       handshake tests; ignored live test tests/sspi_live.rs with NAIL_EWS_LIVE_URL), mapping/ (categories,
                       focus, rules, oof, events, tasks, contacts, rooms, text, time, windows_zones = IANA → Windows ids
                       for EWS writes; ADR 0078), texts.rs (exchange-autodiscover-*,
                       -signin-*, -rule-reason-*, -oof-note-*, -pim-reason-*); feature `testing` = MockAutodiscoverServer
                       (multi-host, Basic / scripted Negotiate, request log)
crates/nail-compose    composer: MIME (mail-builder), Markdown (comrak), CSS inlining, quoting (ADR 0023)
crates/nail-rules      rules engine: conditions, actions → planned queue ops with inverses, dry runs (ADR 0024)
crates/nail-mcp        MCP server (rmcp): Streamable HTTP on loopback, grants, audit; McpData trait +
                       CoreData adapter, admin requests, `nail mcp` stdio bridge (ADR 0025);
                       messages.rs (localized texts), configured_port (ADR 0046); pim.rs: read-only PIM
                       resources (nail://calendars, …/events, contacts, tasks) and tools (list_events,
                       find_free_time, search_contacts, list_tasks) via McpData::pim / pim_raw (ADR 0063);
                       M4a: admin `UpdateMcpRateLimits` (ADR 0079); WP101 (ADR 0080): writes/ (write tools:
                       plan → policy → confirmation → McpData::write through the op queue; idempotency, audit
                       before / after without mailbox content), broker.rs (ConfirmationBroker + store-backed
                       StoreBroker), limits.rs (per-client rate limits), prompts.rs (four prompts, core-mcp.ftl);
                       McpData::{write, query, confirmations, render}, CoreAccess::{handle_write, confirmations,
                       render}; McpServer::start_with(WriteSettings); WP105 (ADR 0084): writes/actor.rs
                       ActorWrites (plug-in writes through the same plans, PimDelete), McpData /
                       CoreAccess::{plugin_tools, call_plugin_tool} (plug-in tools in tools/list by scope)
crates/nail-pgp        OpenPGP (rPGP): keyring (S2K secrets: passphrase or keychain:nail/openpgp/<FPR>),
                       PGP/MIME sign/encrypt/decrypt/verify, inline read, header protection, Autocrypt
                       L1, WKD + keys.openpgp.org via injected HttpClient (ADR 0030); Error::localized()
                       (`pgp-*` in core-crypto.ftl), Error::NotBase64 (ADR 0054), Error::Problem
                       (Nail's own `pgp-problem-*` texts; ADR 0056)
crates/nail-smime      S/MIME (RustCrypto cms/x509-cert): sign/encrypt/verify/decrypt, chain validation
                       (TrustAnchors), PKCS#12, keyring, Windows cert store + CNG keys (ADR 0031)
crates/nail-vault      encryption at rest: sealed format (XChaCha20-Poly1305), data key in the keychain,
                       SealedDirectory (tantivy), Vault migration plain <-> encrypted (ADR 0032);
                       M4c (ADR 0095): vault.json (vaultfile.rs: keychain ref, key check, data key wrapped by the
                       recovery key), recovery.rs (create / verify / rotate / restore / remove), datadir.rs
                       open_data_dir → Ready {store, blobs, vault} | Locked(LockedVault), Vault::with_full_file_encryption
                       / encrypt_database_file (the database file key is derived from the data key)
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
                       paths::lock_data_dir, strings.rs over nail_config::l10n;
                       M2d (ADR 0055): plugin_host.rs (PluginsCtl: one PluginHost, PackageStore in
                       <data-dir>/plugins, catalogs; CoreServices = PluginServices over store / search, never
                       decrypted bodies, ordered log writer, crash errors recorded, quiet hours for `system`
                       notifications), plugin_http.rs (SSRF-safe `nail.http.fetch`), plugins.rs (every
                       plug-in request; commands / UI actions answer Done at once);
                       M3a (ADR 0063): pim/ (mod.rs routing + PimCtl, sources.rs (accounts.kdl sources,
                       collections, local defaults, conflicts), engine.rs (ICS / CalDAV / CardDAV engines,
                       PimReplayer), feed_http.rs (SSRF-safe subscription fetch, ADR 0064),
                       events.rs (edit scopes, instances, free time, tasks), contacts.rs,
                       import.rs (imports, event / task from email), reminders.rs (scheduler + task),
                       search.rs (in-memory PimIndex)); launch_args.rs (launch-argument parser, shared with
                       app/src-tauri and `nail serve --launch`);
                       M3b (ADR 0070): scheduling.rs (cards on GetMessage / GetInvitation, arrival processing
                       in the rule-inbox pipeline, answers / organizer flows, iMIP through the outbox as
                       `AutomaticMail::Itip` prepared mail or server scheduling — never both, free/busy own +
                       CalDAV outbox, schedule inbox per CalDAV pass), gmail.rs (Gmail engine loop: labels,
                       full listing with resume / merge, history deltas, PollTimer; GmailExecutor replay; label
                       queue; sending through the API; TestAccount); pim/engine.rs stores RFC 6638 scheduling
                       info with the DAV discovery (`source_scheduling`);
                       M3c (ADR 0077): exchange/ (mod.rs the 18 requests + ExchangeCtl / ExchangeHooks, setup.rs
                       engines (Graph / EWS, SSPI), TestAccount, Autodiscover in DiscoverAccount, sync.rs the engine
                       task (mail per folder, ExchangeExecutor replay, Exchange queue, Graph polling / EWS streaming),
                       settings.rs (categories, automatic replies, rule placement as inbox rules, vacation mapping),
                       pim.rs (`exchange:<account>` source, ExchangeReplayer: meetings through the meeting APIs));
                       `focus:` (search filter, rules' message view);
                       M4a (ADRs 0079, 0084): agents.rs (confirmations, policies, plug-in activity; the one
                       StoreBroker `confirmation_broker`; `actor_writes` / `actor_write` = plug-in writes through
                       nail_mcp::ActorWrites; `send_draft_checked` before-send hooks, veto map + OverrideSendVeto,
                       added content into the draft, `X-` headers via add_headers in outbox.rs; annotations / previews
                       / actions / job runs through the host; `forget_actor` on revoke / uninstall;
                       render_plugin_blocks at send time), plugin_host.rs CoreServices M4a methods +
                       `Service::clock_moved` (due jobs), rules.rs plugin_arrivals (hooks after the user's rules),
                       context.rs / outbox.rs / pim/reminders.rs plug-in events;
                       M4b (ADR 0087): desktop.rs (desktop requests: app_settings `desktop.*` + the shell's
                       DesktopHost from ServiceOptions::desktop, `unsupported` without; Service::submit_mapi =
                       Simple MAPI → saved draft + LaunchRequest::ComposeDraft, never sent; desktop_settings())
                       M4c (ADRs 0094, 0099): palette.rs (recent actions), print.rs (PrintCtl jobs, PDF via the host),
                       portability/ (mod.rs requests; jobs.rs PortabilityCtl: jobs on the blocking pool + async finish,
                       progress / finished events; mail.rs ImportTarget = MailImportSink (Local Folders tree, or an
                       account's existing folders + AppendMessage, `folder-merged`) and StoreMailSource; pim.rs PimBuffer
                       → import_pim, StorePimSource; backup.rs CoreBackupSource / CoreRestoreTarget (settings.json,
                       keyring.json with keychain secrets, plug-in packages reinstalled disabled); diagnostics.rs
                       collection + DiagnosticsCache (the bundle = the preview)), logs.rs (TeeWriter ring of recent log
                       lines; the binaries tee tracing through it), smartcard.rs (SmartcardCtl = the keyrings'
                       KeyResolver, CorePinSource over PinRequested / AnswerPinPrompt / PinPromptClosed, poller →
                       SmartcardsChanged; recovery-key requests), vault.rs (open via nail_vault::open_data_dir, locked
                       mode = in-memory stand-in store, engines off, restart after RestoreVaultKey
                       (`vault-restart-needed`), encrypt_database_file once after start), api_moves.rs (moves between
                       Exchange / Gmail accounts via nail_exchange::transfer, IMAP/POP3 → API accounts), demo.rs
                       FileSecretStore (`<data-dir>/demo-keychain.json` for seeded runs, demo-multi too);
                       post-M4c (ADR 0101): pipeline.rs (overlapping fetch → prepare → store, used by sync.rs
                       store_batches; MailChangedThrottle), ingest::prepare (blob writes + MIME parsing on worker
                       threads) / ingest_prepared, Service::subscribe_plugin_reloads;
                       M5a (ADRs 0102, 0104, 0107): appearance.rs (ListThemes / SetTheme / SetThemePreferences /
                       ReportSystemAppearance; Follow Windows: Service::open keeps a SystemAppearanceWatch on
                       ServiceOptions::system_appearance, else the Windows registry watcher; reports apply only
                       without a watch), attachments.rs (GetMessageAttachment / SaveAttachment); demo_options
                       installs a light ManualSystemAppearance, demo-multi control `system-appearance`;
                       M5b (ADR 0109): ui_state.rs (GetUiState / SetUiState, UiStateChanged); per-session Follow Windows
                       (remote sessions: ReportSystemAppearance kept per session in Inner.session_appearance,
                       Service::session_appearance; GetConfig / theme answers / ThemeChanged / ConfigChanged computed
                       per session in remote_access.rs)
                       M5c (ADR 0116): windows.rs (window-state requests, pop-out list Inner.pop_outs, PopOutWindow /
                       Return / Close / Focus through the DesktopHost, DesktopChange::PopOut reports keep the geometry per kind)
                       M5d (ADR 0131): editing.rs (GetEditHistory / UndoEdit / RedoEdit, the theme editor's requests;
                       broadcast_history after every theme / layout write; the watcher sends EditHistoryChanged when the
                       files change on disk)
                       P2 (ADRs 0148, 0149): conversations.rs (own_addresses_by_account = "me" from accounts.kdl addresses +
                       identities + stored accounts; list_conversations / get_conversation with FolderFilter; conversation_row /
                       thread_row rebuild rows from the granted folders for remote sessions); demo.rs DemoMail (Corpus /
                       WithConversations: the seeds of `nail serve` add nail-testkit's demo conversations; seed_demo_with,
                       start_demo_imap_with, start_demo_multi_with);
                       P3 (ADR 0164): mail_actions.rs (ApplyMailAction / ResolveMailTargets / UndoMailAction; conversation
                       targets resolved per reach, within a remote grant); WP-actions-core (ADR 0165): one batch per
                       action sorted by (account, folder), undo per actor (`user`, `remote:<session>`, `mcp:<client>`,
                       `plugin:<id>`), plan_snooze / plan_mark_junk (undo cancels wake timers / training), replay.rs
                       grouped IMAP UID STORE / MOVE per folder, gmail.rs + exchange/sync.rs grouped writes,
                       exchange/batch.rs (RunningEngine.batch = the concrete Graph / EWS engine for write_many);
                       tests/mail_actions.rs
crates/nail-mapi       Simple MAPI provider (M4b WP111): cdylib nail_mapi.dll (shipped as mapi\<arch>\NailMapi.dll)
                       + rlib + bin nail-mapi-harness (send / documents / exports / registry; without arguments a
                       fake Nail.exe on NAIL_MAPI_PIPE); windows/exports.rs (13 Simple MAPI exports, signatures
                       checked against windows-sys LPMAPI*), windows/decode.rs + memory.rs (VirtualQuery-checked
                       reads of caller structs), windows/client.rs (named-pipe client, same-user server check,
                       starts Nail.exe — never for an elevated caller; NAIL_MAPI_PIPE / NAIL_MAPI_APP only with
                       the test-only feature `test-overrides`, which the crate's own tests and the e2e build
                       enable), windows/server.rs (reference pipe server), windows/staging_root.rs (RootGuard:
                       staging root not a reparse point, owned by user / Administrators / SYSTEM, pinned),
                       forward.rs, staging.rs (attachment copies in %TEMP%\nail-mapi, sweep of own
                       `<pid>-<hex>-<n>` folders only, limit nail_api::compose::MAX_ATTACHMENT_BYTES),
                       registration.rs (HKLM Clients\Mail\Nail values, .reg rendering) (ADRs 0087, 0088, 0093)
crates/nail-portability M4c WP121 (ADR 0096): importers (mbox streamed: mboxrd / mboxo / mboxcl(2), EML folders,
                       Maildir incl. Dovecot / Courier keywords, Thunderbird profiles: profiles.ini, prefs.js account
                       proposals, Local Folders / POP3 mail, abook.sqlite / history.sqlite / Mork .mab address books,
                       msgFilterRules.dat rule proposals; ics / vcf via nail-ical), exporters (mbox / EML / Maildir / ics /
                       vcf, temp file + rename), `.nailbackup` (backup/: Argon2id + XChaCha20-Poly1305 STREAM over a zip
                       with manifest.json first, random-access reading, restore plan = dry run, restore report),
                       diagnostics.rs (Redactor + bundle writer); the core implements the traits in sink.rs
                       (MailImportSink, PimImportSink, MailExportSource, PimExportSource) and backup/ (BackupSource,
                       RestoreTarget); job.rs JobContext (cancel flag, throttled progress) + finish_report; feature
                       `testing` = KdfParams::insecure_for_tests; fixture tests/fixtures/thunderbird (SQLite books
                       regenerated with NAIL_REGENERATE_FIXTURES=1); post-M4c (ADR 0101): files.rs (`<name>.nail-part`
                       + rename; an existing target is replaced only with `overwrite`)
crates/nail-pkcs11     M4c WP122 (ADR 0095): PKCS#11 smartcards over cryptoki: Module / CryptokiModule, Smartcards /
                       CardKey (PIN flow: retry, last try, locked, cancel; PIN never stored; login cache),
                       BlockingPinSource (adapts the async PinSource; card operations run on blocking threads),
                       feature `testing` = SoftToken (in-memory RSA / P-256 token); nail-pgp / nail-smime smartcard.rs
                       (card keys; Keyring::with_smartcards / use_smartcard_key)
crates/nail-bench      M4c WP123 (ADR 0097): corpus.rs (deterministic 100k corpus), mock.rs (mock IMAP server in a
                       child process), run.rs (metrics through Service; the account is handed over as local so
                       `nail serve` shows the data), memory.rs; bin nail-bench (run / check / corpus / mock-imap);
                       budgets = e2e/perf/budgets.json
crates/nail-server     axum: UI bundle, WebSocket API, blobs, test controls (demo-multi only);
                       M2b: bind_with (TLS rustls + ring, rcgen certs, instant-acme, headers,
                       sandbox origin, proxy route, `auth`), remote_ui.rs RemoteUiHost (ADRs 0038, 0041);
                       lan.rs: LAN address from the interfaces, no socket (ADR 0057);
                       M2c: render tokens bound to the session (sandbox.rs), proxy_limit.rs (ADR 0048);
                       P3 (ADR 0168): sandbox.rs HEIGHT_SCRIPT (hash-pinned script-src, posts the height to `parent`,
                       which must be a frame-ancestor, else 400)
crates/nail-cli        `nail` binary; messages.rs (localized stderr), headless `nail mcp`
                       (`--config-dir`, `--no-start`, `--mcp-port`; prints waiting confirmations to stderr, M4a),
                       serve holds the data-dir lock (its clock-set / clock-advance controls run due plug-in jobs);
                       plugin.rs: `nail plugin dev <folder> [--url|--data-dir]` over the running serve's
                       WebSocket (`nail serve` writes <data-dir>/ui-endpoint.json) (ADR 0055);
                       `nail serve --launch <file-or-link>` (M3a)
app/src-tauri          Tauri 2 desktop shell (starts the MCP server on the settings' port and
                       restarts it on change, and the remote-UI host; data-dir lock; OS notifications
                       for NotificationRequested; main window created in code; nail-proxy protocol;
                       single instance); launch.rs,
                       portable.rs, setup_helper.rs; windows/ (installer sources: nsis/ template copy +
                       hooks + strings (ADR 0040), wix/nail.wxs MSI fragment + bundle.json (upgrade code)
                       (M4b WP112, ADR 0089); tests/installer.rs checks them);
                       M4b shell (WP113, ADR 0090): registry.rs (Registry trait; WindowsRegistry with
                       NAIL_REGISTRY_ROOT; MemoryRegistry for seeded runs), install.rs (InstallationInfo, HKLM
                       facts before HKCU, UpdateSource from plugins.updater with {{channel}} or
                       <channel>/latest.json, NAIL_UPDATE_ENDPOINT / NAIL_UPDATE_PUBKEY loopback-only test
                       override); every test variable only with the feature `test-overrides`
                       (shell::test_env; e2e desktop build only, ADR 0093), default_apps.rs,
                       startup.rs (Run key; the MSI's machine-wide entry counts, never duplicated),
                       updater.rs + updater_tauri.rs (update states over tauri-plugin-updater; installs a
                       downloaded update on restart or quit), dev_endpoint.rs (127.0.0.1, bearer token, no
                       Origin), mapi_server.rs (named pipe, first instance, current-user DACL; waits for the
                       client to close before disconnecting), win.rs, host.rs (ShellHost: DesktopHost), tray.rs,
                       shell.rs (tray menu, second start, seeded-only test control `nail_test_shell`);
                       tests/mapi_wired.rs (DLL client → pipe server → core; ADR 0092);
                       M4c (ADR 0099): printing.rs (PDF: hidden window `print-<job>` on `#/print/<job>`, waits for
                       PrintViewReady, WebView2 PrintToPdf via webview2-com), dialog.rs (PickPath: Windows common item
                       dialog on an STA thread); capabilities cover `print-*` windows;
                       background.rs (ADR 0146): NAIL_TEST_BACKGROUND_WINDOWS=1 (startDesktop sets it) = every
                       window DWM-cloaked, WS_EX_NOACTIVATE, off the taskbar, never set_focus; no notifications /
                       file dialogs / Settings app; test control `windows` + e2e expectWindowsInBackground;
                       M4b deps: tauri tray-icon, tauri-plugin-updater (rustls / ring) (ADR 0087)
packaging/             M4b WP112 (ADR 0089): updater/dev.pub (development update key; private keys never in the
                       repo), winget/ facts, release-notes/<version>.md (written before a release); README =
                       key handling
ui/                    React renderer; feature slots in ui/src/features/<name>/ (README there;
                       M2a: security/; M2b: remote/, launch/; M2c: sieve/ (status, vacation),
                       triage/ (junk, snooze, notifications), sync/ (policies, bodies), menus/
                       (context menus, resolve.ts), customization/Editors.tsx; M2d: plugins/;
                       M3a: pim/ (calendar, contacts, tasks; lazyDialogs.ts, parts.ts PIM_BASE_CSS,
                       @internationalized/date + react-aria-components for the date / time picker;
                       ADRs 0058, 0062); M3b: scheduling/ (InvitationCard, AttendeePicker, MeetingPanel,
                       free/busy timeline), gmail/ (label list / editor, category tabs, MessageLabels)
                       (ADR 0069)); M3c: exchange/ (WP94, ADR 0076: mailboxes, categories, Focused tabs, automatic
                       replies, rule badges, room finder, To Do chooser, setup / settings parts; EXCHANGE_BASE_CSS);
                       M4a (ADR 0083): agents/ (confirmation center + dialog, indicator, notices, PolicyEditor,
                       AGENTS_BASE_CSS; model.ts mirrors nail_api::agents, mock.ts), plugins/ extensions
                       (extensions.ts, MessageAnnotations, AttachmentPreview, ActionMenus, ComposerExtensions,
                       SendVeto (sendDraftChecked), Activity; extensionsMock simulates the host, extensionsSeed),
                       mcp/ write scopes + RateLimits; M4b: system/ (widgets update-settings, desktop-settings,
                       default-apps; dialogs about-nail (F1), first-run; FirstRunGate; ProgramDraftNotice in the
                       composer + openProgramDraft for ComposeDraft launches; SYSTEM_BASE_CSS; mock.ts simulates the
                       desktop host: simulateMapiSendMail, setUpdateState, setDefaultApps; ADRs 0087, 0091, 0092);
                       M4c (ADRs 0094, 0098): portability/ (import / export / backup / restore wizards, diagnostics,
                       jobs + reports, PathField; widgets import-export / backup-restore / diagnostics), palette/
                       (command-palette dialog, palette.open = Ctrl+K, fuzzy.ts, builtinInfo.ts), print/ (print dialog,
                       PrintHost renders `#/print/<job>` with `<html data-nail-mode="print">`, printSheet,
                       PORTABILITY / PALETTE / PRINT_BASE_CSS in theme/compile.ts), security/ (recovery-key widget +
                       dialogs, RecoveryGate, smartcards widget, smartcard-pin dialog; vaultMock.ts);
                       actions/palette.ts (ActionInfo, buildPalette) + Feature.actionInfo;
                       WP105 mounts: composer/pluginExtensions.tsx, reader in
                       widgets/MessageView.tsx (ADR 0084);
                       M5a (ADRs 0102, 0105, 0107): appearance/ (appearance-settings widget, SystemAppearanceReporter,
                       mock.ts mirrors nail_api::appearance and resolves defaults/themes/*.kdl via Vite ?raw),
                       theme/skin.ts (token-only default look of the primitives, between BASE_CSS and the theme's
                       part rules — the base theme has no rules for those parts), widgets/lucideIcons.ts + Icon.tsx
                       (theme icon → Lucide → nothing), widgets/primitives.tsx (Switch, Segmented, Tag, Avatar),
                       widgets/AttachmentCard.tsx + attachmentSave.ts (PickPath → SaveAttachment, else download);
                       M5b (ADR 0109): features/shell/ (WP161: nav-rail, view-host, view.show / next / previous with
                       the open-the-window fallback), features/settings/ (WP163: settings-page, settings.section),
                       widgets/feature.ts (WP162's registrations, merged like a slot: list-header, list / folder actions),
                       per-package skins shell/settings/pim `skin.ts` after theme/skin.ts; layout/views.ts,
                       layout/overflow.ts (overflowOrder, fitToolbar, shownToolbarItems), layout/settingsIndex.ts
                       (Feature.settings → search index), widgets/folderPane.ts (sections, account colour slots),
                       state/uiState.ts (useUiState / useSetUiState; App.tsx applies UiStateChanged), api/uiStateMock.ts,
                       UiState.activeView / listFilter, ActionInfo.icon (every registered action has an icon),
                       menu headers (`context-menu.header`), appearance applyAppearance (reporter applies its answer);
                       M5b packages: features/shell = NavRail.tsx / ViewHost.tsx / badges.ts / model.ts, skin
                       SHELL_SKIN_CSS (ADR 0110); widgets/ Toolbar.tsx + toolbarModel.ts (measured overflow into More,
                       rank=1 primary), FolderTree.tsx + folderPaneState.ts (grouped pane), ListHeader.tsx + listFilter.ts,
                       proposalToolbars.ts (ADR 0111); features/settings = page widgets, model.ts, store.ts, builtins.ts,
                       formerTabs.json (ADR 0112); features/pim CalendarList.tsx / PimMenu.tsx / CalendarDialogs.tsx /
                       PimSettings.tsx / colors.ts, skin PIM_SKIN_CSS (ADR 0113); M5b integration (ADR 0114):
                       customization/LayoutMigration.tsx (offers the new default to a pre-rail layout once, UI-state key
                       `customize.layout-offer`, merge dialog prefilled with defaults/layout.kdl via ?raw);
                       i18n/localizer.ts availableLocales (browser languages only against non-empty catalogs);
                       M5c (ADR 0116): state/windows.ts (window ids / specs / checks mirrored from nail_api::windows,
                       resolveWindowSpec = defaults → Feature.windows → layout window_frames, WindowInstanceContext /
                       useWindowInstance, pop-out routes `#/window/<id>`), state/windowState.ts (useWindowStates /
                       useSetWindowState / usePopOuts / usePopOutActions; App.tsx applies WindowStateChanged /
                       PopOutChanged), api/windowStateMock.ts (enableMockPopOuts / simulatePopOutReport), UiState.windows =
                       window ids in z-order + focusWindow, UiState.composers (per instance), Feature.windows; slots
                       features/windows (WP171), features/popout (WP173: PopOutHost rendered by App.tsx on a pop-out route,
                       with its own WindowLayer `main=<window>`; OS close → DOM event POP_OUT_CLOSE_EVENT, ADR 0122);
                       M5c packages / integration (ADRs 0117–0121): layout/WindowLayer.tsx (layer, z-order, modality with
                       owners `modalOwner`, Ctrl+F6, openingOrder), layout/WindowFrame.tsx (frame, title bar + PopOutButton,
                       8 handles, snap preview, keyboard move / size, debounced saves; tabbables / trapTab),
                       layout/windowSpecs.ts (WindowSpecsContext, WindowTitlesContext = Feature.windowTitles),
                       features/windows/{geometry,commands,useViewport,skin}.ts (WINDOWS_SKIN_CSS); features/composer
                       sessions.ts (composer:<key> windows, UiState.composer = focused composer mirror, close guard),
                       ConfirmDialogs.tsx, popOut.ts (preparer / pop-out page / return), windowOf.ts (composerOf,
                       useComposerWindowOf), skin.ts (COMPOSER_SKIN_CSS); features/popout (PopOutButton, PopOutRouter,
                       hooks, model, parts POPOUT_BASE_CSS); widgets/DeletePermanently.tsx; features/plugins/windows.ts
                       (plug-in dialogs `plugin-dialog|modal-<size>:<key>`, `plugin-send-veto`);
                       features/remote/ReauthWindow.tsx (`remote-reauth`); src/m5c.test.tsx (every window);
                       M5d (ADR 0131): state/editHistory.ts (useEditHistory / useEditHistoryStep, App.tsx applies
                       EditHistoryChanged), layout/LayoutChrome.ts (LayoutChromeContext showHidden / renderNode around
                       every rendered node, nodeRefOfPath; the renderer skips `hidden` nodes), api/editHistoryMock.ts
                       (mock histories, simulateExternalEdit); features/layout-editor (WP181, ADR 0133: customize mode =
                       LayoutChrome per window, pane headers / drop targets / palette / presets, keyboard pick-up, store.ts
                       startEditing, SettingsCard `layout-editor-settings`, mock.ts applyLayoutEditPatch, LAYOUT_EDITOR_SKIN_CSS),
                       features/toolbar-editor (WP182, ADR 0134: right-click → `toolbar-customizer:<toolbar>` window (lazily
                       loaded, like the palette), model.ts placement rules, Settings card `toolbar-editor-settings`, mock.ts
                       applyToolbarEditPatch, TOOLBAR_CUSTOMIZER_SKIN_CSS), features/appearance (WP183, ADR 0135: sliders /
                       colour picker / undo on the page, theme editor window `theme-editor` + `theme-new` / `theme-rename` /
                       `theme-delete`, themeEditMock.ts, APPEARANCE_SKIN_CSS); features/palette/infos.ts (paletteInfos /
                       pluginTitles shared with the customizer); the three skins are in theme/compile.ts (ADR 0137);
                       P1 (ADRs 0140–0144): theme/reader.ts (mail's own rules win; "designed" mail keeps its colours on a
                       white page, reading width / smart dark only for plain mail; image placeholders), theme/skinResponsive.ts
                       (RESPONSIVE_SKIN_CSS: container `nail-fields` on window bodies / settings cards, DIALOG_BUTTON_ROWS =
                       sticky last button rows, GROWING_DIALOGS, INLINE_FIELD_ROWS, two panes stack when narrow),
                       features/windows/geometry.ts shownRect(placedOnPage) (windows placed on the page follow it when the app
                       shrinks), layout/engine.ts flexForSize (fixed pane sizes shrink to their min-sizes), features/pim
                       initialScroll.ts, skin constants CALENDAR_CONTAINER / THEME_EDITOR_STACKED / CUSTOMIZER_*,
                       features/system/QueryProblem.tsx (desktop-only cards say so in the browser); test/fastStyles.ts +
                       test/liveEnvs.ts (WP193);
                       P2 (ADR 0148): widgets/messageListSkin.ts (count pill, participants) + widgets/threadViewSkin.ts (the
                       reader's strip / card feed; `thread-view.*` left defaults/theme.kdl), theme/partAliases.ts (renamed
                       parts: old-name rules also style the new part for one release), template filters `names(n)` /
                       `if-many` + field `{participants}`, state/queries.ts useConversation, mailOps findSummary over thread
                       rows' latest_message, api/mockConversations.ts addMockConversation (Inbox + Sent + a draft), slots
                       features/conversation-list (WP202) / features/conversation-reader (WP203);
                       P2 packages: widgets/messageListModel.ts (rows, children, selection by row key, thread fallback,
                       tree moves), MessageListChild.tsx, messageListControl.ts, template filter `then("text")` + field
                       `latest_from`, `data-fields` per row line (Compact hides preview lines), actions list.expand /
                       list.collapse (ADR 0150); widgets/ThreadViewFeed.tsx + ThreadViewModel.ts + ThreadViewScope.tsx
                       (the card feed: per-card scoped UI store renders the reader's per-message widgets, `card=`),
                       features/conversation-reader (conversation.* actions, readerStack.ts + ReaderOffer.tsx = the
                       one-time "Read conversations as cards" offer, UI-state key `conversation-reader.layout-offer`)
                       (ADR 0151); UiState.messageThreads (j / k continue from a conversation's row; ADR 0153);
                       P3 (ADR 0164): features/mail-actions (conversation actions, undo toast), api/mockMailActions.ts (the
                       contract with undo), theme/frameHeight.ts (mail frames report their height: srcdoc allow-same-origin
                       without scripts, sandbox origin hashed script + postMessage); P3 packages: actions/mailTargets.ts
                       (currentTargets: context menu → picked rows → selection; conversation + listed folder),
                       features/mail-actions run.ts / store.ts / Toast.tsx / move.ts / skin.ts (MAIL_ACTIONS_SKIN_CSS in
                       theme/compile.ts; one ApplyMailAction per action, undo toast + `mail.undo` = z, card More menus;
                       ADR 0167), widgets/DeletePermanently.tsx asks with the count; widgets/MailFrame.tsx
                       (LocalMailFrame / RemoteMailFrame sized by a geometry rule; FrameHeightGuard in theme/frameHeight.ts,
                       frameHeightOf accepts origin "null" from the opaque sandbox origin; ADR 0168);
                       RemoteGate.tsx
                       (login gate), api/reauth.ts, theme/trustedHtml.ts (Trusted Types for srcdoc),
                       i18n/format.ts (the only place that formats dates: useDateFormat / formatDateTime;
                       enforced by format.test.tsx; ADR 0054), App.tsx wraps the layout in
                       usePluginLayout (plug-in commands as actions; ADR 0052),
                       theme/BootTheme.tsx (themed boot / login pages), src/a11y.test.tsx (axe in
                       Vitest); vite.config.ts splits vendor / catalogs chunks, TipTap / CodeMirror
                       load lazily
packages/              npm workspaces (M2d, ADR 0053): plugin-api (@nail/plugin-api: src/api.ts = the host's
                       `nail` global (mirror prelude.js by hand), jsx-runtime, globals.d.ts, testing.ts
                       createTestHost (writes, confirmations, hooks, previews, jobs, MCP tools, l10n),
                       src/extensions.ts hook / preview helpers and checks, src/bytes.ts UTF-8 / base64,
                       src/l10n.ts Fluent for the test host, tools/ checkManifest + packPlugin + cron.mjs +
                       catalogs.mjs + bin nail-plugin, scripts/gen-docs.mjs; src/generated from nail-api),
                       create-nail-plugin (template/ plus templates/{hook,composer,attachment}, --template)
                       (M4a: ADR 0082)
examples/              example plug-ins: follow-up (2.0: creates a task), attachment-check (before-send veto),
                       csv-preview (attachment preview) — not workspaces; tests in
                       packages/create-nail-plugin/test/{examples,example,attachment-check,csv-preview}.test.ts;
                       dist/ is gitignored
e2e/                   Playwright tests (support/desktop.ts: WebView2 CDP smoke test; support/mcp.ts;
                       support/plugins.ts + global-setup.ts build the three examples outside the repo
                       (buildExample / examplePackage);
                       tests/plugins.spec.ts (M2d), tests/a11y-windows.spec.ts (axe on every window, tab and
                       dialog in default / high-contrast / dark, self-checked coverage; ADR 0054),
                       tests/pim.spec.ts (M3a; startNail({launch}) passes `--launch`), tests/scheduling.spec.ts +
                       tests/gmail.spec.ts (M3b), tests/agents.spec.ts + tests/extensions.spec.ts (M4a),
                       tests/desktop-m4b.spec.ts (M4b on the desktop app: MAPI harness, tray / start hidden via
                       shellControl, loopback updater with support/updates.ts, MSI facts via NAIL_REGISTRY_ROOT,
                       nail plugin dev, first run; ADR 0092); M4c: tests/m4c.spec.ts (mbox / Maildir / Thunderbird
                       imports, export + re-import byte-identical, backup → new Nail → restore, palette),
                       tests/vault-recovery.spec.ts (startNail({dirs}) restarts on the same directories, the demo
                       keychain file loses the data key), tests/desktop-m4c.spec.ts (Save as PDF); perf/ (M4c WP123, ADR 0097: budgets.json,
                       list.perf.ts with its own playwright.perf.config.ts (not part of `npm run e2e`), run-perf.mjs
                       = Rust run → Playwright → check; reports in <checkout>/.nail-build/perf/<corpus>));
                       tests/m5a.spec.ts (themes, density, Follow Windows via `system-appearance`, user themes,
                       attachment download); tests/m5b-contract.spec.ts (fallback: view shortcuts open the views' windows
                       with a pre-rail layout, palette); tests/m5b.spec.ts (rail / views / badges / remembered view, More,
                       chips, settings search, calendar menu, dates in a German browser, layout offer, remote dark mode);
                       tests/m5c.spec.ts (window manager scenarios) + tests/desktop-m5c.spec.ts (pop-out composer);
                       tests/m5d.spec.ts (in-app editors: layout.kdl / theme.kdl / themes/*.kdl, undo, conflicts);
                       tests/p2-quickwin.spec.ts (P2 quick win: conversation rows + count, the folded strip; screenshots in
                       <build dir>/screens/p2/quickwin/);
                       support/views.ts composerWindow / composerWindows / discardComposer (M5c);
                       support/views.ts (rail, viewRegion, showView, openSettings, readerAction, runFromPalette);
                       support/responsive.ts (P1, ADR 0139: STANDARD_SIZES 1600/1280/1024/800 × 1000/700,
                       expectResponsive / layoutIssues / resizeWindow — overflow, clipping, text running out of a
                       non-clipping box (`overflowing`, ADR 0147), overlap (incl. a text leaf's sideways spill),
                       off-screen, label alignment of [data-part]; screenshots <build dir>/screens/p1/<name>-<w>x<h>.png;
                       self-test tests/responsive-helper.spec.ts);
                       visual/ (M5a, ADR 0106: visual regression, built-in themes ×
                       densities × 9 screens; own Playwright config; baselines in visual/baselines/<platform>
                       (win32 = reference); review page from scripts/visual-review/)
defaults/              shipped theme.kdl (base + token schema) + layout.kdl (Lucide icon names; M5b: main = nav-rail +
                       view-host, views mail / calendar / contacts / tasks / settings, the mail window, settings window =
                       settings-page, ranked reader bar with More, no main toolbar); themes/ = the built-in
                       themes frost, sage (light), graphite, nocturne (dark), high-contrast (M5a, ADR 0102)
locales/               Fluent catalogs (en, de); core*.ftl = texts built in Rust (M2c; core-plugins.ftl,
                       core-crypto.ftl (pgp-*, message-file-*) M2d); itip.ftl = iMIP mail texts (M3b);
                       M3c: core-exchange.ftl (WP93 + contract ids), core-graph.ftl (WP91), core-ews.ftl
                       (WP92), exchange.ftl (WP94); M4a: core-agents.ftl (contract ids: kinds, states,
                       summaries, fields), core-mcp.ftl (WP101: prompts, tool fields / values, elicitation,
                       audit texts), agents.ftl (WP104), core-plugins.ftl (WP102 host texts, plugin-write-log);
                       M4b: core-desktop.ftl (contract ids: install kinds, update channels / failures / reasons,
                       default apps, mapi-result-*; WP113 shell texts), system.ftl (WP114);
                       M4c: core-portability.ftl (WP121 + contract ids), core-vault.ftl (WP122 + contract ids),
                       portability.ftl / palette.ftl / print.ftl (WP124; palette / print contract ids);
                       M5a: core-appearance.ftl (contract ids: theme names / kinds / sources, densities, issues),
                       appearance.ftl (WP153); M5b: core-shell.ftl (contract ids: views, view actions, toolbar More /
                       group headings, settings groups / default sections, list filters, favorites, folder sections,
                       UI-state issues), shell.ftl (WP161), settings.ftl (WP163); M5c: core-windows.ftl (contract ids: snap
                       halves, instances, window-state / pop-out issues, pop-out changes), windows.ftl (WP171), popout.ftl
                       (WP173); M5d: core-editors.ftl (contract ids: history scopes, edit kinds, edit issues, preset names,
                       config-preset-*), layout-editor.ftl (WP181), toolbar-editor.ftl (WP182), appearance.ftl (WP183
                       theme editor); P2: conversations.ftl (contract: "me", separators, counts / folds, modes),
                       conversation-list.ftl (WP202), conversation-reader.ftl (WP203); P3: core-mail-actions.ftl (contract ids), mail-actions.ftl
                       (WP-actions-ui); every file is embedded in
                       nail-config l10n.rs
scripts/               build-installer.mjs (NSIS + MSI, --arch / --offline-webview / --update-base, NailMapi.dll),
                       sbom.mjs, installer-smoke.ps1, msi-smoke.ps1, update-manifest.mjs, update-server.mjs
                       (loopback), update-check.mjs, updater-key.mjs, winget-manifest.mjs, release-version.mjs,
                       import-signing-cert.ps1 / verify-signatures.ps1, lib/ (minisign, sign, winget, …; M4b
                       ADR 0089), check-socket-binds.mjs (ADR 0057), test/ (node:test);
                       interop/ = separate Cargo workspace (copy the root Cargo.lock in first) (ADR 0040);
                       dev.mjs + tauri.mjs + lib/dev-port.mjs (free dev-server port, Tauri devUrl override);
                       Q1 (ADR 0123): check.mjs + lib/check-steps.mjs (`npm run check`), pre-commit.mjs +
                       lib/staged.mjs (.githooks/pre-commit), install-hooks.mjs (`prepare`: core.hooksPath),
                       check-editorconfig.mjs + lib/editorconfig-check.mjs;
                       work-folders.mjs + lib/work-folder.mjs (ADR 0132: create a work folder, guard the master;
                       assertWorkFolder refuses development commands inside OneDrive); post-edit.mjs (the Claude Code
                       post-edit hook of .claude/settings.json) + lib/run-checks.mjs (per-file checks, shared with pre-commit.mjs)
Quality tooling (Q1, ADR 0123): eslint.config.base.js (shared flat config: typescript-eslint strictTypeChecked +
                       stylisticTypeChecked, type-aware; react() = React / Hooks / jsx-a11y / Nail UI rules),
                       eslint.config.js (all but ui/), ui/eslint.config.js, tsconfig.json (root: types for the loose
                       Node scripts + prelude.js), clippy.toml, [workspace.lints] in Cargo.toml, taplo.toml,
                       .markdownlint-cli2.jsonc, typos.toml, .editorconfig, requirements-dev.txt (typos,
                       actionlint), .githooks/
```

## Commands

Full details in `docs/dev-setup.md`; keep both in sync.

```text
npm ci                                     # npm workspaces: ui, e2e, packages/*
npm run build                              # ui/dist — required before any `cargo … --workspace` (Tauri embeds it)
cargo test                                 # default members = all crates except app/src-tauri
cargo test --workspace                     # incl. the Tauri shell (after npm run build)
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
npm test                                   # socket-bind check (ADR 0057), Vitest (ui) incl. the lint-rule self-test, packages/* `test`, perf + visual script tests
cd ui && npx vitest run --silent=false --reporter=default   # Vitest with console output even when piped (expect no `stderr |` block)
npm run check:sockets                      # only the socket-bind check (scripts/lib/socket-binds.mjs: rules + allowlist)
npm run check                              # everything, 0 warnings (ADR 0123): rustfmt, taplo, Prettier, EditorConfig, typos, markdownlint, actionlint, ESLint, tsc, clippy + rustdoc -D warnings, cargo machete, cargo deny
node scripts/check.mjs --only eslint,tsc   # a subset (--skip, --list, --advisories = cargo deny advisories, on in CI; --fail-fast)
npm run lint                               # ESLint (whole repo, type-aware, --max-warnings 0) + tsc (every tsconfig) + Prettier (whole repo)
npm run lint:md / lint:toml / lint:editorconfig   # markdownlint, taplo fmt --check + taplo check, EditorConfig
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps   # rustdoc lints
cargo machete                              # unused dependencies; typos / actionlint: pip install -r requirements-dev.txt
taplo fmt                                  # format TOML (cargo install taplo-cli --locked)
npm run e2e                                # builds ui + nail-cli + desktop app, then Playwright (docs/dev-setup.md)
npm run e2e:only                           # Playwright against what is already built
npm run build:desktop                      # nail-app (custom-protocol + test-overrides) + DLL / harness for the desktop smoke tests, own target dir
npm run gen:api                            # regenerate ui/src/api/generated + packages/plugin-api/src/generated (a test fails when stale)
npm run docs -w packages/plugin-api        # regenerate the reference sections of userDocs/en/plugins/{api-reference,components}.md (a test fails when stale)
node packages/create-nail-plugin/index.mjs <dir> [--template command|hook|composer|attachment] [--id] [--name] [--sdk]   # scaffold a plug-in
npm run package --prefix examples/<name>   # build + pack an example (follow-up, attachment-check, csv-preview; dist/ gitignored)
nail plugin dev <folder> [--url URL | --data-dir DIR]   # load a plug-in folder into the running `nail serve`, hot reload, log console
node --test "scripts/test/*.test.mjs"      # installer / release script tests
cargo test -p nail-mapi --target i686-pc-windows-msvc   # the x86 Simple MAPI DLL under WOW64 (rustup target add i686-pc-windows-msvc)
cargo deny check                           # licences, bans (openssl, aws-lc), sources, advisories (deny.toml)
cargo run -p nail-bench --release -- --corpus small   # performance benchmark against e2e/perf/budgets.json (M4c WP123; --tolerant, --no-check, --in-process, --work-dir DIR)
nail-bench check --report A --report B [--tolerant] [--out F]   # merge Rust + UI reports, check the budgets (CI tolerance 1.5)
node e2e/perf/run-perf.mjs [--corpus standard|small] [--tolerant]   # whole perf job: needs `cargo build --release -p nail-cli -p nail-bench` + `npm run build`
npx playwright test -c e2e/perf/playwright.perf.config.ts   # only the UI perf scenario (NAIL_PERF_CORPUS / _WORK / _TOLERANT / _FRESH, NAIL_BIN, NAIL_BENCH_BIN)
npm run visual                             # visual regression (M5a, ADR 0106): built-in themes × densities × 24 screens (e2e/visual/screens.ts; some at 1024 px) vs e2e/visual/baselines/<platform>; needs ui build + nail-cli
npm run visual:update                      # also write baselines of new / changed screens; then review <build dir>/visual/review/index.html and commit
npm run visual:review                      # rebuild the review page; npm run visual:selftest = harness self-test
node scripts/build-installer.mjs [--skip-ui] [--arch x64|arm64] [--bundles nsis,msi] [--offline-webview] [--dev-updater-key]   # installers, portable zip, SBOMs → build dir (never the repo)
powershell -File scripts/installer-smoke.ps1   # install to a temp dir, start, [update,] uninstall
powershell -File scripts/msi-smoke.ps1         # elevated (CI): silent MSI install with properties, start, upgrade, uninstall
node scripts/update-server.mjs --root DIR      # loopback update server; update-manifest.mjs / winget-manifest.mjs write manifests
node scripts/sbom.mjs                      # CycloneDX SBOMs
NAIL_UPDATE_SNAPSHOTS=1 cargo test -p nail-config --test schema   # regenerate config JSON Schemas (theme, layout, accounts)
NAIL_UPDATE_SNAPSHOTS=1 cargo test -p nail-config --test rules_schema   # rules.kdl JSON Schema
NAIL_UPDATE_SNAPSHOTS=1 cargo test -p nail-config --test defaults  # nail-api fixtures after editing defaults/*.kdl (then prettier)
NAIL_UPDATE_SNAPSHOTS=1 cargo test -p nail-api --test fixtures     # crates/nail-api/fixtures/icons.json after editing nail_api::icons
bash crates/nail-testkit/corpus/crypto/pgp/generate.sh     # OpenPGP interop vectors (GnuPG 2.4 = Git for Windows /usr/bin/gpg)
bash crates/nail-testkit/corpus/crypto/smime/generate.sh   # S/MIME test CA + vectors (OpenSSL 3.4+ = /mingw64/bin/openssl)
cargo run -p nail-cli -- serve --seed demo --open                 # web mode with demo data (127.0.0.1:4780, else a free port; explicit --port N must be free)
cargo run -p nail-cli -- serve --seed demo-imap                   # demo corpus synced from an in-process mock IMAP server
cargo run -p nail-cli -- serve --seed demo-multi                  # IMAP + POP3 + SMTP mock accounts, mock CalDAV / CardDAV (+ principal bob) + ICS feeds, a mock Gmail account, mock Exchange Online (Graph) + Exchange Server (EWS) accounts (all three switched off) and mock Autodiscover (test controls at /api/test/…)
cargo run -p nail-cli -- serve --seed demo --launch C:/path/x.ics  # hand a file / link to the UI like the desktop shell (M3a)
cargo run -p nail-app --features tauri/custom-protocol -- --seed demo   # desktop app on the built ui/dist (temp dirs)
NAIL_MCP_TOKEN=<token> nail mcp --client <name> [--url URL] [--data-dir DIR]   # stdio bridge to the running app's MCP server
nail mcp --client <name> [--config-dir DIR] [--mcp-port N] [--no-start]   # starts a headless Nail when none runs
npm run dev                                # Vite dev server on a free port (NAIL_DEV_PORT, else 5791, else any; scripts/dev.mjs)
npm run tauri dev                          # desktop app with Vite hot reload (same port choice + matching devUrl; scripts/tauri.mjs)
```

- **Vitest hides console output when stdout is not a terminal** (piped / agent runs), so warnings never show there.
  `ui/src/test/setup.ts` therefore fails every test that logs `console.error` / `console.warn` (React's
  "not wrapped in act(...)", `nail: missing message …`, `nail: formatting …`, a failed action) unless the test
  declares it with `expectConsole(/…/)` / `allowConsole(/…/)` from `ui/src/test/console.ts`. Fix the cause
  instead: await the state the UI settles into (`findBy*`, `waitFor`, `settleQueries(env)` from the harness),
  wrap direct store updates / events / waits in `act`, unmount (`cleanup()`) before resetting module-level
  stores in `afterEach`, mock what the test triggers, give fixture ids catalog entries; axe checks use
  `ui/src/test/axe.ts` (runs inside `act`, violations only). `NAIL_CONSOLE_STACK=1` adds the call stack to each reported line.
  P1 (ADR 0143): `setup.ts` installs `ui/src/test/fastStyles.ts` (a `data-part` pre-filter for jsdom's
  `getComputedStyle`; if `fastStyles.test.tsx` fails after a jsdom upgrade, adapt the filter) and, before unmounting,
  waits up to 3 s until every `createEnv` env is idle (`ui/src/test/liveEnvs.ts`). Prefer
  `userEvent.setup({ delay: null })` in widget tests; never raise a timeout to hide load flakiness.
- `ui/src/i18n/coverage.test.ts` types every template-string message id (``t(`x-${v}`)``) with the TypeScript
  checker: a literal-union `${v}` must have a message for every value; a plain-`string` `${v}` must be listed in
  its `OPEN_TEMPLATES` (values covered by a feature's `runtimeIds()`). Key contexts: every `data-key-context`,
  every `keys context=` of `defaults/layout.kdl` and the plug-in manifest's `KEY_CONTEXTS` must be in
  `ui/src/features/customization/editorModel.ts` `KEY_CONTEXTS` with a `customize-keys-context-*` label.
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
- M2d integration (ADR 0055; packages ADRs 0051 host, 0052 UI, 0053 SDK, 0054 M2c carry-overs): the
  core owns one PluginHost (created on first use in the runtime; enabled plug-ins start in
  `Service::start`, packages verified by hash); `PluginServices` holds the service weakly and never
  blocks a plug-in thread; plug-in bodies are never decrypted; `nail.http.fetch` runs through
  `plugin_http.rs` (nail-proxy rules); placements go through `nail_config::placement_layout` and the
  merge dialog; `RunPluginCommand` / `PluginUiAction` answer `Done` at once. Core tests of plug-ins
  need `#[tokio::test(flavor = "multi_thread")]` (plug-in threads call back into the runtime).
- M2d review fixes (ADR 0056): plug-in searches are header-only outside `mail.read.body`
  (`PluginServices::mail_search` gets a `body_scope`) and drop encrypted mail while the index holds
  decrypted text; a stopped plug-in is interrupted at once and its API refuses all but logging;
  every log line a plug-in causes counts per call and per minute (`max_log_lines_per_minute`), the
  core's log queue is bounded. `nail-pgp` texts Nail writes itself are `Error::Problem`
  (`pgp-problem-*`); `detail` arguments stay library texts only.
- M2d contract: ADR 0050 (`nail_api::plugins`: manifest, permissions `mail.read`, `mail.read.body`,
  `calendar.read` (placeholder), `ui.dialogs`, `notifications`, `storage`, `network:<host>`; component
  trees; plug-in admin / runtime requests and events; plug-in-facing JSON is camelCase, protocol
  snake_case; store v9; stub crate `nail-plugins` with rquickjs 0.14 + zip 8.6 (deflate via zlib-rs);
  slots `nail-cli/src/plugin.rs`, `nail-config/src/{plugins.rs,widgets/plugins.rs}`,
  `ui/src/features/plugins/`, `locales/*/{plugins,core-plugins}.ftl`, `packages/*`, `examples/`,
  `userDocs/en/plugins/`). Remote sessions use plug-ins only with a grant for every account; plug-in
  administration is `admin`, dev loading desktop only. Host requests answer `unsupported` until WP64.
- M3a contract: ADR 0058 (`nail_api::pim`: series + overrides, `EditScope` this / this-and-following
  / all, `PimTime` date / floating / zoned, contacts with merge links, tasks, reminders; 52 requests,
  9 events, `NotificationKind::Reminder`; sources in `accounts.kdl` as top-level
  `calendar-subscription` / `caldav` / `carddav` nodes (DAV may name `account=` to share its login;
  built-in `local` source); store v10 (PIM tables, instance cache, `pim_ops` queue, sealed data);
  stub crates `nail-ical` (calcard 0.3, chrono-tz) / `nail-dav`; slots `nail-testkit-net/src/dav.rs`,
  `nail-store/src/pim.rs`, `nail-search/src/pim.rs`, `nail-triage/src/reminders.rs`,
  `nail-config/src/widgets/pim.rs`, `ui/src/features/pim/`, `locales/*/{pim,core-pim,core-ical,core-dav}.ftl`).
  Packages: ADRs 0059 (`nail-ical`), 0060 (`nail-dav`, mock DAV, Radicale), 0061 (PIM store, search,
  reminders), 0062 (PIM UI).
- M3a integration (ADR 0063): `nail-core/src/pim/` serves every PIM request; items are stored as the contract
  type plus the raw text `nail-ical` writes by patching the stored original; writes queue in the PIM queue
  (never for the local source) and wake the source's engine; engines poll ICS (conditional GET) and
  CalDAV / CardDAV (discovery stored on the source row, sync-collection, replay before sync, 412 = server wins
  with a conflict copy) with backoff; reminders run on the triage clock (`ServiceOptions::clock`), notify
  through `NotificationRequested` (quiet hours mute the sound); the PIM search index is in memory; MCP PIM
  resources / tools need `read` and the grant's account per source (`all_accounts` for local / subscriptions);
  remote PIM still needs a grant for every account. Core tests: `tests/pim.rs`, `tests/pim_sync.rs` (mock DAV /
  HTTP servers, `AdjustableClock`). demo-multi adds `work-calendars` / `work-contacts` (mock DAV, password
  `dav-secret`) and the `holidays` feed, controls `dav-objects` / `dav-put` / `dav-delete` / `feeds`.
- M3a review fixes (ADR 0064): untrusted calendar data is bounded — `nail-ical` nests at most 32 levels, recurrence
  jumps ahead (`skip_before`), `MINUTELY` / `SECONDLY` rules stop after 2,000 occurrences, expansions share an
  `expand::ScanBudget` per request (core `Expander::default()`), the instance cache expands outside the store lock;
  subscriptions are fetched only through `nail-core/src/pim/feed_http.rs` (SSRF-safe; plain loopback `http:` only
  with `ServiceOptions::loopback_feeds`, which `demo::demo_options` sets); `DavClient` sends logins only to the
  requested origin (`DavError::RedirectNeedsLogin`); reminders are capped per item (`limit_plan`) and summarized
  when more than three fire at once.
- M3b contract: ADR 0065 (`nail_api::scheduling`: iTIP methods, the invitation card on
  `MessageBody.invitation`, `InvitationTarget` message / event, 12 requests incl. free/busy and
  settings; `nail_api::gmail`: `gmail` account node (oauth2, no smtp), messages stored once with
  the folder from their labels, user labels = tags, categories on `MessageSummary.gmail`, 4 label
  requests through a label queue; `MailApiEngine` / `AdaptivePoll` / `FreeBusyProvider`; 3 events;
  store v11 with APIs; stub crates `nail-itip` / `nail-proto-gmail` (mock server: feature
  `testing`); slots `nail-dav/src/scheduling.rs`, `nail-store/src/pim/freebusy.rs`,
  `nail-config/src/widgets/{scheduling,gmail}.rs`, `ui/src/features/{scheduling,gmail}/`,
  `locales/*/{scheduling,gmail,core-scheduling,core-gmail}.ftl`). The core answers every M3b
  request `unsupported` until WP85; a Gmail account's engine reports `unsupported`.
- M3c contract: ADR 0072 (`nail_api::exchange`: `exchange` account node with engine Graph (Exchange
  Online, OAuth2) / EWS (on-premises: `auth "windows"` = SSPI Negotiate / Kerberos / NTLM, Basic over TLS,
  OAuth2 hybrid); IMAP fallback = a normal IMAP account; shared / delegate mailboxes as store data
  (automapped ones hide on removal) and `@<mailbox>/…` folder trees; categories = tags (case-insensitive) with
  Outlook colours; Focused Inbox `focus:`; inbox rules with `RulePlacement`; automatic replies as plain text;
  rooms / room lists; To Do lists as task lists of the account's `exchange:<account>` PIM source; 18 requests,
  1 event `ExchangeChanged {areas}`; engine seams `ExchangeEngine` / `NegotiateAuth` and provider-neutral PIM
  shapes in `nail_api::exchange::{engine,pim}`; store v12 with APIs; stub crates `nail-proto-graph`,
  `nail-proto-ews` (mocks: feature `testing`), `nail-exchange`; slots `ui/src/features/exchange/`,
  `nail-config/src/widgets/exchange.rs`, `locales/*/{exchange,core-exchange,core-graph,core-ews}.ftl`). Served
  since WP95 (ADR 0077, below).
  Microsoft tokens are per resource: Graph scopes (`nail_auth::mail_scopes(microsoft, [Graph])`) and Outlook
  IMAP / EWS scopes are separate sign-ins.
- M3b integration (ADR 0070; packages ADRs 0066 `nail-itip`, 0067 `nail-proto-gmail`, 0068 CalDAV scheduling,
  0069 UI): iTIP mail is recorded and applied on arrival per `SchedulingSettings` (never junk or spoofed);
  answers and organizer mail leave through the outbox as `AutomaticMail::Itip` (prepared MIME as the draft's
  only attachment; undo send) unless the CalDAV server schedules (`calendar-auto-schedule` stored with the DAV
  discovery) — then only the event is written. Gmail accounts run `gmail::run` (no SMTP; `transport()` returns
  the engine; no Sent copy). demo-multi's Gmail account starts `enabled=#false` (the other e2e suites count the
  unified inbox); `scheduling.spec.ts` / `gmail.spec.ts` turn it on or deliver iMIP mail with `imap-deliver`.
  Clippy `large_futures`: boxed futures in `Service::dispatch` (scheduling).
- M3b review fixes (ADR 0071): arrival processing applies nothing whose card has an issue that
  `ItipIssue::blocks_automatic` (sender checks, `organizer-changed`, `sent-on-behalf`, `uninvited-attendee`,
  `not-addressed-to-me`), never `PUBLISH`; remote scheduling requests that mail need `send`; arrival rules /
  iTIP only for Inbox / Other folders (`sync::runs_arrival_rules`, IMAP and Gmail); meetings written to a
  CalDAV source with unknown scheduling get `SCHEDULE-AGENT=CLIENT` (`pim::engine::scheduling_unknown`).
- M3c integration (ADR 0077; packages ADRs 0073 Graph, 0074 EWS, 0075 common layer, 0076 UI): Exchange accounts
  run `exchange::sync::run` (one engine `Arc<dyn ExchangeEngine>` per account, also the outbox transport via trait
  upcasting; the server files the Sent copy); messages keep an id derived from their first server id, folders one
  from (mailbox, remote id); other mailboxes hang under `@<address>`; Exchange calendars are server-scheduled
  (answers via `respond_to_meeting`, never iMIP); rule edits and account changes re-place rules (`queue_sieve` →
  `settings::queue_rules`). `ServiceOptions::exchange` (`ExchangeHooks`): Autodiscover HTTP / DNS, the Microsoft
  lookup switch, an injected `NegotiateAuth`, a Graph endpoint for demos. `DiscoverAccount` carries
  `trusted_hosts` / `windows_sign_in` and answers `exchange` (prompts, automapped mailboxes); `StartOAuth` carries
  `protocol` (per-service scopes). demo-multi's Exchange accounts `exchange` (Graph) and `corp` (EWS) start
  `enabled=#false`; `exchange.spec.ts` switches one on or adds accounts through Autodiscover.
- M4a contract: ADR 0079 (`nail_api::agents`: action kinds, policies — destructive kinds (Trash, permanent
  delete, PIM deletes) always ask, outgoing + rules ask by default and may be always-allowed per client / plug-in,
  drafts / flags / tags / PIM writes run without asking by default; `skip_draft_confirmation` is ignored —
  confirmations with attribution, summary, before / after; elicitation approves only non-destructive kinds; MCP
  tool table + write-tool args (`json-schema` feature) + prompts + `McpRateLimits`; plug-in permissions
  `mail.organize` … `mcp.expose`, hooks (budgets, veto → `SendVetoed` / `OverrideSendVeto`, banners via
  `GetMessagePluginAnnotations`), composer / attachment / link / MCP extensions (`RunPluginAction`,
  `RenderAttachmentPreview`, `ComposerEditRequested`), background jobs (`CronSchedule`, CPU quota),
  `nail.l10n`, binary fetch; 11 requests, 5 events; store v14; slots: `ui/src/features/agents/`,
  `nail-config/src/widgets/agents.rs`, `locales/*/{agents,core-agents,core-mcp}.ftl`; WP101–WP104 own
  `nail-mcp`+`nail-cli` / `nail-plugins` / `packages`+`examples`+plug-in & MCP user docs / UI).
- M4a integration (ADR 0084; packages ADRs 0080 MCP writes, 0081 plug-in host, 0082 SDK, 0083 UI): agent and
  plug-in writes share one path (`nail_mcp::ActorWrites` = the MCP write plans → policy → confirmation → Client API
  requests through the op queue); the core owns one `StoreBroker` (`Service::confirmation_broker`, woken by
  `DecideConfirmation`); revoke / uninstall cancel an actor's confirmations and policies; arrival hooks run after the
  user's rules (actions not awaited), before-send hooks in `SendDraft` (veto map in memory 10 min, `OverrideSendVeto`,
  content into the draft, `X-` headers on the built message), open hooks for `GetMessagePluginAnnotations`; the
  `clock-set` / `clock-advance` controls run due plug-in jobs (`Service::clock_moved`); plug-in tools appear in MCP
  `tools/list` for clients with their scopes. Core tests: `tests/m4a_wired.rs` (demo-multi, `multi_thread`).
- Post-M4a (ADR 0086): `PluginCatalogs::localize_manifest` resolves every `%id%` text of every contribution kind
  (a new contribution text must be added there, with a test); confirmed sends pin the draft's content-hash
  revision (`nail_store::draft_revision`, `SendDraft.expected_revision`), checked in the store transaction of each
  draft write on the send path (`Store::save_draft_at`, `Store::schedule_send_at`; mismatch = `DraftChanged` →
  `conflict` / `mcp-tool-error-draft-changed`).
- M4b contract: ADR 0087 (`nail_api::desktop`: installation facts from the installers' registry values,
  updater settings / state machine with the policy winning (MSI-managed / group policy / portable / dev),
  tray / startup settings (Run key wins), default apps + Settings URI, first run, the desktop `nail plugin
  dev` endpoint (LoadDevPlugin only, bearer token in `ui-endpoint.json`), `DesktopHost` trait passed as
  `ServiceOptions::desktop`; 12 desktop-only requests, 3 events; `nail_api::mapi`: Simple MAPI messages,
  MAPI.h result codes, named pipe `\\.\pipe\nail-mapi-<SID>` with length-prefixed JSON frames,
  `Service::submit_mapi` → draft + `LaunchRequest::ComposeDraft`, never sent for a program; slots:
  `crates/nail-mapi` (WP111), `app/src-tauri/windows/` + `tests/installer.rs` + `packaging/` + `scripts/` +
  `.github/workflows/` (WP112), rest of `app/src-tauri` (WP113), `ui/src/features/system/` + `system.ftl`
  (WP114); new deps tauri-plugin-updater (rustls only), tauri tray-icon / image-png, windows-sys,
  windows-registry).
- M4b integration (ADR 0092; packages ADRs 0088 MAPI DLL, 0089 installers / updates, 0090 shell, 0091 UI): a
  program's `MAPISendMail(W)` → `NailMapi.dll` (private copies of the files) → the shell's pipe → `submit_mapi` →
  draft + `ComposeDraft` → composer with the program's note; never sent. The pipe server must not disconnect
  before the client closed its end (`DisconnectNamedPipe` drops unread answers). UI update commands refetch
  `GetUpdateStatus` instead of writing their answer (events may be newer). Seeded desktop runs never touch the real
  registry / Run key / pipe and register `nail_test_shell` (tray item ids, `window-visible`) for the smoke tests;
  shell test variables: `NAIL_REGISTRY_ROOT`, `NAIL_UPDATE_ENDPOINT` + `NAIL_UPDATE_PUBKEY` (loopback only),
  `NAIL_MAPI_PIPE`, `NAIL_TEST_BACKGROUND_WINDOWS` (docs/dev-setup.md) — honoured only by builds with the
  `test-overrides` features (`npm run build:desktop`), never by installer builds. The real MSI install runs only in
  CI (`msi-smoke.ps1`, elevated).
- M4b review (ADR 0093): no machine-wide registration may name a user-writable file (the NSIS MAPI option copies
  the DLLs to `%ProgramFiles%\Nail MAPI` in one elevated `cmd.exe` and registers the copy); nothing elevated reads
  from `%TEMP%`; secrets go to the one step that needs them (the update key reaches only `tauri bundle`);
  release / signing / update-key procedures live in `docs/release.md`.
- M4c integration (ADR 0099; packages ADRs 0095 data protection + smartcards, 0096 portability, 0097
  performance + leftovers, 0098 UI): imports into an account use its existing folders only
  (`folder-merged`; new folders only in Local Folders); restores replace config files through
  `ConfigManager::replace_file` (configuration backups) and merge the rest; a locked data directory
  (encrypted database, key gone) runs on an in-memory stand-in with engines off and opens after a restart;
  smartcard keys sign / decrypt wherever software keys do (`SmartcardCtl` is both keyrings' resolver; PIN
  prompts over events, card work on blocking threads); PDF files through a hidden print window; seeded runs
  keep secrets in `<data-dir>/demo-keychain.json`. Version `1.0.0-rc.1`; `CHANGELOG.md`,
  `docs/release-checklist.md` (human gates).
- M4c contract: ADR 0094 (`nail_api::portability` — import mbox / EML / Maildir / Thunderbird / ICS / vCard,
  export, jobs with `PortabilityJobProgress` / `PortabilityJobFinished` and per-item reports, mail into "Local
  Folders" or through the account's queue, Thunderbird accounts / filters only proposed; `backup` — encrypted
  `.nailbackup`, verify, dry-run `PreviewRestore`; `recovery` — the vault recovery key (`VaultStatus.needs_recovery`);
  `smartcard` — PKCS#11 + `PinRequested` / `AnswerPinPrompt` (desktop only, PINs never stored); `palette` — recent
  actions (served); `print` — print jobs + `#/print/<id>` view with `print.*` theme parts, PDF via
  `DesktopHost::print_to_pdf` (served); `diagnostics` — exact manifest, no mail by default; `PickPath`;
  `Identity.send_as`, `send_with_progress` / `SendProgress`, store v15 server threads). The rest answers
  `unsupported` (`m4c-not-ready`) until WP125. Slots: WP121 `nail-portability`, WP122 vault / store / pgp / smime /
  `nail-pkcs11` + `[patch.crates-io]`, WP123 Graph / Gmail / EWS / exchange + `nail-bench` + `e2e/perf/`, WP124
  `ui/src/features/{portability,palette,print}` + security additions.
- M5a contract: ADR 0102 (`nail_api::appearance`: built-in themes `defaults/themes/<id>.kdl` + user themes
  `<config-dir>/themes/<id>.kdl`; the selection lives in the user's `theme.kdl`: `extends` = fixed theme,
  `appearance follow-system= light= dark= density=`, overrides = tokens `font-size "body"`, `radius "md"`,
  `color "accent"`; `active_theme` rule (Follow Windows → high contrast / dark / light of the pair); density =
  token variant `compact=` → `modes["compact"]` under `:root[data-nail-density="compact"]` + `app[density=…]`
  part condition; token schema `token_schema()` checked by nail-config `tests/defaults.rs`; icons = Lucide names
  (`nail_api::icons`, legacy names mapped); `ListThemes` / `SetTheme` / `SetThemePreferences` → `Themes`,
  `ThemeChanged`; `SystemAppearanceSource` in `ServiceOptions::system_appearance`). The core answered
  `unsupported` until the integration (served since WP155, below). Slots: WP151 `defaults/themes/` + `defaults/theme.kdl` +
  `nail-config/tests/theme_contrast.rs`; WP152 rest of `nail-config` + `nail-core/src/appearance.rs`; WP153 `ui/` +
  `locales/`; WP154 `e2e/visual/` + `scripts/visual-review/`.
- M5a integration (ADR 0107; packages ADRs 0103 themes + contrast test, 0104 selection / user themes / Follow
  Windows, 0105 UI skin / Lucide / mail panes / appearance page, 0106 visual regression): no theme.kdl = Follow
  Windows with Frost / Graphite (`nail_config::default_selected_theme()`); an accent override sends `ThemeChanged`
  before `ConfigChanged`. Seeded runs see a light OS (`demo_options`); never rely on the machine's mode in tests.
  `defaults/theme.kdl` has no rules for the parts `ui/src/theme/skin.ts` styles and no `icons` (Lucide) — a new
  default part rule's colours must be in the pair table of `tests/theme_contrast.rs`; input-like parts use
  `border "$border.control"`. `ReportSystemAppearance` is ignored while the core watches the OS.
  `GetMessageAttachment` (base64, ≤ 64 MiB) / `SaveAttachment` (desktop only, absolute path).
- M5b contract: ADR 0109 (views = layout windows shown in `view-host`, `nav-rail` from `rail`, badges;
  `view.show "<view>"` + aliases `view.mail` … (`ctrl+1` … `ctrl+4`, `g m`), opening the view's window while the main
  window has no `view-host`; toolbar overflow (`rank`, `display`, `more {}`, `hide-disabled`, `overflow`), menu
  `header`; folder-pane sections (Favorites, accounts with avatar-palette colour slots, smart folders only when present,
  collapsed in UI state); `list-header` + ListFilter; the settings page (`settings-page` section, groups General /
  Look & feel / Security / Extensions, already holding every settings tab's widget; search index from
  `Feature.settings`); every registered action has an icon (layout `actions` or `ActionInfo.icon`); UI state in
  `<config-dir>/ui-state.json` (GetUiState / SetUiState / UiStateChanged, served); remote sessions follow their own
  browser's light / dark mode (served)). The default layout has the views / rail / page sections but unchanged windows
  until WP165. Slots: WP161 `features/shell/` + shell.ftl, WP162 `widgets/` + `theme/skin.ts` + app.ftl, WP163
  `features/settings/` + the `settings` export of every feature but pim + settings.ftl, WP164 `features/pim/` + pim.ftl.
- M5b integration (ADR 0114; packages ADRs 0110 shell, 0111 mail view, 0112 settings page, 0113 calendar view): the default
  main window is `nav-rail` + `view-host` (plus config errors / status bar); the M5a panes live in `window "mail"`
  (grouped folder-tree, list-header first in the list pane); no main toolbar (palette, folder pane, rail instead);
  settings window = `settings-page` (+ section `calendar` with `pim-settings`); `pim.*.open` / `settings.open` alias
  `view.show`. E2E: views are regions "`<View>` view", Settings sections via the page nav (`e2e/support/views.ts`),
  window 1280 × 1000, the last view survives reloads (switch to Mail first). Split `auto` = content width.
  Pre-rail user layouts keep their main window (views open as windows) and get the new default offered once through
  the merge dialog. `ThemeInfo.preview` (a theme's own colours for the gallery); the UI negotiates browser languages
  only against non-empty catalogs (an empty `locales/de` keeps dates English).
- M5c contract: ADR 0116 (window ids `<kind>` / `<kind>:<instance>` in `UiState.windows`, z-order topmost last,
  `focusWindow`; WindowSpec resolved as defaults → `Feature.windows` (`{ confirmation: true }` = the only modal windows)
  → `layout.kdl` `window` properties `size` / `min-size` / `modal` / `resizable` / `pop-out` / `instances`; remembered
  state per kind in `<config-dir>/window-state.json` (versioned; GetWindowStates / SetWindowState / ResetWindowStates /
  WindowStateChanged, served); desktop pop-outs (PopOutWindow / ReturnPopOut / ClosePopOut / FocusPopOut / ListPopOuts /
  PopOutChanged through `DesktopHost::open_pop_out` …, served by the core; the shell's trait methods are WP173's; a
  pop-out webview loads `#/window/<id>` and reads its `context` from ListPopOuts); `UiState.composers` for further
  composers). The default layout sizes every secondary window, `composer` has `instances="multiple"`. Slots: WP171
  `ui/src/layout/` + `features/windows/` + windows.ftl, WP172 `features/composer/` + `windows` registrations in other
  features, WP173 `app/src-tauri/` + `features/popout/` + popout.ftl, WP175 `nail-core/tests/{gmail,exchange}.rs` + the
  Gmail / Exchange engine sources.
- M5c integration (ADR 0121; packages ADRs 0117 window manager, 0118 non-modal windows + several composers, 0119
  desktop pop-outs, 0120 Gmail / Exchange sync races): every dialog is a window of the window manager (also plug-in
  dialogs, the send veto, remote re-authentication, "Delete permanently?"); only confirmations are modal and they block
  only their owner (`modalOwner`: same instance key, else the window focused when it opened, else the main window —
  page-modal = `data-owner="main"`, scrim below every window, the only `aria-modal`); modal windows stack above the
  others. Composers are `composer:<draft|wN>` windows titled by their subject; `UiState.composer` mirrors the focused
  one. Skins `WINDOWS_SKIN_CSS` / `COMPOSER_SKIN_CSS` follow the shared skin; window sizes come only from specs
  (`layout.kdl` `size=` / `min-size=`, `Feature.windows`), never from theme rules. API messages are stored with their
  remote id / Exchange / Gmail data in one transaction (`Store::merge_with_remote`, `ingest_with_remote`).
- Q1 integration (ADR 0129; packages ADRs 0124 q-rust-a, 0125 q-rust-b, 0126 q-rust-c, 0127 q-ts-rest, 0128 q-ts-ui):
  `npm run check` is green with 0 warnings and blocks in CI (`quality`); `nail serve` without remote login answers
  `POST /api/auth` with `204` (`nail_api::remote::NO_LOGIN_STATUS`); `SearchIndex::open_with` / `PimIndex::open_with`
  borrow the wrapper; the Vite build splits the initial code into `vendor`, `vendor-ui`, `catalogs`, `default-theme`
  and size-capped `app` chunks with `strictExecutionOrder` (the mock core loads only with `?mock=1`); rolldown's load-dependent plug-in timing
  notice is off (`checks.bundlerTimings`). Vite configs that import a symlinked workspace package keep
  `resolve.preserveSymlinks: true` (`ui/`, `packages/create-nail-plugin`; needed for checkouts inside OneDrive).
- M5d contract: ADR 0131 (layout edits = new `LayoutPatch` kinds through `UpdateLayout` (+ `base_revision`); `hidden=`
  on layout nodes; `presets` section with four default mail presets; actions `layout.customize` / `toolbar.customize`,
  `menu "context:toolbar"`; theme edits `EditTheme` / `DuplicateTheme` / `RenameTheme` / `DeleteTheme` /
  `GetThemeDetails` on user themes and the user's `theme.kdl`; undo history per scope `layout` / `appearance` in
  nail-config (every write of those files, 50 entries, in memory), `GetEditHistory` / `UndoEdit` / `RedoEdit`,
  `EditHistoryChanged`; conflicts = the file's revision changed on disk → `conflict` / `edit-issue-conflict`, undo
  `force` backs up first). The new patches and theme operations answer `unsupported` / `edit-issue-not-ready` until
  WP184. Slots: WP181 `features/layout-editor/` + `layout/LayoutRenderer.tsx` / `LayoutChrome.ts` + the settings-page
  placement in `defaults/layout.kdl`; WP182 `features/toolbar-editor/` + `widgets/Toolbar.tsx` / `toolbarModel.ts`;
  WP183 `features/appearance/` + the inspector's "Edit style"; WP184 `crates/nail-config/`.
- M5d integration (ADR 0137; packages ADRs 0133 layout editor, 0134 toolbar customizer, 0135 appearance + theme editor,
  0136 write-back): every patch and theme operation is served (no `edit-issue-not-ready`). Mounts: the View menu =
  right-click on the navigation rail (`menu "context:nav-rail"`, context part `nav-rail`), the command palette
  (`layout.customize`, `toolbar.customize`, `theme.edit`, `theme.new`), Settings → Layout & toolbars
  (`layout-editor-settings`, `toolbar-editor-settings`) and → Appearance, right-click on a toolbar. Theme ids from
  names: NFKD without combining marks, `theme-` before a non-letter start, ≤ 60 chars — core and UI alike. A first
  `UpdateLayout` creates `layout.kdl`, so undoing it removes the file. E2E: `m5d.spec.ts`; the a11y window sweep
  covers the editors; visual screens `layout-editor`, `toolbar-customizer`, `appearance`, `theme-editor` (22 then; P2 adds `conversation`, `conversation-expanded`: 24).
- P1 contract (ADR 0139): HTML mail broke because nail-mime's sanitizer drops `<style>` / `<head>` / `class` / body
  attributes and nobody resolves `cid:` (the srcdoc frame also inherits the page CSP, which has no `cid:`); the
  responsive standard (8 viewport sizes, windows at min-size and resized; no clipped text, overlaps, page scroll) is
  checked with `e2e/support/responsive.ts` plus a screenshot review; shared dialog / form rules go into
  `ui/src/theme/skinResponsive.ts` (after every feature skin, before the theme's part rules).
- P1 integration (ADR 0144; packages ADRs 0140 HTML mail, 0141 windows / dialogs / settings, 0142 views + editors, 0143
  test stability): mail bodies go through `nail_mime::sanitize_mail_html` (style sheets, classes, `cid:` → `data:`);
  layouts follow their window / card width through container queries, never the viewport; split panes with a fixed
  size shrink to their `min-sizes`; a dialog's last button row is sticky (one row at the end, not several). E2E:
  `p1-mail.spec.ts`, `p1-windows.spec.ts` (coverage = the a11y window sweep's dialog list), `p1-views.spec.ts`; a new
  dialog must be listed in one of them. Avoid `:has()` with descendant searches in skins (jsdom / axe cost).
- P1 verify (ADR 0147): a one-line (`nowrap`) label belongs in a column sized by its text (`max-content`), never in a
  `minmax(0, 1fr)` column that can shrink under it (the text runs under the next control; the helper's `overflowing`
  check finds it).
- Control widths (ADR 0160): text-like controls (`form.input` / `form.select` / `form.textarea`) fill their field's
  column or block by default (`theme/skin.ts`: `width: 100%`, `flex-basis: content` so a flex row keeps their content
  as basis); a control that is small on purpose carries `data-width="short" | "medium" | "content"` (on the control or
  its picker's root; `CONTROL_WIDTHS`, `SelectField width=`, `NumberField` and number / time inputs are `short`); rows
  of controls side by side are listed in `skinResponsive.ts` `FIELD_ROWS`. The responsive helper's `undersized-control`
  check fails a control left at the browser's default width (free room beside it, or short of its column's shared right
  edge); `p1-windows.spec.ts` has a `pim` group (the PIM dialogs) and `expectLinedUp` for To / Cc / Bcc / Subject,
  account setup, rule editor, event and contact editor fields.
- Accent in High contrast (ADR 0161): the user's accent override is dropped while the active theme's kind is
  `high-contrast` (fixed, Follow Windows, user themes extending it; `nail_api::appearance::accent_ignored`, UI
  `accentIgnored`); `theme.kdl` keeps the value; Settings → Appearance disables the accent with a note.
- Inline images (ADR 0162): `AttachmentInfo.shown_in_body` = the sanitized HTML body keeps a `cid:` reference to the
  part and it is an image the reader embeds (`SanitizedHtml::inline_shown`); the reader's `widgets/AttachmentList.tsx`
  hides those cards behind "Show all attachments (N)" (`aria-expanded`); `has_attachments`, plug-in bodies and the
  print view leave them out too. `inline` stays the header guess.
- P2 contract (ADR 0148): a conversation listed in a folder = the thread's messages there + the user's own messages
  of the thread from the same accounts' Sent folders (`from_me`); drafts never join (Drafts lists each draft alone);
  without a folder every folder but Drafts / Trash / Junk. "me" comes from the accounts' identities. The default
  layout lists conversations (`message-list threads="collapsed"`; `expandable` = Design B, `expanded` its old name);
  a conversation row selects its newest message (`ThreadSummary.latest_message`, found by `findSummary`). The
  reader's strip shows the newest three messages and folds the rest ("N earlier messages"), styled by the UI skin
  (no `thread-view.*` rules in defaults/theme.kdl). E2E row counts are conversation counts (demo-multi unified
  inbox 14, demo inbox 11). Slots: WP201 core (nail-store, nail-core conversations.rs / mail.rs), WP202 list
  (widgets/MessageList*, messageListSkin.ts, template/, features/conversation-list, list keys), WP203 reader
  (widgets/ThreadView*, threadViewSkin.ts, features/conversation-reader, reader stack + keys), WP204 e2e
  (p2.spec.ts, visual screens, richer demo threads, userDocs mail); both UI packages regenerate the layout
  fixtures / goldens, the integrator regenerates them after merging.
- P2 integration (ADR 0153; packages ADRs 0149 core, 0150 list, 0151 reader, 0152 demo conversations + e2e): the
  default reader is `thread-view mode="cards"` (toolbar + feed, `sizes="auto 1fr"`); the subject lives in the feed's
  `thread-view.subject` h1 (or `message-view.header.subject` when a message shows alone, `data-mode="single"`), so
  e2e waits on `READER_SUBJECT` from `e2e/support/views.ts`. Strip-era user layouts keep the strip and get a
  one-time offer. Seeds of `nail serve` include the demo conversations (`DemoMail::WithConversations`): demo-multi
  unified inbox 17 rows, demo inbox 14, work inbox 15, demo Sent 5; Nightingale counts 7 (Inbox + Sent); core tests
  keep the plain corpus. E2E: `p2.spec.ts`, `p2-list.spec.ts`, `p2-quickwin.spec.ts` (strip + offer).
- P2 merge (ADR 0163): the shell's updater announces changes one at a time (`Shared::notify_order`; the core's
  notifier reads the state itself) and a downloaded release stays ready across later checks; the UI cancels a running
  `GetUpdateStatus` on `UpdateStatusChanged`. The mock Graph / EWS tenants' dates are absolute (2026-10-05 …): a spec
  that syncs them pins the core clock (`control(server, "clock-set", { at })`, see `exchange.spec.ts`).
- P3 contract (ADR 0164): `ApplyMailAction { targets, action }` acts Gmail-like on conversations (`MailAction::reach`:
  Archive / Move / Snooze / Junk = the listed folder's messages, Delete / read / tags / unflag = every message the row
  counts incl. Sent replies, Flag = the newest not written by the user, Delete permanently only in Trash / Junk); one
  batch, one undo (`UndoMailAction`, `mail_batches`); remote sessions act within their grant (never refused for hidden
  messages). Until WP-actions-core the core's stub forwards per message without undo; the UI mock implements all of
  it. Schema v17: `thread_activity` (WP-perf: v18 maintenance, `ListThreads` on it, `thread-page-p95-ms` ≤ 10).
  Mail frames: local `sandbox="allow-same-origin"` (never scripts), remote a hash-pinned height script on the sandbox
  origin (never same-origin).
- P3 integration (ADR 0169; packages ADRs 0165 actions core, 0166 thread-activity speed, 0167 actions UI, 0168 card
  height): every row / toolbar / key / menu action acts on the conversation through one `ApplyMailAction` batch with
  undo (toast, `z`); a card's own actions (its More menu) act on that message; Delete permanently only in Trash /
  Junk, asked with the count. New default keys `#` delete, Shift+I / Shift+U read / unread, `!` junk, `l` labels,
  `z` undo. MCP `update_messages` / `delete_messages` and plug-in `nail.mail.update` / `delete` take
  `conversations` (SDK `PluginMailTargets`). Schema stays v18 (no v19). HTML mail frames are as tall as their mail
  (no 40vh minimum). E2E: `p3-actions.spec.ts`, `p3-card-height.spec.ts`; `thread-page-p95-ms` budget 10.
- `nail serve` and the desktop app start the MCP server (the settings' fixed port; `--mcp-port`,
  `--no-mcp` for serve); the endpoint is in `<data-dir>/mcp-endpoint.json`. `nail serve` and seeded
  runs never open an editor (`open_editor`). One process per data directory (`DataDirLock`: serve,
  desktop, headless `nail mcp`).
- M2c review fixes (ADR 0049): the store records where Nail's Sieve script was uploaded and which
  rules it runs (arrival rules skip them across restarts and failed uploads; renamed / moved /
  switched-off / deleted-account scripts are removed from the recorded place); snooze wakes are
  one store transaction (`Store::wake_snoozed`), stranded hidden-folder mail is swept to the
  Inbox; the hidden Snoozed folder is the *marked* one (`folder_marks`; a user's `Snoozed` stays
  visible, Nail's is then `Nail Snoozed`); wake notifications are per account and folder; server
  search hits are `SearchHit.from_server`, capped at a page and not counted in paging
  (`Service::search_with`, `ServerSearchMode`; MCP searches locally); `nail mcp` checks the
  data-dir lock before trusting the endpoint file; `scripts/interop` has ManageSieve tests
  (`sieve` server feature; offline also against the mock).
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

## If you are the lead (the main session)

The owner starts Claude Code in the master folder `<master>`. That folder
is only the launch point: drive the work folders by absolute path (`git -C`, `cd` in each command), and edit, build,
test and commit only there; in the master run nothing but `git merge --ff-only work-1`. A new session picks up the work
like this:

1. Read `.claude/memory/next-steps.md` (the current wave and what comes next) and the wave's status line in
   `docs/plan.md`.
2. `git fetch` in `nail-work-1`, then check the package branches: `git log --oneline main..origin/work-<N>`. Agents belong
   to the session that started them: a package with commits on its branch and no running agent is finished or
   half-finished — check it in its folder (`npm run check`, its tests), then merge it or let an agent finish it.
3. Run waves with the `nail-wave` workflow (`.claude/workflows/README.md`): packages in `nail-work-2` and up, reset to
   `main`; reviewers with a fresh context; merge into `work-1`, full suite, push `work-1`,
   `git -C <master> merge --ff-only work-1`.
4. CodeGraph's index lives in `nail-work-1/.codegraph` (the master has none): pass
   `projectPath: <work root>\nail-work-1` to `codegraph_explore`, or run `codegraph explore` there.
5. Every agent prompt names the folder, the branch and the work package; agents report the commands they ran and
   their results.
6. The owner reviews proposals and milestone results as HTML pages (artifacts); keep replies short.

## Git

- `main` lives in the master and moves only by fast-forward to `work-1` after a green lead stage. Work folders commit on
  their own `work-<N>` branch. Commit only when asked, or as part of an agreed workflow stage.
- End commit messages with the Co-Authored-By / Claude-Session lines required by the session.

## Memory

Project memory lives in the repo at `.claude/memory/` (one fact per file, index in `.claude/memory/MEMORY.md`). Don't store project memory anywhere else. Update or delete entries that turn out to be wrong.
