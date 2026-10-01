# ADR 0063: M3a integration — calendars, contacts and tasks in the core

Status: accepted · 2026-10-01 · M3a WP75

## Context

Wave M3a built the PIM pieces in parallel against the contract of ADR
0058: `nail-ical` (ADR 0059), `nail-dav` with its mock server and the
Radicale interop tests (ADR 0060), the PIM store, search and reminder
scheduler (ADR 0061) and the PIM UI (ADR 0062). WP75 wires them into the
core, MCP, plug-ins, the shells, the default layout and the demo, and adds
the end-to-end scenarios (plan section 8, WP75).

## Decisions

### Contract changes (requests of the packages)

- `EventOverride.this_and_future` (`#[serde(default)]`): an override
  written by another client with `RECURRENCE-ID;RANGE=THISANDFUTURE`.
  `nail-ical` fills it when reading and `expand` honours it (as well as
  `ExpandOptions::this_and_future`). Nail itself splits a series instead.
- `AlarmRelated` documents the task rule of RFC 5545: `start` counts from
  `DTSTART`, `end` from `DUE`, each falling back to the other.
- `nail_api::plugins::PluginCalendarEvent` (+ `MAX_PLUGIN_CALENDAR_*`) for
  `nail.calendar.events`; `PluginPermission::CalendarRead` is no longer a
  placeholder (`is_placeholder` is always false now).
- The layout widgets of WP74 got typed options (`widgets/pim.rs`), the
  new kind `calendar-list`; `PIM_BASE_CSS` is appended to the compiled
  base CSS; the default theme has the PIM parts and four icons.

### Core (`nail-core/src/pim/`)

- `mod.rs` routes the 52 requests; `PimCtl` in the service holds the
  source engines, the reminder scheduler and the search index. Every item
  is saved twice — the contract type (`PimItem`) and the raw text
  `nail-ical` wrote by **patching the stored original** — in one store
  transaction that also queues the server write (local items are never
  queued). Ids: random `ev-` / `task-` / `ct-` / `grp-` / `cal-` /
  `book-` for items made here, derived from collection + `UID` for items
  a server or feed sent (a resync finds the same rows); `UID`s are
  `<uuid>@nail` (events, tasks) and `urn:uuid:<uuid>` (vCard 4) or a bare
  UUID (vCard 3, CardDAV).
- **Validation** adds what the contract cannot check: zones must exist,
  an end in another zone must not precede the start (instants compared).
  Issues answer `invalid_request` with the first issue's Fluent id.
- **Edit scopes** use `nail-ical::edit`: `this` = override / `EXDATE`;
  `this-and-following` = `split_series` (the new series is written with
  `RELATED-TO` the old `UID`; the editor's recurrence replaces the shared-
  out one only when the user changed it; on the first occurrence it is
  `all`); `all` replaces fields and recurrence and moves recurrence ids,
  `RDATE`s and `EXDATE`s with a new start (as the UI mock does).
- **Local defaults**: on first use the local source gets "My calendar",
  "My tasks", "My contacts" (localized at creation) and the Collected book.
- **Sources** come from `accounts.kdl`; `reconcile_pim_sources` (start and
  every accounts change) mirrors them into the store (removed sources' data
  goes) and (re)starts one engine task per enabled source. Secrets: a DAV
  source with `account=` uses that account's incoming login (password or
  OAuth2 through the core's token manager), otherwise its own
  `keychain:nail/pim/<id>/<slot>`.
- **Engines** (`engine.rs`): a pass runs at start, every `refresh` minutes
  (`EngineTiming::poll_override` in tests), on `SyncPimSources` and when a
  write is queued. Retryable failures (`network`, `unavailable`,
  `rate_limited`) set `offline` with a doubling backoff from
  `reconnect_min` to `reconnect_max`; `auth_failed` sets `auth-error`.
  - ICS: conditional GET (`ETag`, `Last-Modified`) with reqwest on Nail's
    TLS; `webcal:` → `https:` (`http:` only for loopback); one read-only
    calendar `<source>-feed`, settings from the configuration or the feed;
    `alarms=#false` strips alarms; items missing from the feed go.
  - CalDAV / CardDAV: discovery once (`nail_dav::discover`, SRV through
    `SystemDns`; stored as JSON on the source row) and a collection listing
    every pass; collections keep the user's settings after the first sight;
    `sync_collection` with the store's href→ETag map; changed objects go
    through `apply_server_item` (server wins, local kept as conflict copy).
    The queue is replayed before each sync by a `PimReplayer` over
    `write_object` / `delete_object` / `MKCALENDAR` / `MKCOL` / `PROPPATCH`
    (`412` → `Conflict`); `PimOpStatus` events report done / failed.
