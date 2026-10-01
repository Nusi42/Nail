# ADR 0058: M3a contract — calendars, contacts, tasks, PIM sources, store v10, slots

Status: accepted · 2026-10-01 · M3a WP70

## Context

Wave M3a (plan section 8) adds calendars, contacts and tasks from local
storage, ICS subscriptions and CalDAV / CardDAV servers: iCalendar /
vCard and recurrence (WP71, `nail-ical`), the DAV client and its mock
server (WP72, `nail-dav`, `nail-testkit-net`, `scripts/interop`), the PIM
store, search and reminder scheduler (WP73, `nail-store`, `nail-search`,
`nail-triage`) and the PIM UI (WP74), built in parallel and integrated by
WP75. This ADR fixes the contract they build against, the store schema
they share and the slots that keep them apart.

Constraints (plan "Scope decisions for M3"): no GPL / LGPL / AGPL
dependency, no OpenSSL / aws-lc; no non-loopback sockets (ADR 0057); MCP
gets read-only PIM access in M3, writes stay in M4; strings only from
Fluent catalogs; the core owns data and every write goes through the
(PIM) operation queue.

## Decisions

### Model (`nail_api::pim`)

- **Times** are `PimTime` (tagged `type`): `date {date}` (all-day,
  `YYYY-MM-DD`, exclusive as an end), `floating {local}` and `zoned
  {local, tz}` (IANA name or `UTC`; Windows / Outlook TZIDs are mapped by
  `nail-ical`, the original `VTIMEZONE` stays in the raw item). Instants
  (reminders, timestamps) are RFC 3339 UTC strings. Validators
  `is_date`, `is_local_date_time`, `is_time_zone_name`, `is_utc_instant`,
  `is_color`, `is_https_or_loopback_url` live in `pim/mod.rs`.
- **Calendars** (`Calendar`, `CalendarSettings`): task lists are
  calendars with the `task` component (CalDAV keeps both in one
  collection); settings = name, description, colour, visibility,
  reminders on/off, default alarms, order. Read-only for subscriptions.
- **Events** are stored as a **series**: `CalendarEvent {fields:
  EventFields, recurrence: Recurrence?, overrides: [EventOverride
  {recurrence_id, fields}]}`. `EventFields`: title, start, end
  (`DURATION` becomes an end, written back as it was), location,
  description, status, transparency, visibility, priority, categories,
  colour, URL, organizer, attendees (role, `PARTSTAT`, `CUTYPE`, RSVP,
  delegation), alarms (`Alarm`: action, relative / absolute trigger,
  repeat, RFC 9074 `UID` / `ACKNOWLEDGED`), conferencing links (RFC 7986
  `CONFERENCE`), attachments (URL or inline by SHA-256 in the blob store),
  `PimLink`s to messages (written as `ATTACH:mid:` RFC 2392).
  `Recurrence` = structured `RecurrenceRule`s (all `BY*` parts,
  `COUNT` / `UNTIL` as `RecurrenceEnd`, `WKST`) + `RDATE`s + `EXDATE`s;
  `RecurrenceRule::is_valid` checks RFC 5545's ranges.
