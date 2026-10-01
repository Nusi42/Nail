# ADR 0064: M3a review fixes: bounded parsing and expansion, SSRF-safe subscriptions, logins stay on their origin, bounded reminders, live subscription settings

Status: accepted · 2026-10-01 · M3a review

## Context

The M3a reviews confirmed eight defects in the wave integrated by ADR 0063
(and three smaller gaps), most of them reachable from data a third party
controls (a subscribed feed, a shared CalDAV calendar, an imported or
attached `.ics` / `.vcf`):

- **Stack overflow.** `nail_ical::content::parse` had no nesting limit; the
  component tree it built is dropped and written recursively. 20,000 nested
  `BEGIN:` lines (180 KB) aborted the process on a 2 MB worker thread — and
  engines sync again on start, so a feed could crash Nail in a loop.
- **Unbounded recurrence work.** `expand::instances` walked every
  occurrence from `DTSTART` (up to `MAX_SCANNED` = 200,000 per series): a
  `FREQ=SECONDLY` series from 1970 cost ~105 ms per expansion and showed
  nothing; 1,000 of them (100 KB) took 105 s for one month view — inside the
  store's write transaction, freezing every store user (mail included).
  Reminder planning and edit-scope checks (`is_occurrence`, `end_before`)
  walked the same way, the latter without any cap. As a side effect an
  `HOURLY` series older than ~22.8 years showed no occurrences.
- **Quadratic parsing.** Every parsed item and override was deduplicated
  with a linear `retain` over everything read so far (also vCards; and each
  item's wrapper scanned all `VTIMEZONE`s, and unclosed components were
  named by scanning all issues): a 32 MB feed took minutes, on a runtime
  worker (`sync_ics` parsed outside `spawn_blocking`).
- **Credentials across origins.** `DavClient` followed redirects to any
  https host and attached the `Authorization` header to every hop, so a DAV
  endpoint (or any hop) could obtain the source's password, the linked mail
  account's password, or an OAuth access token.
- **SSRF in subscriptions.** Feeds were fetched with the plain HTTP client:
  private, link-local and loopback targets were reachable (`webcal://localhost`
  became plain `http://localhost` — reachable from any web page's `webcal:`
  link through the protocol handler), and the whole body was buffered
  before the size check.
- **Notification storm.** Every occurrence of a `MINUTELY` / `SECONDLY`
  series with a `VALARM` became a reminder (up to 10,000 per event), all due
  ones fired at once, one OS notification each.
- **Subscription changes ignored.** `alarms`, name and colour were applied
  only when the feed's calendar was created; the stored ETag made the next
  fetch a `304`, so items kept their alarms stripped and the calendar its
  reminders off. A changed URL still sent the old conditional headers.
- **Collected contacts invisible.** Recipients the Collected book gained
  from sent mail were neither added to the in-memory PIM index (contact
  search, `SearchPim`, MCP `search_contacts`) nor announced
  (`ContactsChanged`).
- Smaller: feeds that redirect could not be subscribed (redirects off, `3xx`
  reported as not found); the feed's `REFRESH-INTERVAL` / `X-PUBLISHED-TTL`
  was parsed but never used; no core test covered the `412` conflict path
  and `ResolvePimConflict`.

## Decision

- **Nesting limit.** `content::parse` keeps at most `MAX_DEPTH` = 32 levels
  (real data nests four); a deeper `BEGIN` and everything in it is skipped
  and reported (`ContentIssueKind::TooDeep`, shown as a malformed line).
  Every format parser goes through it.
- **Recurrence.**
  - `RuleIter::skip_before` / `Occurrences::skip_before` jump to the period
    before a target arithmetically (by `INTERVAL`, for every frequency;
    whole periods only, so `BYSETPOS` stays correct). Rules with a `COUNT`
    skip only when every period yields exactly one occurrence (no `BY*`
    parts, `DAILY` or finer, or `WEEKLY`), so the count stays exact.
    `instances`, `is_occurrence` and `end_before` use it (`end_before`
    counts only for `COUNT` rules, bounded by `MAX_SCANNED`).
  - `MINUTELY` / `SECONDLY` rules yield at most
    `recur::MAX_SUB_HOURLY_OCCURRENCES` = 2,000 occurrences from the start,
    whatever their `COUNT` / `UNTIL` say (applied in the engine, so stored
    data is unchanged and every caller is covered).
  - `expand::ScanBudget` (shared, `REQUEST_SCAN_BUDGET` = 1,000,000 scans):
    `ExpandOptions::budget`; the core's `Expander` holds one per request and
    reports `InstanceExpander::exhausted`; reminder planning shares one per
    pass.
  - `Store::ensure_instances` expands **outside** the connection lock and
    writes the rows in one short transaction afterwards, skipping an event
    whose stored data or window changed meanwhile; an exhausted expander's
    results are not cached (a later query retries).
