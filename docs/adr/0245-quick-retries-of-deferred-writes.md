# 0245 — Quick retries of deferred writes

Status: accepted (2026-11-23)

## Context

A retryable failure while replaying the operation queue (a timeout, a busy
server, a write that waits for another account's earlier one) ends the
account's pass with its writes still queued (ADR 0230). Nothing retried
them before the account's next poll or the user's next action, so a large
bulk action could sit for minutes after a single slow answer, and tests
that drain a large queue stalled under load.

## Decision

- A pass that defers writes schedules one quick retry of the account's
  queue (`crates/nail-core/src/replay_retry.rs`, called from
  `replay::replay_account`, so every engine gets it).
- The waits reuse `nail_net::http::RetryPolicy`: they double from
  `EngineTiming::replay_retry` (2 s; 100 ms in `EngineTiming::fast`) up to
  one minute, with up to a quarter of jitter, five times
  (`QUICK_RETRIES`). A server's `Retry-After` replaces the wait up to five
  minutes; after a longer one the account waits for its next poll.
- At most one retry waits per account. A pass that defers nothing resets
  the count and drops a waiting retry; `Service::shutdown` drops them all.
  After five quick retries the account waits for its polls as before.
- The quick retries come on top of the queue's attempt limit
  (`ReplayPolicy::max_attempts` + `QUICK_RETRIES`), so an operation is not
  given up sooner than before.
- Permanent failures (rejected credentials, a missing folder) still fail
  at once.
- The values are fixed: there is no user setting. Users cannot judge them,
  and a wrong value either hammers the server or leaves the queue stuck.

## Consequences

- Deferred writes usually go through within seconds instead of waiting for
  the next poll.
- A retry asks the engine to replay (`Service::request_replay`); an IMAP
  session that is reconnecting replays after it connects, as before.
