# 0213 — One integration-test binary per crate

Status: accepted (2026-11-05)

## Context

Cargo builds every top-level file in a crate's `tests/` directory as a test
binary of its own, and each one links the crate and all its dependencies.
`cargo test --workspace` linked 206 binaries: `nail-core` alone had 33
integration-test files, `nail-config` 25, `nail-store` 16. Linking
dominated the build time of the tests, and parallel links of large binaries
ran the machine out of memory (paging file, os error 1455). `nail-pgp` and
`nail-proto-imap` already used one binary (`tests/integration/main.rs`) so
that the shared helpers are checked for dead code.

## Decision

- **A crate has at most one integration-test binary.** With two or more test
  files they are modules of `tests/integration/main.rs`: `#![cfg(test)]`
  once, `mod common;` first, then one `mod` per former file. Shared helpers
  live in `tests/integration/common/` and are used as `crate::common::…`.
- A Windows-only file keeps its own `#![cfg(windows)]` as an inner attribute
  of the module. Paths in `include_str!` / `include_bytes!` and the insta
  snapshots (`tests/integration/snapshots/`, now prefixed `integration__`)
  moved with the files; fixtures and golden files stayed where they were.
- One file's tests run with `cargo test -p <crate> --test integration
  <file>::`; the regeneration commands in `docs/dev-setup.md` say so.
- All tests of a crate now share one process. Tests that change something
  process-wide (environment variables, the current directory, global state
  of the crate) need a shared lock in `common`; none of the merged tests
  did.
- Helpers in `common` that no module uses any more are deleted; the former
  per-file `#[expect(dead_code)] mod common;` is gone.
- **Except `nail-core`**, which keeps one binary per file for now. Merged,
  its 280 tests ran up to 32 at once in one process, each starting a whole
  service. Under that load three timing-dependent tests failed in 3 of 7
  Linux runs (WSL), each time a different one:
  `security::autocrypt_enables_encryption_after_one_message`,
  `pim_sync::conflicts_keep_the_server_version_and_resolve_both_ways` and
  `m2c_wired::rules_and_vacation_run_on_the_sieve_server`. The last one
  points to a real race: a forced `SyncSieve` can answer `pending` when an
  earlier upload ends first. Per-file binaries passed in 8 of 8 runs. The
  merge waits until those races are fixed.
- Single-file crates keep `tests/<name>.rs`. `scripts/interop` is not a
  member of the workspace and runs only in the interop job; it keeps its two
  files.

## Consequences

The workspace links 112 test binaries instead of 206 (80 once `nail-core`
is merged too), with the same 4091 tests. A test that needs its own process (`harness = false`, a required
feature, process-wide state that a lock cannot guard) gets its own
`tests/<name>.rs` and a note why.
