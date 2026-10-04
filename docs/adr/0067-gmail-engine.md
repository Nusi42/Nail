# ADR 0067: Gmail API engine and mock Gmail server (WP82)

Status: accepted · 2026-10-01 · M3b WP82 (the integrator renumbers)

## Context

Plan M3b WP82: `nail-proto-gmail` syncs Gmail accounts through the Gmail
REST API (`users.messages`, `users.history`, `users.labels`,
`users.threads`, batch requests) against the contract of ADR 0065
(`nail_api::gmail`, `MailApiEngine`, `OutgoingTransport`, `AdaptivePoll`,
`EngineError::retry_after_secs`). No push (Pub/Sub needs a server), no
non-loopback sockets in tests, no new third-party crates beyond the
workspace.

## Decisions

### Client (`client.rs`, `error.rs`, `wire.rs`)

- `GmailClient` on the workspace reqwest with nail-auth's rustls/ring
  configuration, no redirects. URL policy: `https:`, or `http:` to a
  loopback host only (`localhost`, 127/8, `::1`) — the mock server and a
  `gmail` node with `tls="plaintext"` on loopback.
- Auth: `GmailAuth::OAuth2 {tokens: Arc<dyn AccessTokenProvider>,
  provider, secret}` (the core's token manager; scope
  `https://mail.google.com/`) with **one forced refresh on `401`**, or a
  fixed bearer for tests. A provider error is passed through unchanged
  (`GmailError::Token`).
- **Backoff** (`RetryPolicy`, default 4 retries, 0.5 s doubling to 16 s with
  up to 25 % jitter): `429`, `403` with reason `rateLimitExceeded` /
  `userRateLimitExceeded` / `dailyLimitExceeded` / `quotaExceeded`, and
  `5xx` (plus broken connections) are retried inline, honouring
  `Retry-After` (seconds or HTTP date). A wait longer than
  `max_inline_wait` (30 s) is not slept: the error goes up as
  `ErrorCode::RateLimited` / `Unavailable` with
  `EngineError::retry_after_secs`, so the core's poll loop
  (`AdaptivePoll::failed`) waits instead of holding a task.
  **Non-idempotent calls** (`send`, `insert`, `drafts.create`,
  `labels.create`) are retried only on rate limits (refused before
  processing), never after `5xx` or a broken connection (they may have
  been stored).
- Errors: `GmailError` → `ErrorCode` + retryable + `gmail-*` Fluent ids
  (`locales/en/core-gmail.ftl`, 19 ids, `MESSAGE_IDS` checked against the
  catalog): insufficient scope (`403 insufficientPermissions` /
  `ACCESS_TOKEN_SCOPE_INSUFFICIENT`) is `auth_failed` with its own text; a
  label colour outside Gmail's palette is `gmail-label-color-rejected`;
  `409` is `conflict`; moves Gmail has no equivalent for are
  `unsupported` (`gmail-move-unsupported`).
- JSON resources accept 64-bit numbers as strings or numbers; base64url
  decoding tolerates padding, the standard alphabet and line breaks.

### API coverage

`messages.list` (`q`, `labelIds`, `includeSpamTrash`, paging), `get`
(`minimal` / `metadata` / `full` / `raw`), `modify`, `batchModify` (1000
ids per call), `trash`, `untrash`, `delete`, `insert`
(`internalDateSource=dateHeader`), `send`, `attachments.get`,
`drafts.create`; **media upload** (`/upload/…?uploadType=multipart`,
`multipart/related`) for raw messages above 4 MB; `history.list` (all four
history types, paging); `labels.list` / `get` / `create` / `patch`
(explicit `null` clears the colour) / `delete`; `threads.list` / `get` /
`modify`; **HTTP batch** (`POST /batch/gmail/v1`, `multipart/mixed`, 50
parts per batch by default, at most 100): parts are matched by
`Content-ID`, rate-limited / failing parts are retried in a later batch
with backoff, `404` parts are reported per id. Ids that are not plain
`[A-Za-z0-9_-]` never go into a batch path. Attachments are part of the
raw MIME (`fetch_raw`); `attachments.get` serves `format=full` views.

### Engine (`engine.rs`, `history.rs`, `mapping.rs`, `labels.rs`)

- `GmailEngine: MailApiEngine + OutgoingTransport`;
  `GmailEngine::for_account(&AccountConfig, tokens)` builds it from a
  `gmail` node (`api_base_for`, OAuth2 secret reference of the account;
  anything else is `GmailError::Config`).
- **Folders**: the six `GmailFolder`s; `RemoteFolder.name` is the stable
  path (`INBOX`, `ARCHIVE`, …) — the UI names folders by role.
