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
- **No non-loopback sockets.** Never open sockets on non-loopback/unspecified addresses (TCP or UDP), except the user-enabled remote UI listener; use the OS DNS API; tests bind 127.0.0.1 (ADR 0057; `scripts/check-socket-binds.mjs` runs in `npm test`).
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
                       ADRs 0087, 0092)
crates/nail-store      SQLite metadata (v13), blob store, per-account operation queue with sync-side
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
crates/nail-mime       MIME parsing, HTML sanitizing, top-level header list (rules); message_file.rs
                       (read_message_file, localized MessageFileError, MAX_MESSAGE_FILE; ADR 0054); domain.rs,
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
                       to NAIL_FAKE_INSTALLER_RECORD instead of installing; ADR 0092)
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
                       the rendered tree
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
crates/nail-mapi       Simple MAPI provider (M4b WP111): cdylib nail_mapi.dll (shipped as mapi\<arch>\NailMapi.dll)
                       + rlib + bin nail-mapi-harness (send / documents / exports / registry; without arguments a
                       fake Nail.exe on NAIL_MAPI_PIPE); windows/exports.rs (13 Simple MAPI exports, signatures
                       checked against windows-sys LPMAPI*), windows/decode.rs + memory.rs (VirtualQuery-checked
                       reads of caller structs), windows/client.rs (named-pipe client, same-user server check,
                       starts Nail.exe, NAIL_MAPI_APP only together with NAIL_MAPI_PIPE), windows/server.rs
                       (reference pipe server), forward.rs, staging.rs (attachment copies in %TEMP%\nail-mapi,
                       limit nail_api::compose::MAX_ATTACHMENT_BYTES), registration.rs (HKLM Clients\Mail\Nail
                       values, .reg rendering) (ADRs 0087, 0088)
crates/nail-server     axum: UI bundle, WebSocket API, blobs, test controls (demo-multi only);
                       M2b: bind_with (TLS rustls + ring, rcgen certs, instant-acme, headers,
                       sandbox origin, proxy route, `auth`), remote_ui.rs RemoteUiHost (ADRs 0038, 0041);
                       lan.rs: LAN address from the interfaces, no socket (ADR 0057);
                       M2c: render tokens bound to the session (sandbox.rs), proxy_limit.rs (ADR 0048)
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
                       NAIL_REGISTRY_ROOT; MemoryRegistry for seeded runs), install.rs (InstallationInfo,
                       UpdateSource from plugins.updater with {{channel}} or <channel>/latest.json,
                       NAIL_UPDATE_ENDPOINT / NAIL_UPDATE_PUBKEY loopback-only test override), default_apps.rs,
                       startup.rs (Run key; the MSI's machine-wide entry counts, never duplicated),
                       updater.rs + updater_tauri.rs (update states over tauri-plugin-updater; installs a
                       downloaded update on restart or quit), dev_endpoint.rs (127.0.0.1, bearer token, no
                       Origin), mapi_server.rs (named pipe, first instance, current-user DACL; waits for the
                       client to close before disconnecting), win.rs, host.rs (ShellHost: DesktopHost), tray.rs,
                       shell.rs (tray menu, second start, seeded-only test control `nail_test_shell`);
                       tests/mapi_wired.rs (DLL client → pipe server → core; ADR 0092);
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
                       WP105 mounts: composer/pluginExtensions.tsx, reader in
                       widgets/MessageView.tsx (ADR 0084);
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
                       nail plugin dev, first run; ADR 0092))
defaults/              shipped theme.kdl + layout.kdl; themes/ (example themes: high-contrast.kdl)
locales/               Fluent catalogs (en, de); core*.ftl = texts built in Rust (M2c; core-plugins.ftl,
                       core-crypto.ftl (pgp-*, message-file-*) M2d); itip.ftl = iMIP mail texts (M3b);
                       M3c: core-exchange.ftl (WP93 + contract ids), core-graph.ftl (WP91), core-ews.ftl
                       (WP92), exchange.ftl (WP94); M4a: core-agents.ftl (contract ids: kinds, states,
                       summaries, fields), core-mcp.ftl (WP101: prompts, tool fields / values, elicitation,
                       audit texts), agents.ftl (WP104), core-plugins.ftl (WP102 host texts, plugin-write-log);
                       M4b: core-desktop.ftl (contract ids: install kinds, update channels / failures / reasons,
                       default apps, mapi-result-*; WP113 shell texts), system.ftl (WP114);
                       every file is embedded in nail-config l10n.rs
scripts/               build-installer.mjs (NSIS + MSI, --arch / --offline-webview / --update-base, NailMapi.dll),
                       sbom.mjs, installer-smoke.ps1, msi-smoke.ps1, update-manifest.mjs, update-server.mjs
                       (loopback), update-check.mjs, updater-key.mjs, winget-manifest.mjs, release-version.mjs,
                       import-signing-cert.ps1 / verify-signatures.ps1, lib/ (minisign, sign, winget, …; M4b
                       ADR 0089), check-socket-binds.mjs (ADR 0057), test/ (node:test);
                       interop/ = separate Cargo workspace (copy the root Cargo.lock in first) (ADR 0040)
