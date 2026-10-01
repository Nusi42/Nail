# ADR 0078: M3c review fixes — resumable listings, moves, streaming back-off, meeting answers, EWS zones

Status: accepted · 2026-10-02 · M3c review fixes

## Context

The reviews of M3c (ADR 0077) found: interrupted first listings turned old
mail into arrivals; a failing EWS streaming subscription caused back-to-back
full passes; messages moved by other clients were downloaded again and lost
their local state; any local edit of an answered meeting sent the answer
again and was never written; EWS writes pinned events (also series) to UTC;
a re-listing after an expired cursor merged too early or never; the EWS
paths had almost no integration tests; Autodiscover buffered whole bodies
and offered a Windows sign-in to every host of the domain.

## Decisions

### Full listings are resumable and merged once (store v13)

- A mail folder's listing in progress lives in
  `exchange_folders.full_sync_page` as `first:<page>` / `full:<page>`
  (`sync::Listing`); a PIM collection's in the new
  `exchange_pim_collections.full_sync_page`. The cursor stays `NULL` until
  the last page; the ids a listing saw are kept in `exchange_listed`
  (`Store::add_exchange_listed`), so a listing that took several passes
  still merges correctly (`Store::exchange_unlisted_messages`).
- `first` (never listed) brings no arrivals, also when it is interrupted
  (Graph 429, EWS `ErrorServerBusy`, network, quitting); `full` (after an
  expired cursor, or no cursor after a sync) does.
- The merge runs only after the last page; a `MAX_PAGES` break goes on in
  the next pass; an expired page of a resumed listing restarts it once; an
  expired cursor of a listing from the start is an error (no merge with an
  empty list — the PIM path used to remove every server item).

### Moves seen from other clients

- Removals are marked (`exchange_removed`), not deleted: after the due
  folders of a mailbox, if anything is marked, every other folder of that
  mailbox is synced in the same pass, then what is still marked is
  deleted (`flush_removed`). A removal of a message that is already in
  another folder is ignored.
- Graph asks for immutable ids on every request (`Prefer:
  IdType="ImmutableId"`, merged into the `Prefer` header, also in
  `$batch` parts): a moved message keeps its id and is moved locally by
  `apply_change`.
- EWS ids change on a move: a new message's `Message-ID` is matched first
  against marked messages of the same mailbox; outside a first listing, a
  stored message with the same `Message-ID` in another folder is a move
  when its old id is gone (`fetch_mime` answers not found) — copies stay
  copies. The moved message keeps its store id, tags, snooze and rule
  history; it is no arrival. A move by Nail into a folder whose listing
  runs is added to that listing.

### Streaming back-off

- When `wait_for_changes` fails, the loop records the Retry-After
  (throttle status and `PollTimer`), polls until the next pass and waits at
  least a back-off (reconnect min → max, reset by a successful wait).
- `StreamingWatcher` returns an error when a subscription is lost again
  within `RENEW_QUIET` (2 minutes) of its renewal instead of "every folder
  may have changed" once more (no wake-all loop behind load balancers
  without affinity). Affinity headers are not sent yet.

### Meeting answers

- `exchange_meeting_answers` keeps per meeting (collection + UID) the own
  answer as last synced and an answer queued by `RespondToInvitation`
  (comment sealed at rest, whether the organizer is told). The replayer
  answers through the meeting API only when the own status differs from
  the synced one or an answer is queued, with its comment and send flag;
  otherwise the edit (reminder, categories, notes, show-as) is a normal
  update with `MeetingSend::None`.
- Updates of a meeting the user does not organize leave the organizer's
  parts out: Graph `event_update_to_graph` (no `attendees`,
  `responseRequested`, `recurrence`), EWS `event_updates` (no attendee
  lists, `IsResponseRequested`, `Recurrence`).

### EWS time zones

- `nail_exchange::mapping::windows_zones`: CLDR `windowsZones` (territory
  001 plus common other cities) and, for other IANA zones, the Windows
  zone whose IANA zone has the same UTC offsets every hour of the event's
  year and the next. `exchange/pim.rs` `ews_zones` writes timed events and
  their recurrence in that Windows zone (the end converted to the start's
  zone) and all-day events in the user's zone; EWS then sends
  `StartTimeZone` / `EndTimeZone` / `TimeZoneContext`. A timed series whose
  zone has no Windows equivalent is refused (replay fails with a reason);
  a single event goes out in UTC. Tasks are still written in UTC.

### Autodiscover

- `ReqwestHttp` reads bodies in chunks and stops at `MAX_BODY`.
- `AutodiscoverRequest.login_hosts`: with `windows_sign_in`, the core passes
  the hosts whose `needs-login` prompt was shown for the address
  (`ExchangeCtl::remember_login_prompt`); another host asking to sign in
  gets its own prompt. TLS channel binding (`SECBUFFER_CHANNEL_BINDINGS`,
  tls-server-end-point) is **not** implemented: reqwest/rustls would have
  to hand the server certificate to the SSPI handshake on the same
  connection; it needs a live Exchange 2019 CU14+ / SE with Extended
  Protection to verify — a follow-up.

### Tests

- Store: `exchange_sync` unit tests (listings across passes, removals,
  PIM listing marker, meeting answers); schema v13.
- Core `tests/exchange.rs`: interrupted first listing (Graph, > 5 pages per
  call) without NewMail; interrupted re-listing that still removes a server
  deletion; moves by another client on Graph and EWS keeping the local
  tag; a refused streaming subscription without a pass loop; an edited
  answered meeting without a second answer (comment and send flag
  passed); EWS writes, sending, automatic replies, rule placement with
  folder ids, rooms, free/busy, calendar / contacts / tasks sync, a series
  with `StartTimeZone` W. Europe and an answer with its comment.
- Playwright: an EWS scenario (move, category, send, automatic replies,
  meeting answer); demo controls `ews-messages` (any folder, categories,
  start zone), `ews-settings`, `ews-meetings`.

## Consequences

- A removal costs one extra round of folder deltas for the mailbox in that
  pass (cheap when nothing changed). A moved EWS message is downloaded once
  more to read its `Message-ID` (no second copy is stored).
- Existing data: v13 adds tables and a column only.