- **Full sync**: `users.getProfile` first (its `historyId` is the first
  page's cursor), then `messages.list` with `includeSpamTrash=true` and
  `after:<unix>` for "recent N days", metadata via batch
  `format=minimal`. `RemoteMessage.raw` stays `None` (the core fetches raw
  per its body policy). Chat transcripts (`CHAT`) are not synced. A
  message deleted between list and get is skipped; any other per-message
  failure fails the page (the core keeps its state and retries).
- **Incremental sync**: `history.list` from the cursor; `404` (and a
  cursor that is not a number) → `DeltaOutcome::CursorExpired`. Records
  are folded (`fold_history`): added-then-deleted = removed only; label
  changes of a new message do not make it "changed"; a changed message
  takes the labels of its last record (Gmail includes them), or the
  earlier labels plus the record's diff, or a `minimal` fetch. New
  messages are fetched in batches. At most `history_pages_per_call` (10 ×
  500 records) per call: then the cursor is the last record's id and
  `SyncDelta.more` is set.
- **Mapping**: folder by `GmailFolder::for_labels`; flags `UNREAD` /
  `STARRED` / `DRAFT` (Gmail has no `\Answered`: always false — the core
  must not let a Gmail sync clear a local `\Answered`); tags = names of
  user labels (`LabelTable`, refreshed once when unknown label ids show
  up); `GmailMessageInfo` with thread id (the message's own id when
  missing) and category.
- **Writes** (`RemoteWrite`, idempotent): `SetFlags` →
  `LabelChange::from_flags` + user labels for tags (a new tag creates the
  label; names Gmail refuses are skipped with a warning; unknown tags to
  remove are ignored; an empty change sends nothing); `Move` →
  `move_change`, using `trash` into Trash and `untrash` out of it;
  `Trash` / `Delete`; a missing message is `WriteOutcome::Gone`;
  `Append` first searches `rfc822msgid:` (a replayed append finds its
  copy), then `drafts.create` for Drafts or `insert` with the folder label +
  `UNREAD` / `STARRED`, filed into the thread of its `In-Reply-To` /
  `References` when that is in the mailbox.
- **Sending** (`OutgoingTransport::send`, `send_raw`): `messages.send`
  with the raw MIME. Gmail takes the recipients from the headers, so
  envelope recipients missing from `To` / `Cc` / `Bcc` are added as a
  `Bcc` header (Gmail removes it); addresses with spaces, controls or
  separators are refused (no header injection). Replies are sent with the
  original's `threadId`. Gmail files the message in Sent itself (the
  core must not append a Sent copy; `IncomingServer::sends_itself`).
  `SendReceipt.response` = `gmail id <id> thread <thread>`.
- **Labels**: `labels()` (system first, then by name), `label(id)` with
  counts, `create_label` (a `409` on replay returns the existing label of
  that name), `update_label`, `delete_label` (`404` = done). The core's
  label queue (`gmail_label_ops`) calls these and stores the results.
  `LABEL_PALETTE` / `is_palette_color` list Gmail's label colours.
- **Polling** (`poll.rs`): `PollTimer` wraps `AdaptivePoll` with the
  account's `poll_range()`: changes or a resync → minimum interval,
  nothing → doubling to the maximum, errors → their `Retry-After` once,
  `more` → poll again at once.

### Mock Gmail server (feature `testing`)

`testing::MockGmailServer` binds 127.0.0.1:0 (ADR 0057) and serves an
in-memory `Mailbox` under `/gmail/v1/users/me`, `/upload/gmail/v1/…` and
`/batch/gmail/v1`: profile, messages (search terms `after:` / `before:`
(seconds or `yyyy/mm/dd`), `rfc822msgid:`, `in:`, `label:`, `is:`,
`category:`; unknown terms are `400` so engine bugs show), every `get`
format, modify / batchModify / batchDelete / trash / untrash / delete,
insert / import / send (JSON or multipart / media upload; send needs a
recipient header and drops `Bcc` from the stored copy), attachments,
drafts.create, history (types and label filters, paging, expiry → `404`),
labels (name conflicts `409`, palette check `400`, system labels
read-only, delete removes the label from messages with history), threads
(list / get / modify / trash / untrash / delete) and batch requests.
Threads follow `threadId` or `In-Reply-To` / `References`.
Test controls: `set_token` (bearer check, `401` with the recorded body),
`inject(InjectedError)` (recorded `429` / `403` rate / `500` / `403`
scope / `404` bodies, `Retry-After`, path / method filter, count — also
inside batches), `set_quota` (Gmail quota units: get / list / modify 5,
send 100, history 2, labels 1, …), a request log (method, path, query,
in batch, token), mutation helpers (`deliver`, `modify`, `delete`,
`create_label`, `rename_label`, `expire_history`, `with_mailbox`) and
inspection (`message`, `labels_of`, `labels`, `history_id`, `sent`).

Fixtures (`crates/nail-proto-gmail/fixtures/`): `mailbox-demo.json`
(`ada@gmail.example`, history id 4200: inbox thread with a sent reply,
the five categories, an archived starred message with a user label and a
PDF, a receipt stored as recorded base64url `raw`, spam, trash, a draft,
a chat) — `testing::fixtures::DEMO_MAILBOX` for core tests and the
demo-multi seed — and `responses/` with recorded API answers (labels,
messages, history, a batch answer, error bodies) that the unit tests
parse and the mock answers with.

The plan placed the mock in `nail-testkit-net`; ADR 0065 moved it into
this crate behind `testing` (no dependency of testkit on the engine's
wire types). Unchanged here.

### Dependencies

`mail-parser` (workspace, Apache-2.0 OR MIT) added to the crate for
`Message-ID` / `In-Reply-To` / recipients of raw messages; everything else
was already declared (reqwest, nail-auth, serde_json, base64, chrono, url,
axum for `testing`). No new third-party crate in the workspace, no
OpenSSL / aws-lc.

## Consequences

- WP85 builds the Gmail sync loop on `GmailEngine` (`full_sync_page`
  until `next_page` is `None`, store the first page's cursor, then
  `changes` with `PollTimer`), replays queued ops as `RemoteWrite`s,
  runs the label queue through `create_label` / `update_label` /
  `delete_label`, sends through the engine as the account's
  `OutgoingTransport`, and uses `profile()` for `TestAccount`.
- Not done here: Gmail `drafts.update` / `drafts.send` (drafts are
  appended as new drafts), `messages.import` with spam classification,
  `watch` / Pub/Sub (by design), per-user concurrency limiting across
  engines (one engine per account keeps requests sequential).
