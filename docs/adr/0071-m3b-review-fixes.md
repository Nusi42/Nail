# ADR 0071: M3b review fixes: no event takeover by UID, untrusted SENT-BY, uninvited answers, `send` for remote scheduling, Gmail arrivals, unknown server scheduling

Status: accepted · 2026-10-01 · M3b review

## Context

The M3b reviews confirmed eight defects in the wave integrated by ADR 0070
(and three smaller gaps). Four of them let a stranger change the user's
calendar through iTIP mail that Nail applies on arrival (the default
`SchedulingSettings` add invitations, apply updates and record replies):

- **Organizer takeover.** The user's copy of an event is found by `UID`
  alone. Every co-attendee knows the `UID`, and so does anyone who saw a
  forwarded invitation. Nothing compared the message's `ORGANIZER` with the
  copy's. The sender check only compared `From` with the organizer the
  message itself names. So Mallory could send a `REQUEST` from her own
  address that named herself as the organizer. It replaced time, title,
  location *and* organizer without a warning. After that, the real
  organizer's updates were flagged as spoofed.
- **PUBLISH / observer overwrite.** `PUBLISH` skips the sender check. The
  automatic "apply updates" clause had no method or role filter. The
  stored version was looked up under the *message's* organizer, so any
  other organizer compared against "no version" and always looked newer.
- **Self-certified `SENT-BY`.** A sender equal to the `SENT-BY` parameter of
  the same untrusted object passed the sender check, which defeated it.
- **Party crashers.** A `REPLY` from someone not on the event added them
  (`set_status` / delegation append unknown attendees). Every later
  `SendInvitations` / `CancelMeeting` mailed them the meeting.

Four more:

- Remote sessions with `calendar` but without `send` could mail iTIP
  messages, including `ForwardInvitation` of any event to any address,
  from the user's identities.
- Gmail queued new mail of *every* folder for arrival rules and iTIP
  processing (IMAP: Inbox and Other only). The user's own sent `REQUEST`
  came back via history and was applied onto the organizer's copy.
- While a CalDAV source's RFC 6638 scheduling was unknown (before the
  first pass, after a failed discovery, or after a failed `OPTIONS`, which
  was stored as "no server scheduling"), Nail mailed iMIP and wrote events
  that a scheduling server also delivered. Discovery was never repeated.
- `ProposeNewTime`, `RefreshInvitation`, `ForwardInvitation`, declining a
  counter and the junk exclusion had no functional core test.

## Decisions

### Card checks (`nail-itip/src/card.rs`, contract `ItipIssue`)

- Three new non-fatal issues (`core-scheduling.ftl`, shown as alerts like
  the spoofing ones):
  - `organizer-changed`: an organizer-side message (`REQUEST`, `ADD`,
    `CANCEL`, `DECLINECOUNTER`, `PUBLISH`) whose `ORGANIZER` differs from
    the stored copy's (`None` on one side counts as different; two of the
    user's own addresses count as the same);
  - `sent-on-behalf`: the sender matches only the `SENT-BY` that the message
    names (organizer or attendee side). The claim is unverifiable, so it is
    a warning instead of a pass;
  - `uninvited-attendee`: a `REPLY` / `COUNTER` / `REFRESH` from someone who
    is neither an attendee of the copy (series or occurrence) nor in an
    attendee's `DELEGATED-TO`.
- `ItipIssue::blocks_automatic()`: the sender checks, these three,
  `not-addressed-to-me` and the fatal issues. The card still offers its
  actions, so the user can apply such a message by hand.
- `apply::keep_personal` keeps the stored organizer, so even a manual
  update never changes who organizes the event.

### Arrival processing (`nail-core/src/scheduling.rs`)

- Nothing is applied when any issue `blocks_automatic`. `PUBLISH` is never
  applied on its own. Organizer messages are applied automatically only
  for the attendee role, answers only for the organizer role. The user's
  own `REQUEST` coming back is therefore never an "update".
- The stored organizer version (card and `record_version`) is keyed on the
  organizer of the user's copy (the message's only when there is no copy).

### Remote sessions (`nail-core/src/remote_access.rs`)

`check_remote_request` requires `send` for every scheduling request that
can mail:

- `RespondToInvitation` with `send: true`;
- `ProposeNewTime`, `RefreshInvitation`, `ForwardInvitation`;
- `SendInvitations`, `CancelMeeting`, `RespondToCounter`;
- `ApplyInvitation` of a `REFRESH` (it mails the current version).