- **Edit scopes** (`EditScope`: `this` | `this-and-following` | `all`,
  default `all`): `this` writes an override (delete: an `EXDATE`);
  `this-and-following` splits the series — the original ends before the
  occurrence (`UNTIL`), a new series (new `UID`, `RELATED-TO` the old one)
  starts with it and is returned as `EventSaved.split`; `all` changes the
  series (moving the start shifts the overrides' recurrence ids).
- **Instances** (`EventInstance`): views ask for occurrences in a range
  with a viewer zone (`ListEventInstances {from, to, calendar_ids?, tz}`),
  served from the instance cache; they carry the stored start / end, the
  resolved UTC instants and a summary (title, location, status,
  transparency, colour, recurring / overridden, attendees / alarms flags).
- **Contacts** (`Contact`, `ContactFields`): vCard 3.0 / 4.0 fields —
  kind, `FN` (required), `N`, nicknames, typed emails / phones / URLs /
  IMPP / related (`LabeledValue {value, kinds, label, preferred}`),
  several postal addresses, organization, title, role, birthday /
  anniversary (`ContactDate` without year allowed), notes, categories,
  photo (stored in the blob store; written as base64 `data`), custom
  fields. The card keeps its version (`VCardVersion`) and unknown
  properties. Lists use `ContactSummary`.
- **Groups** (`ContactGroup`): vCard 4 `KIND:group` + `MEMBER`, or
  Apple-style groups in vCard 3 books; members are contacts of the same
  book or bare addresses.
- **Merging**: `MergeContacts {primary_id, contact_ids, fields}` folds
  contacts of the primary's address book into it (their cards are
  deleted) and **links** contacts of other books to it
  (`Contact.linked_ids`; each keeps its card — design: "each merged
  contact still linked to its sources"). `UnlinkContact` undoes a link.
  `FindDuplicateContacts` reports groups by email, phone or name.
- **Tasks** (`Task`, `TaskFields`): start, due, status, percent, priority
  (1–9), completed time, categories, location, URL, visibility, alarms
  (relative to due), parent (`RELATED-TO;RELTYPE=PARENT`, same list),
  manual order (`X-APPLE-SORT-ORDER`), links; `subtask_ids` on the task.
  `CompleteTask` on a recurring task completes the current occurrence and
  moves the task on (override kept).
- **Reminders** (`Reminder {target: event occurrence | task, fire_at,
  state: pending | fired | snoozed | dismissed, snoozed_until, …}`).
  Snooze and dismiss write `ACKNOWLEDGED` / a snooze alarm (RFC 9074)
  through the PIM queue where the server keeps them.
- **Sync state per item** (`PimItemSync`: `synced` | `pending` |
  `conflict`). Conflicts: the server wins, the local version is kept as a
  conflict copy until `ResolvePimConflict {item, keep: server | local}`.
- **Validation**: `issues()` on `EventFields`, `Recurrence`, `Alarm`,
  `TaskFields`, `ContactFields`, `CalendarSettings`,
  `AddressBookSettings`, `PimSourceConfig`; issues are enums with Fluent
  ids (`PimIssue` → `pim-issue-*`, `PimSourceIssue` →
  `pim-source-issue-*`, in `core-pim.ftl`). The core answers invalid data
  with `invalid_request` + the issue id as `localized`.
- New ids: `PimSourceId` (`local` reserved), `CalendarId`,
  `AddressBookId`, `EventId`, `ContactId`, `ContactGroupId`, `TaskId`,
  `ReminderId` (opaque except the source id, which is the node name).

### Sources and where their settings live

Decision: **`accounts.kdl`**, as new top-level nodes (no separate file,
no `pim` block), because ADR 0013 keeps all connection settings there
and DAV servers usually share a mail account's login:

```kdl
calendar-subscription "holidays" url="webcal://example.org/holidays.ics" color="#d33" refresh=1440
caldav "work-calendars" account="work"
carddav "work-contacts" account="work" url="https://dav.work.example/"
caldav "home" url="https://dav.home.example/" { auth "password" username="ada" }
```

- `AccountsConfig.pim_sources: [PimSourceConfig {id, name?, enabled,
  settings: ics {url, color, alarms, refresh_minutes, auth} | caldav /
  carddav {account?, url?, auth?, refresh_minutes}}]`, file order.
- A DAV source with `account=` uses that account's incoming login and
  email domain (RFC 6764 discovery when `url` is missing); without an
  account it needs `url` and an `auth` with a `username`. Secrets default
  to `keychain:nail/pim/<id>/<slot>` (`PimSourceConfig::default_secret`),
  never clashing with account secrets. URLs: `https:` (plus `webcal:` /
  `webcals:` for subscriptions); `http:` only for loopback hosts.
- The built-in **local** source (local calendars, task lists, address
  books and the "Collected" book) is not in the file and cannot be
  removed. Collections are discovered / created at runtime and live in the
  store, not the file.
- `nail-config` reads (positions, `config-pim-*` errors in `core.ftl`),
  writes format-preserving (`AccountsDocument::upsert_pim_source`,
  `remove_pim_source`, `derive_pim_source_id`; removing an account
  removes the DAV sources that use its login) and describes them in the
  JSON Schema; documented in `userDocs/en/accounts-reference.md`.
- **Discovery** in M3a: SRV through the OS resolver
  (`nail_discovery::DnsLookup`) and `/.well-known/` URIs; **no TXT
  lookups** (RFC 6764 makes `path=` optional; the resolver has no TXT
  query yet).

### Requests, responses, events

52 requests; every write answers with the saved item and the queued
operation ids (the PIM queue reports through `Event::PimOpStatus {status:
PimOpStatus {op_id, source_id, state, error, message, localized}}`).

| Area | Requests | Responses |
| --- | --- | --- |
| Sources | `ListPimSources`, `AddPimSource {config, secret?}`, `UpdatePimSource`, `RemovePimSource`, `TestPimSource` (discovery + login + collections), `SyncPimSources {source_id?}` | `PimSources`, `PimSourceSaved`, `PimSourceTested`, `Done` |
| Calendars | `ListCalendars`, `CreateCalendar {source_id, settings, components}`, `UpdateCalendar`, `DeleteCalendar` | `Calendars`, `CalendarSaved`, `PimDeleted` |
| Events | `ListEventInstances`, `GetEvent`, `CreateEvent`, `UpdateEvent {scope, recurrence_id?}`, `DeleteEvent {scope, recurrence_id?}`, `MoveEvent`, `EventFromMessage`, `FindFreeTime` | `EventInstances`, `PimEvent`, `EventSaved {event, split?, op_ids}`, `EventSuggested`, `FreeTime` |
| Address books | `ListAddressBooks`, `CreateAddressBook`, `UpdateAddressBook`, `DeleteAddressBook` | `AddressBooks`, `AddressBookSaved` |
| Contacts | `ListContacts` (paged), `GetContact`, `CreateContact`, `UpdateContact`, `DeleteContacts`, `MoveContact`, `SearchContacts`, `FindDuplicateContacts`, `MergeContacts`, `UnlinkContact`, `GetContactPhoto` | `Contacts`, `PimContact`, `ContactSaved`, `ContactMatches`, `ContactDuplicates`, `ContactPhoto` |
| Groups | `ListContactGroups`, `CreateContactGroup`, `UpdateContactGroup`, `DeleteContactGroup` | `ContactGroups`, `ContactGroupSaved` |
| Tasks | `ListTasks`, `GetTask`, `CreateTask`, `UpdateTask`, `CompleteTask`, `MoveTask`, `DeleteTask {with_subtasks}`, `TaskFromMessage` | `Tasks`, `PimTask`, `TaskSaved`, `TaskSuggested` |
| Other | `SearchPim {query, kinds, limit}`, `PreviewPimImport {source}`, `ImportPim {source, calendar_id?, address_book_id?}`, `ResolvePimConflict` | `PimSearchResults`, `PimImportPreview`, `PimImported` |
| Reminders | `ListReminders {until?}`, `SnoozeReminders {reminder_ids, until}`, `DismissReminders` | `Reminders`, `Done` |

"Create event / task from email" returns **unsaved** fields (title from
the subject, attendees, a link to the message, an attached `.ics` when
present); the UI saves them with `CreateEvent` / `CreateTask`. Imports
take `PimImportSource::File {path}` (desktop only: launch arguments,
`LaunchRequest::ImportCalendarFile` / `ImportContactsFile`) or `Data
{file_name, content}` (uploads); `webcal:` launches become
`AddPimSource` with an ICS subscription. Address autocomplete keeps
`SuggestAddresses` (WP73 ranks contacts + collected + recent inside it).

Events: `PimSourcesChanged`, `CalendarsChanged`, `AddressBooksChanged`,
`EventsChanged {calendar_ids}`, `TasksChanged {calendar_ids}`,
`ContactsChanged {address_book_ids}`, `ReminderDue {reminder}`,
`RemindersChanged`, `PimOpStatus {status}`. A firing reminder also goes
through the notification path as `NotificationRequested` with the new
`NotificationKind::Reminder` (`id` `reminder:<id>`, no account / folder /
messages; quiet hours do not hold reminders back; texts
`notification-reminder-*` in `core-pim.ftl`).

**Served now:** nothing — the core routes every PIM request to
`nail-core/src/pim.rs`, which answers `unsupported` until WP75 (like the
M2d host requests). The UI mock (`ui/src/features/pim/mock.ts`) does the
same; `tests/m3a.rs` checks all 52.

### MCP and remote access

- MCP (read-only in M3): payload and argument types in `pim/mcp.rs`
  (`nail://calendars`, `nail://calendars/{id}/events{?from,to}`,
  `nail://contacts`, `nail://contacts/{id}`, `nail://tasks`; tools
  `list_events`, `find_free_time`, `search_contacts`, `list_tasks`;
  event / contact resources carry iCalendar / vCard text). They need the
  `read` scope. **Grants** stay as they are:
  `McpGrant::allows_pim_source(account)` — a source belonging to a
  granted account is visible, sources without an account (local,
  subscriptions, standalone DAV) only with `all_accounts`. Per-source
  grants may come with the M4 writes. The plug-in `calendar.read`
  permission is made real by WP75.
- Remote sessions (`nail-remote-auth::required_access`): reads `read`;
  event, calendar and reminder writes `calendar`; contact, group, task
  writes, imports and conflict resolution `pim` (an import of events also
  needs `calendar`, checked by the core); deleting a whole calendar /
  address book is `Sensitive`; source administration is `admin`. Until
  WP75 narrows answers per source, the core (`check_remote_request`)
  requires a grant for every account for PIM requests, never imports a
  desktop file for a remote session, checks the message of
  `EventFromMessage` / `TaskFromMessage`, and sends PIM events and
  reminder notifications to full grants only.

### Store schema v10 (`nail-store`, `src/pim.rs`)

`pim_sources` (the `local` row is inserted by the migration; settings
stay in `accounts.kdl`, the row holds discovery and sync state),
`pim_collections` (calendars / address books, settings JSON, href,
`ctag` / `sync_token` / `etag` / `last_modified`, `role` `collected`),
`pim_objects` (the raw iCalendar / vCard resource, href, ETag, sync state,
`conflict_raw`, `deleted`), `pim_events` / `pim_tasks` / `pim_contacts`
(parsed JSON + plain columns for lists, sorting and ranges; groups are
contacts of kind `group` with `pim_group_members`), `pim_contact_emails`
(autocomplete, duplicates), `pim_contact_links` (merged persons),
`pim_event_instances` + `pim_instance_windows` (the instance cache:
Unix seconds, dates and floating times as UTC wall time with `floating`
= 1, read in the viewer's zone with a 14-hour widening), `pim_reminders`
(removed with their event / task by triggers), `pim_ops` (the PIM queue,
per source). Everything cascades from its source. **Sealed at rest**:
`pim_objects.raw`, `pim_objects.conflict_raw`, `pim_events.data`,
`pim_contacts.data`, `pim_tasks.data` (titles and names stay readable
like subjects, ADR 0032). Parallel packages add no migrations; WP75
appends v11 if a package reports a need.

### Crates and libraries

Licences checked (`cargo metadata`, `cargo deny check licenses bans
--offline`); `cargo tree -i openssl-sys` / `aws-lc-rs` stay empty.

- `calcard` 0.3.14 (Stalwart Labs, **Apache-2.0 OR MIT**, `default-features
  = false`: no JSCalendar / JSContact) — iCalendar / vCard parsing and
  writing, RRULE expansion (a port of rust-rrule) and zone detection;
  deps ahash, hashify, chrono, chrono-tz, mail-parser / mail-builder (all
  MIT / Apache-2.0, mostly already in the lock). Chosen over `icalendar`
  (no recurrence, weaker parser) and `rrule` + our own parser (two
  libraries for one job). Its writer upper-cases enumerated values
  (`KIND:GROUP`): WP71 keeps the original text where losslessness
  matters. `chrono-tz` 0.10 (MIT / Apache-2.0) for IANA zones.
- `nail-ical` (WP71, stub): calcard, chrono, chrono-tz; smoke tests parse,
  expand a daily series across the end of summer time in Berlin, round
  trip unknown properties, parse vCard 3 / 4.
- `nail-dav` (WP72, stub): the workspace reqwest (TLS from `nail-auth`,
  ring), quick-xml, url, base64, `nail-discovery` (SRV); smoke tests parse
  a namespaced multistatus and build PROPFIND / REPORT / MKCALENDAR with
  Basic auth. No new HTTP or XML library.

### Collision-free slots

| Area | Slot | Owner |
| --- | --- | --- |
| iCalendar / vCard / recurrence / zones / alarms / feeds | `crates/nail-ical/`, `locales/{en,de}/core-ical.ftl` | WP71 |
| CalDAV / CardDAV client, mock server, Radicale interop | `crates/nail-dav/`, `crates/nail-testkit-net/` (`src/dav.rs` created), `scripts/interop/`, `locales/{en,de}/core-dav.ftl` | WP72 |
| PIM store APIs, instance cache, PIM search, reminder scheduler, collected book, duplicates, autocomplete ranking | `crates/nail-store/` (`src/pim.rs`; no migrations), `crates/nail-search/` (`src/pim.rs`), `crates/nail-triage/` (`src/reminders.rs`), `locales/{en,de}/core-pim.ftl` | WP73 |
| PIM UI | `ui/src/features/pim/` (registered in `features/registry.ts`, mock routed in `api/mock.ts`, state `data.features.pim`, messages in the coverage test), `crates/nail-config/src/widgets/pim.rs` (the six PIM widget kinds moved there), `locales/{en,de}/pim.ftl` | WP74 |

Integrator-owned during M3a (report, do not edit): `Cargo.toml`
(dependencies above are pre-declared — ask before adding more),
`Cargo.lock` beyond a package's own new dependency, `nail-api`,
`nail-store` migrations, `nail-core`, `nail-server`, `nail-mcp`,
`nail-remote-auth`, `nail-config` (except `src/widgets/pim.rs`),
`app/src-tauri`, `defaults/`, `e2e/`, `.github/workflows/`,
`docs/adr` numbering. Contract or schema changes go to WP75 as
`apiRequests`.

## Consequences

- WP71–WP74 compile against fixed types and generated TypeScript; the
  suites stay green in between.
- WP75 wires: sources from `accounts.kdl` (local source, ICS polling,
  CalDAV / CardDAV sync engines with polling and the PIM queue replay,
  conflicts), the requests in `nail-core/src/pim.rs`, the instance cache,
  reminders through the notification path and the desktop shell, launch
  imports (`.ics` / `.vcf` / `webcal:`), "create event / task from email",
  composer autocomplete from contacts, MCP resources / tools with grants,
  the plug-in `calendar.read` permission, per-source remote narrowing, a
  default layout with a calendar window, a mock CalDAV / CardDAV source
  in the demo-multi seed, and the M3a Playwright scenarios.
- Two lists must stay in step with the protocol: the exhaustive matches
  (core dispatch, `check_remote_request`, `remote_event`,
  `required_access`) fail to compile when a request or event is added;
  `PIM_REQUESTS` (UI mock) and `tests/m3a.rs` count 52.
