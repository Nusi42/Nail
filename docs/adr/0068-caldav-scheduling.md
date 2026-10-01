# ADR 0068: CalDAV scheduling, free/busy and the scheduling mock server (WP83)

- Status: accepted (M3b WP83)
- Context: plan section 8, WP83; contract ADR 0065; DAV client ADR 0060;
  sockets ADR 0057.

## Decision

WP83 extends `nail-dav` with CalDAV scheduling (RFC 6638) and free/busy,
and `nail-testkit-net`'s `MockDavServer` with several principals,
schedule inboxes and outboxes, free/busy answers and switchable
auto-scheduling. No new crate: the code uses the workspace `reqwest`,
`quick-xml` and `axum` already in both crates, so no licence changes.

### Modules

| Module | What |
| --- | --- |
| `nail_dav::icalendar` | a small content-line layer: unfolding (CRLF or LF), parameters (quoted, several values), folding at 75 octets (never inside a UTF-8 sequence), `patch_lines` that rewrites only the lines it changes and keeps every other line byte for byte |
| `nail_dav::freebusy` | `VFREEBUSY` requests and replies, parsing (`FREEBUSY` with `FBTYPE`, `start/end` and `start/duration` periods, several per line), clipping and merging, and `own_busy_periods` over expanded instances |
| `nail_dav::scheduling` | discovery of the scheduling properties, outbox `POST`, `schedule-response` parsing, `CalDavFreeBusy` (the contract's `FreeBusyProvider`), the schedule inbox, `SCHEDULE-AGENT` / `SCHEDULE-STATUS` |
| `nail-testkit-net` `dav/schedule.rs`, `dav/ical.rs` | the mock's outbox, free/busy computation and auto-scheduling |

`nail-dav` does not depend on `nail-ical`. Scheduling only reads and writes
a handful of properties and parameters. Going through the full model would
also mean re-serializing objects that the server and other clients wrote.
The line layer keeps unrelated lines untouched. `nail-ical` stays the
model for events.

### Discovery

`discover_scheduling(client, principal, options_url)`:

- **PROPFIND (depth 0) on the principal** reads `schedule-inbox-URL`,
  `schedule-outbox-URL`, `calendar-user-address-set` (only `mailto:`
  entries, without the scheme), `calendar-user-type` and `displayname`. A
  principal with neither inbox nor outbox is `DavError::NoScheduling`
  (`unsupported`, `dav-no-scheduling`).
- **OPTIONS** on a calendar home or calendar (the principal when none is
  given) decides `auto_schedule`: true when the `DAV` header has
  `calendar-auto-schedule`. When OPTIONS fails, Nail assumes the server
  does not schedule, except that a refused login stops discovery.
- **The inbox** gives `schedule-default-calendar-URL`, if it has one.

The core (WP85) keeps `SchedulingInfo` with the source's discovery. It
chooses `SchedulingTransport::Server` when `auto_schedule` is set and
`server_schedules(...)` holds for the event.

### Free/busy

- **Request.** `build_request` writes `METHOD:REQUEST` with a
  `VFREEBUSY` that has a fresh `UID`, `DTSTAMP`, the range as UTC
  `DTSTART` / `DTEND`, `ORGANIZER` and one `ATTENDEE` per address (all
  `mailto:`). `post_free_busy` POSTs it to the outbox as
  `text/calendar`. It does not send the draft-era `Originator` /
  `Recipient` headers, because RFC 6638 dropped them.
- **Batching.** `query_free_busy` sends at most 25 recipients per POST.
  A non-`2xx` answer is a `DavError`: `403` is `Forbidden`, `503` is
  retryable with `Retry-After`.
- **Answers per recipient.** The `schedule-response` gives
  `recipient` (as `href`, or as plain text from older servers),
  `request-status` and `calendar-data`.
  - `2.x`: the reply's busy periods (only from `VFREEBUSY`s that name
    the recipient, or name nobody), clipped to the range and merged
    with `BusyPeriod::merge`.
  - `3.7` and `5.3` (unknown user): the recipient is left out, and the
    core reports it as `FreeBusySource::Unknown`, as the contract says.
  - `3.8`: error `forbidden` with `dav-freebusy-no-access`.
  - Anything else: error `unavailable` with `dav-freebusy-failed`
    (with the status code).

  The entries come back in query order with
  `FreeBusySource::Caldav { source_id }`.
