# 0204 — Plug-in budgets count the thread's CPU time

Status: accepted (2026-10-26)

## Context

ADR 0051 gives every plug-in call a CPU budget (and ADR 0079 an hourly CPU
quota), and `HostLimits` documents both as CPU time. The runtime measured
them with `Instant`, though: elapsed time. On a busy machine a plug-in's
thread waits for a processor, and that wait was charged as if the plug-in
had computed. A full `cargo test --workspace` run made a test's plug-in
(5,000 refused API calls in one turn) pass its 500 ms budget and stop as
`cpu-budget`; on a user's loaded PC the same would stop a well-behaved
plug-in and overcharge its hourly quota.

## Decision

- The turn budget and the quota meter use the CPU time of the plug-in's
  own thread (`nail-plugins` `cpu_clock`): `GetThreadTimes` on Windows
  (user + kernel time, counted in clock ticks of about 15.6 ms),
  `clock_gettime(CLOCK_THREAD_CPUTIME_ID)` on Linux and macOS. A turn is
  armed and checked by the interrupt handler on that thread, so the
  calling thread's clock is the plug-in's.
- Wall-clock limits stay wall clock: the activation and reply timeouts, a
  job run's and an MCP tool call's limits, and the stop of a plug-in that
  the host shuts down.
- `libc` (MIT OR Apache-2.0, already in the lock through other crates)
  becomes a direct dependency of `nail-plugins` on Unix; Windows uses the
  workspace's `windows-sys`.

## Consequences

- Load on the machine no longer stops plug-ins or uses up their quota.
- On Windows a turn's time is known in ticks: a budget can be exceeded by
  up to one tick before the interrupt sees it, and very short turns are
  metered as whole ticks or nothing (on average right, since the ticks go
  to whichever thread was running).
- A turn is synchronous JavaScript (it cannot wait for anything), so its
  CPU time and its run time differ only by the time it waited for a
  processor.
