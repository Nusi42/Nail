# ADR 0028: M1b review fixes: rule mail loops, draft autosave races, MCP grant leaks, undo

Status: accepted · 2026-09-29 · M1b review

## Context

The M1b review found that rules could send mail in an endless loop, that
an aborted autosave upload could leave stale drafts on the server, that
MCP answers revealed whether hidden accounts and folders hold matching
mail, that one stale message made a whole rule run impossible to undo, that
a sender could put links into the user's automatic reply, and that
redirects bypassed the outbox. This ADR records the fixes. ADRs 0024,
0025, 0026 and 0027 point here where a fix changes what they say.

## Decisions

### Rule mail is marked and never starts a loop

- `Draft` gains `automatic: AutomaticMail | undefined` (`forward`,
  `reply`, `redirect { message_id }`), set by the core for every message a
  rule sends and kept in the stored draft JSON. It is optional in the
  contract (`#[ts(optional)]`), so clients that never see rule drafts are
  unchanged.
- `nail-compose` stamps such mail with `Auto-Submitted: auto-replied`
  (reply) or `auto-generated` (forward, redirect), RFC 3834, and with
  `X-Nail-Loop: <sending address>` (`nail_api::LOOP_HEADER`).
- The outbox does not run on-send rules on the Sent copy of rule mail. An
  on-send "forward everything I send" rule therefore forwards the user's
  messages, not its own forwards.
- `nail-rules` refuses (`SkipReason::AutomaticMail`):
  - **forward and redirect** of rule mail (`X-Nail-Loop`), of mail an own
    account already redirected (`Resent-From` names an account address),
    and of bounces and automatic responses (`Auto-Submitted: auto-replied…`,
    `multipart/report`, `mailer-daemon` / `postmaster` senders). A forward
    to a dead address would otherwise forward its own bounce.
  - **reply**: everything above plus what it refused before (any
    `Auto-Submitted` other than `no`, bulk and list mail, no-reply senders).
- Forwarding mailing-list mail and automatic notifications stays allowed.
  The review suggested the full reply guard for forwards too; that would
  break the common "forward my list / notification mail to another
  address" rules. The loop cannot happen anyway: the forward is new mail
  with both marks, so it is not forwarded again when it comes back. For
  the same reason `Resent-*` headers from other people's redirects (for
  example a university address that redirects to the user) do not block a
  forward.
- `validate` still accepts forward / reply / redirect on `on-send` rules.
  With the skip above they cannot loop, and "forward what I send to my
  archive" is a real use. No per-sender rate limit is added.

### Redirects go through the outbox

- `rule_redirect` no longer talks SMTP itself. It queues a draft with
  `automatic: redirect { message_id }`, the recipients and the original's
  subject (for the outbox list). The original's bytes ride along as the
  draft's single attachment (`redirect.eml`, `message/rfc822`, its blob
  hash). That keeps the blob alive while the redirect waits, even when a
  later action of the same rule deletes the message.
- When the entry is sent, `Service::redirect_message` builds the message
  with `nail_compose::resent_message` (the original unchanged, preceded by
  `Resent-From/To/Date/Message-ID` and `X-Nail-Loop`). The envelope goes to
  the draft's recipients only (not the identity's automatic Bcc).
- Redirects thereby get retries, survive being offline, and get a Sent
  copy like every other message. Rule mail is saved without scheduling a
  server Drafts copy (`Service::store_draft(draft, upload: false)`).

### Automatic replies insert values as text

- `rule_reply` renders the template with
  `nail_compose::render_markdown_template`: placeholders become ASCII
  tokens, the template's Markdown is rendered and sanitized, then the
  tokens are replaced by the HTML-escaped values (line breaks folded) in
  one pass, and the result is sanitized again. A subject such as
  `[Verify](https://evil.example) <img src=…>` stays literal text.
- The reply is therefore an HTML-mode draft (template HTML above the HTML
  quote from `NewDraft`); its text part is derived from the HTML, so the
  plain-text part shows no Markdown escapes. The recipient is unchanged
  (`Reply-To`, else `From`): both headers are equally sender-controlled, so
  choosing one over the other would not stop a relay.

### Draft autosave uploads are never aborted once started

