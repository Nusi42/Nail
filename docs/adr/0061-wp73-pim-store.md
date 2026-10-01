# ADR 0061: PIM store, instance cache, PIM search, reminder scheduler (WP73)

Status: accepted · 2026-10-01 · M3a WP73

## Context

WP73 (plan section 8) fills the slots of ADR 0058 in `nail-store`
(`src/pim.rs`), `nail-search` (`src/pim.rs`) and `nail-triage`
(`src/reminders.rs`): store APIs for every v10 PIM table, an instance
cache for range queries, PIM search, the reminder scheduler, the
Collected book, duplicate detection and merging, and address
autocomplete ranking. It runs in parallel with `nail-ical` (WP71) and
`nail-dav` (WP72), so none of the three crates depends on them, and it
adds no migration (schema v10 as given).

## Decisions

### The store never parses iCalendar / vCard

The core hands every item over twice: parsed (`nail_store::pim::PimItem`,
wrapping the `nail_api` types) and as the raw text it was parsed from or
written to (`nail-ical`). The store keeps both in step:
`pim_objects.raw` (sealed) and the parsed rows' `data` JSON (sealed) plus
their plain columns. The only vCard the store writes itself is the
four-line card of a Collected address (no parser needed).

Two links the parser knows only by `UID` are carried next to the item:
`PimItem::Task { parent_uid }` (`RELATED-TO;RELTYPE=PARENT`) and
`PimItem::Group { members }` (`MEMBER` URIs). When empty they are derived
from `fields.parent_id` / `member_ids` + `member_emails`. Reads resolve
them back to ids (`Task.fields.parent_id`, `subtask_ids`,
`ContactGroup.member_ids`, `Contact.group_ids`). `pim_group_members.member`
stores a normalized key (`member_key`: lower case, `urn:uuid:` dropped,
`mailto:` lower case) so lookups by contact `UID` or address are plain
equality.

### User writes and the PIM queue (`items.rs`, `ops.rs`)

* `save_pim_item` / `delete_pim_item` / `move_pim_item` change the tables
  and queue the server write in **one transaction**. Local-source items
  are `synced` and never queued; subscriptions and read-only collections
  refuse writes (`Error::Conflict`).
* Operations (`PimOperation`: `put`, `delete`, `create-collection`,
  `update-collection`, `delete-collection`) name the **row**, not the
  content: the replayer reads the current raw, URL and ETag when it runs.
  A pending upload of the same item is reused, so several edits upload
  once and a crash never replays stale content.
* A delete of an uploaded item hides it at once (parsed rows, instances,
  reminders go) and keeps the `pim_objects` row `deleted = 1` until the
  server confirmed; an item never uploaded is removed with its queued
  upload.
* `replay_pim_source` mirrors the mail queue: per-source order, a retry
  ends the pass, `in_flight` rows return to `pending` on open
  (`Store::open` now recovers both queues), `max_attempts`. A confirmed
  upload records URL and ETag and makes the item `synced` once nothing is
  queued for it. `PimReplayOutcome::Conflict` (412) fails the operation;
  the next sync decides.
* Moves are an upload to the new place plus a delete of the old URL, on
  each source's queue.

### Sync merge: server wins, the local version is kept

`apply_server_item(item, raw, href, etag)` finds the resource by URL,
else by `UID`, keeps the stored item id, and:

| Local state | Server | Result |
| --- | --- | --- |
| none | — | inserted |
| synced | same raw / ETag | unchanged |
| synced | changed | replaced |
| pending | ETag = the one the edit was based on | local edit kept (uploads next) |
| pending | changed | **conflict**: server version shown, local raw in `conflict_raw`, queued writes failed |
| deleted locally | changed | the server's version returns |
| conflict | changed | server version updated, copy kept |

`remove_server_item` deletes, except a locally edited item, which stays
as a conflict copy without URL. `resolve_conflict_keep_server` drops the
copy (or the item, when the server deleted it); "keep mine" is a
`save_pim_item` of the re-parsed copy. **Editing a conflicted item
resolves the conflict in favour of the edit.** `retain_server_uids` serves
ICS feeds fetched in full.

### Instance cache (`instances.rs`)

* `list_event_instances(from, to, calendars?, viewer_tz, &dyn
  InstanceExpander)`: the core passes `nail-ical`'s expander through the
  trait (the store cannot depend on WP71). Single events need no expander.
* Series are expanded **on demand in 90-day chunks**
  (`INSTANCE_CHUNK`); `pim_instance_windows` records the expanded range
  and only the missing part is expanded. One chunk before the requested
  range is always covered so occurrences up to 90 days long that started
  earlier still show (longer occurrences of a series that started before
  that are a known limit).
* Cache seconds (`time.rs`): instants for zoned times, the wall clock read
  as UTC for dates and floating times (`floating = 1`); reads widen by
  14 hours and place floating rows in the viewer's zone. **A change of the
  viewer's zone needs no re-expansion.**