The full-PIM grant these requests already need covers every identity's
account. `required_access` in `nail-remote-auth` stays `calendar`; its
comment points to the core check.

### Gmail (`nail-core/src/gmail.rs`)

- New mail counts as arriving only in folders where
  `sync::runs_arrival_rules` holds (Inbox, Other), shared with IMAP. Gmail
  has no Other folders, so archived-on-arrival mail (Gmail filters that
  skip the inbox) runs no rules, as with IMAP's All Mail.
- A resumed full listing now merges too. Before it stores the cursor, it
  asks for every message id (`GmailEngine::all_message_ids`: `messages.list`
  ids only, 500 per page, Spam / Trash included). If that fails, the next
  pass resumes from the last page again. Chat transcripts are in the id set
  but never stored, so the merge ignores them.

### Unknown server scheduling (`nail-core/src/pim/engine.rs`, `events.rs`; `nail-dav`)

- `scheduling_unknown(source)`: a CalDAV source with no stored scheduling
  answer. While it holds, `save_item` marks the organizer and every
  attendee of a meeting `SCHEDULE-AGENT=CLIENT`
  (`nail_itip::server::mark_client_scheduled`) before writing. Nail mails,
  and the server keeps out, now and after discovery (the delivery plan
  then reads `CLIENT`). We prefer this to refusing to send, because
  answering keeps working while the server is unreachable.
- `discover_scheduling` returns transient `OPTIONS` failures (network,
  timeout, TLS, 429/503, 5xx) as errors, so the answer stays unknown. Only
  a definite answer (e.g. `405`) means "no server scheduling".
- `StoredScheduling.asked_at`: the principal is asked again after a day
  (`SCHEDULING_RECHECK_SECS`, also when the clock moved back or the value
  predates this field). A failed re-check keeps the previous answer.

### UI

- `forms.tsx` `protocolChoices`: a Gmail API account offers only "Gmail
  API" (no conversion to IMAP / POP3 from the settings). IMAP and POP3
  accounts switch between each other as before.
- `AccountSettings` edits `GmailOptions` (shortest / longest check
  interval).
- The invitation card shows the new issues as alerts.

## Tests

- `nail-itip`: card tests for the three issues (Mallory's hijack, a
  `PUBLISH` over the event, a `SENT-BY` claim on both sides, an uninvited
  answer vs. a declared delegate). An apply test keeps the organizer.
  `nail-api`: `blocks_automatic`.
- `nail-core/tests/scheduling.rs` reproduces the review probes A–E
  (`hijacks_by_uid_are_never_applied`,
  `replies_from_uninvited_people_are_not_recorded`; both fail without the
  fix). New coverage: `attendees_propose_refresh_and_forward` (COUNTER /
  REFRESH / forwarded REQUEST on the mock SMTP server),
  `declined_counters_tell_the_attendee_and_keep_the_time`,
  `junk_invitations_are_never_applied` (server `$Junk`), and
  `unknown_server_scheduling_never_delivers_twice` (`OPTIONS` failing,
  auto-scheduling server: one iMIP mail, `SCHEDULE-AGENT=CLIENT` on the
  server, no server delivery).
- `tests/remote.rs`: `remote_scheduling_mail_needs_the_send_scope`.
- `tests/gmail.rs`: `only_inbox_mail_counts_as_arriving` (Sent / Trash
  invitations not applied) and
  `a_resumed_full_listing_still_removes_deleted_messages` (second listing
  page fails once). `nail-proto-gmail`: `all_message_ids`. `nail-dav`:
  `OPTIONS` 500 vs. 405. Engine unit tests for the re-check and the
  marking.
- Vitest: Gmail settings form. Playwright: `gmail.spec.ts` asserts the
  Gmail form, and `scheduling.spec.ts` gets a scenario where invitations
  go to the CalDAV calendar chosen in the invitation settings and the
  organizer's update changes the object on the mock CalDAV server
  (`dav-objects`).

## Consequences

- Legitimate delegates who send through Outlook's "on behalf of"
  `SENT-BY` now get a warning and are not applied on their own. They are
  one click away.
- Events written to a CalDAV server before its scheduling is known keep
  `SCHEDULE-AGENT=CLIENT`: Nail keeps mailing for those meetings even
  after the server turns out to schedule.
- Still deferred: an e2e run of the RFC 6638 auto-schedule path. It is
  covered by the core tests `a_server_that_schedules_delivers_and_nail_mails_nothing`
  and `unknown_server_scheduling_never_delivers_twice`.