- **Linear parsing.** UID and `RECURRENCE-ID` duplicates are found with hash
  maps (a later copy still replaces the earlier one; the order of the rest
  is unchanged), `VTIMEZONE`s are indexed by `TZID` once per calendar,
  unclosed components are named through a map. Feed parsing and item
  conversion run in `spawn_blocking` (also the subscription test).
- **DAV logins stay on their origin.** `DavClient::send` sends the login
  only to the scheme / host / port of the requested URL
  (`client::same_origin`), like curl and browsers. A redirect to another
  origin that asks for the login ends with the new
  `DavError::RedirectNeedsLogin(host)` (`ErrorCode::Config`, text
  `dav-redirect-login`): the user enters that server's address if they trust
  it. Discovery reports it before "no service".
- **SSRF-safe subscriptions** (`nail-core/src/pim/feed_http.rs`, the rules
  of `nail-proxy` / `plugin_http.rs`): `https:` only (any port); every DNS
  answer must be public (`nail_proxy::is_public_ip`), literals too; the
  connection is pinned to the checked addresses and the peer re-checked;
  redirects are followed by hand (at most 5), each hop checked, the feed's
  login only to the configured URL's origin; `Content-Length` over the
  limit is refused before reading, the stream is cut at the limit; no
  system proxy, cookies or `Referer`; 60 s for the whole fetch. Plain
  `http:` to loopback hosts only with `ServiceOptions::loopback_feeds`, set
  by `demo::demo_options` (tests, seeded demos with their mock servers).
  The subscription keeps its configured URL (a redirect's target is not
  stored); the conditional headers apply to the whole chain.
- **Bounded reminders.** Series repeating more often than hourly (not
  all-day) plan no reminders; `nail_triage::reminders::limit_plan` keeps
  per item the last `MAX_LATE_PER_ITEM` = 3 and the next
  `MAX_AHEAD_PER_ITEM` = 20 (the plan moves on hourly); more than
  `MAX_SINGLE_NOTIFICATIONS` = 3 notification-worthy reminders in one poll
  become one summary notification (`reminders_summary_notification`, tag
  `reminders`, texts `notification-reminders-*`); `ReminderDue` still goes
  out per reminder.
- **Live subscription settings.** `sync_ics` records what it applied from
  `accounts.kdl` (app setting `pim.feed-applied.<source>`: fetch URL, name,
  colour, alarms). On a difference it updates the calendar (name and colour
  when set and changed, reminders = `alarms` when changed — edits made in
  the app otherwise stay), drops the conditional headers when the URL or
  `alarms` changed, and for changed `alarms` marks the feed's items for
  rewriting (`Store::forget_server_versions`: unchanged server text would
  otherwise be skipped).
- **Refresh interval.** A subscription without `refresh=` follows the
  feed's `REFRESH-INTERVAL` / `X-PUBLISHED-TTL` (at least 15 minutes,
  `nail_ical::feed::refresh_minutes`), else 360; a configured value wins
  (5 and more). The engine remembers the feed's value across `304`s; after
  a restart it is known again with the next full fetch.
- **Collected contacts.** `record_recipients` indexes the contacts
  `collect_addresses` wrote and broadcasts `ContactsChanged` for the
  Collected book.

## Consequences

- Hostile calendar data costs bounded CPU, memory and stack; a hostile
  feed's series simply show fewer or no occurrences.
- A rare race (an event saved while its expansion runs) leaves that event
  out of one answer; the save's `EventsChanged` makes the UI ask again.
- Users with a `/.well-known/` redirect to another host must enter that
  host as the server address once; the error says which.
- Subscriptions to services on the local network or this computer are
  refused (`forbidden`) outside tests and seeded demos.
- Tests: `content` deep nesting on a 256 KB stack; skip-ahead equivalence
  for 20 rules × 6 targets, sub-hourly cap, far targets, shared budget;
  linear parsing of 60k items, duplicate order (iCalendar and vCard); store
  expansion outside the lock (stale / exhausted results not cached),
  `forget_server_versions`; `feed_http` (hop rules, private names and
  literals, redirects and login stripping, size limits, `304`); DAV login
  never follows a cross-origin redirect (client and discovery); triage
  `limit_plan` and the summary notification; core: sub-hourly reminders,
  `reminder_floods_are_bounded`, `changed_subscriptions_take_effect`,
  `loopback_subscriptions_are_refused_in_real_runs`,
  `conflicts_keep_the_server_version_and_resolve_both_ways`,
  `sent_mail_fills_the_collected_book_and_contact_search`, the refresh
  interval.