* Invalidation: saving an event drops its rows and window; a calendar's
  zone change (`set_calendar_timezone`, `save_calendar`) drops the
  calendar; `note_zone_data(version)` drops everything when the zone
  database version changes (DST rules moved); `invalidate_instances` for
  explicit cases. Deletes cascade.
* Unknown zone names read as floating rather than failing. DST: gaps move
  forward, repeated times take the earlier instant (as `nail-triage`).
* `busy_intervals` + `free_slots` serve `FindFreeTime` (busy, not
  cancelled, not transparent occurrences, merged).

### Contacts (`contacts.rs`)

* `list_contacts` pages by sort name (family name first) and leaves out a
  contact linked to a primary that is listed too: a merged person shows
  once, a linked card in another (filtered) book still shows.
* `find_duplicate_contacts`: union of persons sharing an address, a phone
  number (last nine digits, at least seven) or an individual's name (word
  set, case and punctuation ignored); linked contacts count as their
  primary; the Collected book is left out unless asked for by id.
* `merge_contacts(merged, raw, others)` in one transaction: contacts of
  the primary's book are folded (deleted through the queue), others are
  **linked** and keep their cards; links pointing at a merged contact (or
  at the primary's former head) move to the primary. Groups that listed a
  folded contact are returned (`groups_to_update`) for the core to rewrite
  their `MEMBER`s with `nail-ical`. `merge_contact_fields` gives the merge
  dialog a union of the fields. `unlink_contact` undoes links.
* **Collected book**: `ensure_collected_book(name)` makes the local book
  `local-collected` (role `collected`, name from the caller:
  `pim-address-book-collected`); `collect_addresses` counts usage in the
  v4 `collected_addresses` table and keeps one card per address that no
  real book knows (renamed when a name arrives later).
* **Autocomplete** (`suggest_recipients(prefix, limit, now)`): contacts of
  books with `autocomplete` on, plus collected addresses; prefix of the
  address, the name or a word of it. Score = match (exact address 5,
  name start 4, address start 3, word start 2.5) + contact 4 (+0.5 for a
  preferred address) + frequency `2·ln(1+uses)` fading to half over
  ~90 days + recency `3·e^(−days/30)`; one entry per address, contact
  names preferred. Frequent recent use can outrank an unused contact, an
  old habit fades.

### Reminders (`nail-store` `reminders.rs`, `nail-triage` `reminders.rs`)

* Store: one row per alarm occurrence with a **derived id** (hash of
  item, occurrence, alarm key), so re-planning after restarts and edits
  finds the same rows: `replace_item_reminders` keeps the state when the
  fire time is unchanged and resets to pending when it moved;
  `set_reminder_state`, `list_reminders(until, local_tz)` (fired +
  snoozed + pending before `until`), `reminder_views`.
* `nail-triage::reminders` is pure: `plan_alarms(occurrences, local_tz,
  from, until)` computes fire times (relative to start / end — a task's
  start / due —, absolute once per series, `REPEAT` as `key+n`, alarms
  acknowledged at or after their time skipped). Zoned times are
  instants; **dates and floating times use the local zone passed in**, so
  a zone change is a re-plan.
* `ReminderScheduler<S: ReminderStore, C: Clock>` on the triage timer
  wheel and injectable clock: `load` (restart), `next_wake`, `poll`
  (marks fired; late ones beyond `ReminderPolicy::notify_late_up_to`,
  default 2 h, are listed but not notified), `snooze` (future, at most a
  year; `reminder-error-*`), `dismiss`, `replan`, `forget`, `reload`.
  Snooze / dismiss return `ReminderOp`s (RFC 9074 `ACKNOWLEDGED` /
  snooze alarm) for the core to write through the PIM queue.
  `reminder_notification` builds the `NotificationKind::Reminder`
  notification (`reminder:<id>`, `notification-reminder-*`).

### PIM search (`nail-search` `pim.rs`)

* A separate Tantivy index (`PimIndex`, directory `search-pim`, own
  schema version) — PIM changes never force a mail reindex; opened with
  the same `DirectoryWrapper` as the mail index, so it is **sealed at
  rest** when encryption is on; unreadable / other-version indexes are
  rebuilt (`was_rebuilt` → reindex from the store).
* Fields: title, names, emails, phones (also as digits), location, text;
  every query word must match a word start (type-ahead), titles and names
  rank first; digit queries match phone numbers by digits (leading zeros
  dropped). `search(query, kinds, collections, limit)`,
  `search_contacts`, and `contact_match` for `ContactMatch.field`.

## Consequences

* WP75 implements the trait adapters (`InstanceExpander` over
  `nail-ical`, `ReminderStore` over the store's reminder methods), opens
  `PimIndex` beside the mail index (also in the vault migration), calls
  `note_zone_data` at startup and re-plans reminders on item changes,
  window moves and zone changes.
* No schema change was needed; nothing for v11.
