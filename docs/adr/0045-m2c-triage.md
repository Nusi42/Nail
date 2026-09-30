# ADR 0045 (M2c WP52): nail-triage — junk classifier, snooze scheduler, notification policy

Status: accepted · 2026-09-30 · M2c WP52 (number to be assigned by the integrator)

## Context

Plan M2c WP52 asks for a local Bayesian junk classifier per account, snooze
with an IMAP keyword plus a hidden folder and local wake timers, and a
new-mail notification policy with per-account / folder / rule settings and
quiet hours. ADR 0043 fixed the contract (`nail_api::triage`), the store
(v7: hashed junk tokens, `snoozed_messages`, notification overrides) and
reserved `crates/nail-triage` as **pure logic**. WP55 wires it into the core.

## Decisions

### Crate shape

No I/O, no store, no clock of its own:

- persistence goes through traits the core implements on `nail-store`:
  `classifier::JunkStore` (`token_counts`, `trained`, `train`, `untrain`)
  and `snooze::SnoozeStore` (`load`, `save`, `remove`, `rebind`); in-memory
  reference implementations (`MemoryJunkStore`, `MemorySnoozeStore`) carry
  the store's semantics and back the tests;
- time goes through `clock::Clock` (`SystemClock`, `ManualClock`), so tests
  and the Playwright scenarios control it;
- every write is returned as `ops::TriageOp` (`EnsureSnoozedFolder`,
  `AddKeyword`, `RemoveKeyword`, `Move` to a `FolderTarget`: a folder id, a
  role of an account, or the account's hidden Snoozed folder). The core maps
  them onto its operation queue. **There is no delete operation**; every
  planner is property-tested never to target Trash.

### Tokenizer (`tokenize`)

Selected headers (`From`, `Sender`, `Reply-To`, `Return-Path`, `To`, `Cc`,
`Subject`, `List-Id`, `X-Mailer`, `User-Agent`, `Content-Type`,
`X-Spam-Flag`, `X-Spam-Status`) are namespaced (`subject:cheap`, plus
`from:addr:…` / `from:domain:…`); body words use UAX #29 boundaries
(`unicode-segmentation`), lowercased, 3–24 characters (CJK / Thai from one
character), longer words `skip:<first>:<length bucket>`, numbers of more
than four digits `num:<digits>`; links give `url:host:`, `url:domain:`,
`url:path:`, `url:ip`, `url:insecure`. Distinct tokens in first-seen order,
at most 3,000 per message, at most 256 KiB of text. Callers pass decoded
headers and text (`nail-mime`). Property tests: well-formed tokens, no
duplicates, determinism, bounded length, concatenation = union, ASCII case
insensitivity.

### Classifier (`classifier`)

Robinson's `f(w) = (s·x + n·p(w)) / (s + n)` with `p(w)` from the ratios of
trained junk / not-junk messages containing the token (so unbalanced
training does not skew it), SpamBayes defaults `s = 0.45`, `x = 0.5`,
tokens with `|f − 0.5| < 0.1` ignored, at most 150 clues, combined with
Fisher's chi-square (`(S − H + 1) / 2`, log sums so long messages do not
underflow). Classes: **junk** at or above `JunkSettings::threshold`,
**not junk** at or below `ClassifierParams::ham_cutoff` (20, kept below the
threshold), **unsure** in between. Unsure mail is not moved; its verdict is
`junk: false` with the score. Nothing is scored before `min_training`
messages of each class.