- **`CalDavFreeBusy`** implements `FreeBusyProvider`.
  - It validates the query first (`FreeBusyQuery::issues`). An invalid
    query gives `invalid_request` with the `freebusy-issue-*` text and
    never reaches the server.
  - RFC 6638 lets servers refuse an `ORGANIZER` that is not the calendar
    user. When the query's organizer is not one of the principal's
    addresses, the provider uses the principal's first address instead.
- **Parsing.** `FBTYPE=FREE` is skipped. Unknown `FBTYPE`s count as
  `BUSY` (RFC 5545). Negative durations and periods with end ≤ start
  are dropped. One `VFREEBUSY` gives at most 20,000 periods, so a
  hostile server cannot make Nail allocate without bound.
  `build_reply` writes the matching `METHOD:REPLY`, so the core can
  answer iMIP free/busy requests.

### The user's own calendars

`own_busy_periods(instances, &OwnFreeBusy { from, to, exclude_event,
calendars }, my_status)` takes expanded `EventInstance`s from
`ExpandEvents` or the store's instance cache, already resolved to UTC.
`EventInstance` has no `PARTSTAT`, so the caller supplies the user's
answer through `my_status`.

| Instance | Result |
| --- | --- |
| Cancelled, or transparent (`TRANSP:TRANSPARENT`) | Ignored |
| The user declined or delegated it | Ignored |
| The user has not answered, or answered tentatively | `BUSY-TENTATIVE` |
| `STATUS:TENTATIVE`, accepted or no attendees | `BUSY-TENTATIVE` |
| Anything else | `BUSY` |

The event being edited (`exclude_event`) and calendars that do not count
are left out. The result is clipped to the range and merged.

The contract's slot `nail-store/src/pim/freebusy.rs` stays a stub: in
this parallel stage WP83 owned only `nail-dav` and `nail-testkit-net`. The
function lives in `nail-dav` because it needs nothing from the store.
WP85 calls it with instances from the store.

### Schedule inbox

- **`list_inbox`** finds the members with PROPFIND depth 1 and their
  ETags, then reads them with `calendar-multiget` (at most 500 items per
  pass). Each `InboxItem` has `METHOD`, `UID`, the sender (`ORGANIZER`
  for organizer methods, the first `ATTENDEE` otherwise) and `DTSTAMP`.
  Items come oldest `DTSTAMP` first, so a `REQUEST` is applied before
  its `CANCEL`.
- **`delete_inbox_item`** deletes with `If-Match`. An item that is
  already gone counts as deleted. An item changed since it was read
  (`412`) stays for the next pass.
- **`process_inbox(client, inbox, async |item| decision)`** hands each
  item to the caller, which applies it (`nail-itip`, WP81/WP85) and
  answers `Delete` or `Keep`.

### `SCHEDULE-AGENT` / `SCHEDULE-STATUS`

- **`ScheduleAgent`** is `SERVER`, `CLIENT` or `NONE`. Unknown values
  mean `SERVER`.
- **`server_schedules(ical, own_addresses, auto_schedule)`** is true when
  the server will deliver:
  - as organizer: some attendee other than the user does not have
    `SCHEDULE-AGENT` set to `CLIENT` or `NONE`;
  - as attendee: the `ORGANIZER` does not have `SCHEDULE-AGENT` set to
    `CLIENT` or `NONE`.
- **`set_schedule_agent(ical, Client)`** marks every `ORGANIZER` /
  `ATTENDEE`. Nail uses it when it sends iMIP itself, for example by the
  user's choice or for a calendar on another server, so the server does
  not deliver a second copy. `Server` removes the parameter again.
- **`strip_schedule_params`** removes `SCHEDULE-STATUS` and
  `SCHEDULE-FORCE-SEND`. Both are the server's to write, and an iMIP copy
  must not carry them.
- **`schedule_statuses(ical)`** reads the server's per-attendee state
  after a write: `1.0` / `1.1` pending, `1.2` delivered, `3.x` / `5.x`
  failed. The weakest state wins.

  A scheduling server changes the stored object, so it sends no `ETag`
  with the `PUT` answer. `write_object` then reads the object back,
  which gives the core the server's version with the statuses.