```

## Commands

Full details in `docs/dev-setup.md`; keep both in sync.

```
npm ci                                     # npm workspaces: ui, e2e, packages/*
npm run build                              # ui/dist — required before any `cargo … --workspace` (Tauri embeds it)
cargo test                                 # default members = all crates except app/src-tauri
cargo test --workspace                     # incl. the Tauri shell (after npm run build)
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
npm test                                   # socket-bind check (ADR 0057), Vitest (ui) incl. the lint-rule self-test, packages/* `test`
npm run check:sockets                      # only the socket-bind check (scripts/lib/socket-binds.mjs: rules + allowlist)
npm run lint                               # ESLint + tsc + Prettier (ui), tsc + Prettier (e2e)
npm run e2e                                # builds ui + nail-cli + desktop app, then Playwright (docs/dev-setup.md)
npm run e2e:only                           # Playwright against what is already built
npm run build:desktop                      # nail-app (custom-protocol) for the desktop smoke test, own target dir
npm run gen:api                            # regenerate ui/src/api/generated + packages/plugin-api/src/generated (a test fails when stale)
npm run docs -w packages/plugin-api        # regenerate the reference sections of userDocs/en/plugins/{api-reference,components}.md (a test fails when stale)
node packages/create-nail-plugin/index.mjs <dir> [--template command|hook|composer|attachment] [--id] [--name] [--sdk]   # scaffold a plug-in
npm run package --prefix examples/<name>   # build + pack an example (follow-up, attachment-check, csv-preview; dist/ gitignored)
nail plugin dev <folder> [--url URL | --data-dir DIR]   # load a plug-in folder into the running `nail serve`, hot reload, log console
node --test "scripts/test/*.test.mjs"      # installer / release script tests
cargo test -p nail-mapi --target i686-pc-windows-msvc   # the x86 Simple MAPI DLL under WOW64 (rustup target add i686-pc-windows-msvc)
cargo deny check                           # licences, bans (openssl, aws-lc), sources, advisories (deny.toml)
node scripts/build-installer.mjs [--skip-ui] [--arch x64|arm64] [--bundles nsis,msi] [--offline-webview] [--dev-updater-key]   # installers, portable zip, SBOMs → build dir (never the repo)
powershell -File scripts/installer-smoke.ps1   # install to a temp dir, start, [update,] uninstall
powershell -File scripts/msi-smoke.ps1         # elevated (CI): silent MSI install with properties, start, upgrade, uninstall
node scripts/update-server.mjs --root DIR      # loopback update server; update-manifest.mjs / winget-manifest.mjs write manifests
node scripts/sbom.mjs                      # CycloneDX SBOMs
NAIL_UPDATE_SNAPSHOTS=1 cargo test -p nail-config --test schema   # regenerate config JSON Schemas (theme, layout, accounts)
NAIL_UPDATE_SNAPSHOTS=1 cargo test -p nail-config --test rules_schema   # rules.kdl JSON Schema
NAIL_UPDATE_SNAPSHOTS=1 cargo test -p nail-config --test defaults  # nail-api fixtures after editing defaults/*.kdl (then prettier)
bash crates/nail-testkit/corpus/crypto/pgp/generate.sh     # OpenPGP interop vectors (GnuPG 2.4 = Git for Windows /usr/bin/gpg)
bash crates/nail-testkit/corpus/crypto/smime/generate.sh   # S/MIME test CA + vectors (OpenSSL 3.4+ = /mingw64/bin/openssl)
cargo run -p nail-cli -- serve --seed demo --open                 # web mode with demo data (127.0.0.1:4780)
cargo run -p nail-cli -- serve --seed demo-imap                   # demo corpus synced from an in-process mock IMAP server
cargo run -p nail-cli -- serve --seed demo-multi                  # IMAP + POP3 + SMTP mock accounts, mock CalDAV / CardDAV (+ principal bob) + ICS feeds, a mock Gmail account, mock Exchange Online (Graph) + Exchange Server (EWS) accounts (all three switched off) and mock Autodiscover (test controls at /api/test/…)
cargo run -p nail-cli -- serve --seed demo --launch C:/path/x.ics  # hand a file / link to the UI like the desktop shell (M3a)
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
  `NAIL_MAPI_PIPE` (docs/dev-setup.md). The real MSI install runs only in CI (`msi-smoke.ps1`, elevated).
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

## Git

- Branch `main`. Commit only when asked, or as part of an agreed workflow stage.
- End commit messages with the Co-Authored-By / Claude-Session lines required by the session.

## Memory

Project memory lives in the repo at `.claude/memory/` (one fact per file, index in `.claude/memory/MEMORY.md`). Don't store project memory anywhere else. Update or delete entries that turn out to be wrong.