Training: `train` returns a `TrainingReceipt` (previous class derived from
the store's `Trained` / `Retrained` / `Unchanged`), `undo` reverts it
(untrain, or train back as the previous class). `message_key` uses the
`Message-ID` (without angle brackets), else `local:<id>`, matching the store.

Plans: `plan_arrival` — the server's verdict first when
`honor_server_flags` (`$NotJunk` beats `$Junk` and the Junk folder; server
verdicts train the classifier, the classifier never trains itself), then the
classifier when `enabled`; junk gets `$Junk` and, with `move_to_junk`, a
move to the Junk role. `plan_mark` — keywords (`$Junk` / `$NotJunk`, only
changes), a move to Junk, or back to the Inbox when marked not junk inside
Junk.

### Snooze (`snooze`, `wheel`)

`SnoozeScheduler<S: SnoozeStore, C: Clock>` keeps the wake times in a
hierarchical **timer wheel** (six levels × 64 one-second slots, overflow
list beyond one 2³⁶ s block; `next_deadline` is exact and the core sleeps
until it; property-tested against a sorted-map model). `snooze` validates
(future, at most ten years), persists, and plans `EnsureSnoozedFolder` +
`$Snoozed` + move to the Snoozed folder — only for messages not already
snoozed (re-snoozing only changes the time and keeps the wake folder).
`poll` returns the due messages with `RemoveKeyword $Snoozed` and one move
per target: the wake folder, or the Inbox role when that folder is gone
(`FolderLookup`). `load` rebuilds the wheel at startup, so passed wake
times are due at the first poll (restart / offline). A store failure while
waking keeps the entries scheduled for the next poll. The clock going
backwards never wakes early. `rebind_by_message_id` follows a message to a
new local id after a resync.

### Notifications (`notify`, `localtime`)

`decide` implements ADR 0043's precedence — off → quiet hours → rule
`notify` → **junk (new: filed-as-junk mail does not notify unless a rule
says so)** → folder override → account override → `folders` (Inbox, or all
except Sent / Drafts / Trash / Junk) — and returns the deciding `Reason`.
`in_quiet_hours` evaluates on the local wall clock of any
`chrono::TimeZone` (the core passes `chrono::Local`); ranges crossing
midnight belong to the day they start on; malformed / empty ranges are never
quiet. DST behaviour is tested with a rule-based test zone (EU and US
rules): quiet hours end at local 07:00 on the short night, a range inside
the spring gap never happens, a repeated hour is quiet both times.
`localtime::resolve_local` moves gap times forward and takes the earlier of
repeated times; `days_later_at` gives "tomorrow 08:00" on the wall clock.

`NotificationBatcher` groups notifying arrivals per account and folder (tag
`new-mail:<account>:<folder>`), released 3 s after the last message of a
burst and at most 15 s after the first; titles / bodies are
`LocalizedMessage`s (`notification-new-mail-title` with `count` +
`account`, body with the newest message's `sender` + `subject` only when
previews are on). `snooze_woke` and `test_notification` build the other two
kinds. Every id is listed in `nail_triage::FLUENT_IDS` and checked against
`locales/en/core-triage.ftl` (new: `snooze-error-*`).

### Dependencies

Only what ADR 0043 declared (chrono, unicode-segmentation, nail-api,
thiserror) plus `proptest` 1.11 (MIT / Apache-2.0, already in the lock
through nail-search) as a dev-dependency. No time-zone database crate: the
core uses the OS zone through `chrono::Local`; tests use their own rule
zone.

## Consequences / follow-ups for WP55

- Implement `JunkStore` on `Store`; `untrain` needs a new store accessor
  (`Store::untrain_junk(account, message_key) -> bool`: delete the
  `junk_training` row and decrement its tokens), since v7 only retrains.
- Implement `SnoozeStore` on `Store` (`list_snoozed(None)`, `snooze`,
  `unsnooze`, `rebind_snooze`); run one `SnoozeScheduler` in the core with a
  task that sleeps until `next_wake()` (capped, e.g. one minute, to notice
  clock changes) and on `SnoozeMessages` / `UnsnoozeMessages`; map
  `TriageOp`s onto queue operations and hide the Snoozed folder.
- Classify on arrival (tokens from decoded headers + text), store verdicts,
  run `decide` + `NotificationBatcher` on `NewMail` with the rules'
  `flow.notify`, and emit `NotificationRequested`.