### Rooms and resources

`SchedulingInfo::user_type` maps `calendar-user-type` to `AttendeeKind`
(`ROOM`, `RESOURCE`, `GROUP`, `INDIVIDUAL`, else `Unknown`), and the mock
offers room principals (`MockDavUser::room`). This is the placeholder for
the M3c room finder: Exchange room lists and rooms fill
`FreeBusySource::Exchange`. CalDAV has no standard room directory, and
principal-property-search is not used.

### Mock server

`MockDavConfig` gains these fields:

| Field | Default | Meaning |
| --- | --- | --- |
| `email` | `ada@example.org` | The main user's calendar-user address. Before, the address was `mailto:ada`. |
| `scheduling` | on | Inbox, outbox and free/busy POST; `calendar-schedule` in `DAV`. |
| `auto_schedule` | off | Existing M3a tests and the demo keep their behaviour. |
| `users` | none | More principals (`MockDavUser::new` / `::room`). Each has a Basic login, address, user type and a `personal` calendar. |

At runtime the server offers `add_user`, `set_auto_schedule`,
`set_free_busy_visible`, `deliver` (into an inbox), `inbox`,
`scheduled_messages`, and the path helpers `inbox_path`, `outbox_path`,
`user_principal_path` and `user_calendar_path`.

`current-user-principal` follows the login. `collection_paths()` leaves
out inboxes and outboxes. The recorded request has the
`Schedule-Reply` header.

**Free/busy POST** works only on the user's own outbox (`403`
otherwise).

- The request must be `text/calendar` (`415`), a `VFREEBUSY` with
  `METHOD:REQUEST` (`400` with `valid-scheduling-message`), and the
  `ORGANIZER` must be the user (`403` with `organizer-allowed`).
- Each recipient gets `2.0` and a reply built from their calendars:
  opaque, not cancelled, not declined; tentative or unanswered events
  are `BUSY-TENTATIVE`. Zoned times count as UTC and recurrences are
  not expanded, which is enough for tests.
- Unknown recipients get `3.7`, and users who hide their free/busy get
  `3.8`.

**Auto-scheduling**, when on:

- An organizer's `PUT` delivers a `REQUEST` to every attendee with
  `SCHEDULE-AGENT=SERVER`:
  - a local attendee gets it in their inbox, and in their default
    calendar while their own `PARTSTAT` is kept;
  - an external attendee is recorded as mailed (`1.1`).

  The server sets `SCHEDULE-STATUS` (`1.2` / `1.1`) and answers without
  an `ETag`. Attendees removed from the event get a `CANCEL`.
- An organizer's `DELETE` sends `CANCEL`s and marks the local copies
  `STATUS:CANCELLED`.
- An attendee's `PUT` with a changed `PARTSTAT` sends a `REPLY` to the
  organizer, updates the organizer's copy and sets `SCHEDULE-STATUS` on
  the `ORGANIZER`. An attendee's `DELETE` sends a declining `REPLY`.
  `Schedule-Reply: F` suppresses both.

Every message is recorded as a `MockScheduleMessage`.

## Consequences

- **Transport.** WP85 can tell server scheduling from iMIP per source
  (`SchedulingInfo::auto_schedule` and `server_schedules`), mark events
  `SCHEDULE-AGENT=CLIENT` when Nail mails, and show per-attendee
  delivery from `schedule_statuses`.
- **Free/busy.** `QueryFreeBusy` combines `own_busy_periods` for the
  user's identities (`FreeBusySource::Own`) with one `CalDavFreeBusy`
  per CalDAV source with an outbox. Attendees nobody answers for are
  `Unknown`.
- **Demo.** demo-multi can add a second principal (`MockDavConfig::users`)
  so the Playwright scenario "free/busy shows busy blocks of another demo
  account" has data. Turning on `auto_schedule` gives the server-scheduling
  flows.
- **Limits.**
  - Schedule-inbox processing is pull-only: the core polls the inbox
    with the source's sync. There is no push.
  - The mock does not expand recurrences for free/busy.
  - The client sends no `Originator` / `Recipient` headers, so very old
    pre-RFC servers may refuse the POST, which then shows as a per-query
    error.