- `DraftUploads` keeps waiting uploads (with a generation number) and a
  per-draft async lock. A task sleeps for the autosave delay, then *claims*
  itself (removes its own entry, only if its generation is still current)
  and runs the upload under the draft's lock. `replace` and `cancel` abort
  only waiting tasks; a claimed upload always runs to the end.
- `cancel` (used by `SendDraft` and `DeleteDraft`) aborts a waiting
  upload, waits for a running one and returns the draft's lock guard. The
  caller holds it while it reads the draft's server copy and schedules or
  deletes the draft, so it always sees the finished upload.
- The upload writes the new local copy, the draft's pointer to it and the
  queued `AppendMessage` / `DeleteMessage` of the old copy in one blocking
  store call without an await in between (`Service::announce_enqueued`
  sends the events afterwards). It does nothing when the draft is gone or
  already in the outbox (`Store::is_draft_queued`).

### Undoing a rule run restores what it can

- `Store::undo_rule_run` does the undo in one transaction: it checks that
  the run is undoable, queues the inverse operations newest first, each in
  a savepoint, and marks the run `undone`. An inverse whose local apply
  fails with not found, conflict or invalid data (message deleted or
  expunged, already moved back by hand) is rolled back to its savepoint and
  skipped; the rest are still queued. A refused undo changes nothing.
- `Store::enqueue_all` and `enqueue` share `enqueue_in_tx`.

### MCP: hidden accounts and folders stay hidden

- `search_messages` with a restricted grant (not all accounts, or folders
  excluded) applies the grant to the hits **before** paging: it reads the
  unrestricted results in pages of 200 (at most 5000 hits), keeps the
  readable ones, and pages over those. `offset`, `limit` and `next_offset`
  count readable messages only; there is no `hidden` count and no total.
  The query language can name any account or folder (`account:`,
  `folder:`), so filtering the query itself would not be enough. An
  unrestricted grant keeps the index's paging and total.
- `get_thread` / `nail://threads/{id}` no longer report `hidden`. A
  restricted grant gets subject and participants from the visible messages.
- The UI context shows `account_id`, `folder_id` and `thread_id` only when
  the grant covers them (the thread: when one of its messages is readable).
  The open draft is shown only when its identity's account is granted and
  the message it replies to, forwards or redirects is readable; otherwise
  `{id, hidden: true}`. `McpData` gains `identities()` for this.

### Smaller fixes

- **MCP audit:** refused connections write at most 20 audit entries a
  minute; the rest are counted and summed up in one entry in the next
  window. The audited path is cut to 64 bytes (except the known
  endpoints). Tool calls with malformed arguments and unknown tools are
  audited (outcome `error`).
- **`nail serve` never opens an editor.** The UI may run in a browser on
  another machine; `OpenConfigFile` answers `unsupported` there, as the
  user docs and ADR 0026 say. The desktop app still opens one (unless
  seeded).
- **CI** builds the desktop app (`npm run build:desktop`) and runs the
  WebView2 smoke test (`NAIL_E2E_DESKTOP=1`).
- **Tests** for the core's MCP event forwarding: a subscription through
  the core-started server gets an update after new mail and after
  `ReportUiContext`.

## Deferred (recorded in plan.md §6, "M1b follow-ups")

- Editing UI for toolbar order, key bindings and column order. The
  write-back helpers exist (ADR 0026); split sizes are the only UI edit
  wired so far.
- The other deviations the ADRs record: loopback HTTP instead of a named
  pipe for `nail mcp` (ADR 0025), no headless start of `nail mcp`,
  `notify` / `suppress` have no effect yet, "create rule from message"
  from the summary without `List-Id`, and a received `text/markdown`
  alternative listed as an attachment (ADR 0027).

## Consequences

- Rules cannot loop by themselves or with another Nail; the marks also
  tell other RFC 3834 aware software that the mail is automatic.
- A restricted MCP search may need several index reads; it stops after
  5000 hits, so a very broad query over mostly hidden mail can end without
  `next_offset` although more readable mail exists further down.
- Rule-sent drafts carry `automatic`; if the user edits a failed one in
  the composer, the UI's copy drops it, and the edited draft is sent as
  ordinary mail.