- **Instances** come from the store's cache with an `InstanceExpander`
  over `nail-ical` (viewer zone UTC = the cache's frame); cancelled
  occurrences are left out. The zone database version is noted at start.
- **Reminders** (`reminders.rs`): the `nail-triage` scheduler on the
  store, on the triage clock. Plans cover now − 2 days … now + 7 days
  (missed ones fire late once), again hourly, after item changes, syncs
  and calendar changes. Fired reminders become `ReminderDue` plus a
  `NotificationRequested` (`NotificationKind::Reminder`) when the
  notification settings are on; **during quiet hours they are shown
  without sound** (ADR 0058 said quiet hours do not hold reminders back —
  kept — and the sound is the part quiet hours mute). Dismiss writes
  RFC 9074 `ACKNOWLEDGED` into writable items; snooze stays local (a
  snooze alarm would be dropped by the next plan, see follow-ups).
- **Search**: the PIM index (`nail_search::PimIndex`) lives **in memory**,
  built from the store on first use and updated on every change. PIM data
  is small; nothing of it reaches the disk outside the sealed store, so the
  vault migration has nothing to convert (WP73 offered the sealed on-disk
  directory; not needed now).
- **Imports** read a desktop file (size limit, launch arguments) or
  uploaded text; an item whose `UID` is in the target collection is
  updated. **From email** reads the message through the reader (decryption
  and the remote `encrypted` gate apply); an attached `text/calendar` is
  used when the message is not end-to-end encrypted.
- **Address autocomplete**: `SuggestAddresses` = `Store::suggest_recipients`
  (contacts with autocomplete, Collected, recent). Sent mail fills the
  Collected book (`collect_addresses`); incoming mail is only counted
  (`record_addresses`), so spam senders do not become contacts.

### MCP and plug-ins

- `McpData::pim` / `pim_raw` (defaults answer `unsupported`, so fakes keep
  compiling); `CoreData` answers through `Service::handle` and the store.
  Resources `nail://calendars`, `nail://calendars/{id}/events{?from,to}`,
  `nail://contacts`, `nail://contacts/{id}`, `nail://tasks`; tools
  `list_events`, `find_free_time` (working hours `day_start` / `day_end`
  clipped per day in `tz`), `search_contacts`, `list_tasks`. All need
  `read`; a calendar or address book is visible when the grant covers its
  source's account, every one with `all_accounts`
  (`McpGrant::allows_pim_source`). PIM content is wrapped as untrusted.
- `nail.calendar.events(from, to, {limit})` (`calendar.read`): occurrences
  of the shown calendars (title, times, place, recurring), at most 92 days
  and 500 events; the SDK, its test host and the generated reference
  follow.

### Shells, layout, demo

- The launch-argument parser moved from `app/src-tauri/src/launch.rs` to
  `nail_core::launch_args` (re-exported by the shell); `nail serve
  --launch <file-or-link>` hands launch requests to the core like the
  desktop shell. The UI opens the import dialog (`.ics` / `.vcf`, previewed
  as `PimImportSource::File`) or the sources dialog (`webcal:`).
- Default layout: windows `calendar` (toolbar `calendar`, mini-month,
  calendar list, task list, calendar views), `contacts` (list + card) and
  `tasks`; actions `pim.*`, `calendar.*`, `tasks.*`, `contacts.*`; keys
  `g c` / `g p` / `g t` and the `calendar` context; main toolbar Calendar /
  Contacts / Tasks; reader toolbar and message menu "Create event / task
  from email".
- `--seed demo-multi` adds a `MockDavServer` (sample events and contacts,
  sources `work-calendars` / `work-contacts`) and two ICS feeds on the mock
  HTTP server (`holidays` configured, `town` for adding), with the test
  controls `dav-objects`, `dav-put`, `dav-delete` and `feeds`.

### Remote sessions

PIM requests still need a grant for every account (the contract-stage
rule); answers are not narrowed per source yet (follow-up).

## Consequences

- Core tests: `tests/m3a.rs` (every request served), `tests/pim.rs` (local
  flows, MCP grants), `tests/pim_sync.rs` (CalDAV / CardDAV both ways, auth
  error, ICS read-only, reminders at the clock time), `tests/plugins.rs`
  (`calendar.read`); Playwright `e2e/tests/pim.spec.ts` (nine scenarios)
  and the a11y sweep over the PIM windows and dialogs.
- Follow-ups (plan "M3a follow-ups"): per-source remote narrowing; RFC 9074
  snooze alarms on the server (with planning that keeps them); replanning
  on an OS zone change (today hourly); a conflict choice in the event
  editor (the contact card has it); `RDATE;VALUE=PERIOD` ends; DNS TXT
  paths; scheduling / iTIP (M3b).
