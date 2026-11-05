# 0214 — Faster builds: compile jobs from free memory, e2e on the test build

Status: accepted (2026-11-05)

## Context

The 0.8.4 release took well over two hours on a 32-thread machine, almost all
of it compiling Rust:

- **Two compile jobs.** Builds ran with `CARGO_BUILD_JOBS=2`, set by hand
  after rustc failed with os error 1455 (the paging file was full) while
  other programs held much of the memory.
- **206 test binaries**, now one per crate (ADR 0213).
- **The CLI for e2e.** `cargo build -p nail-cli` after `cargo test --workspace`
  compiled again (about 3 minutes), because a build without tests resolves
  other dependency features.
- **The release build (about 65 minutes)** runs four cargo builds: the CLI,
  the MAPI DLL for x64 and for i686, and the app. Each one compiles
  `nail-api` in release, which takes about 7.5 minutes on its own: about
  375 s of its 448 s go to LLVM's crate-local ThinLTO.

Tried and rejected:

- **One release target directory** for the CLI, the DLLs and the app (all
  with a static C runtime). It saved nothing: cargo resolves features per
  invocation, so `serde`, `tokio`, `nail-api` and `nail-core` compiled again
  in every step anyway. Workspace-wide feature unification
  (`resolver.feature-unification`) is still unstable. The shared folder also
  let the app's `Nail.exe` overwrite the CLI's `nail.exe`: on Windows they
  are the same file name.
- **Codegen settings for `nail-api`.** `codegen-units = 64` took 521 s and
  `codegen-units = 1` more than 600 s. Turning ThinLTO off took 144 s, but
  `lto` is a profile-wide setting and would change the optimisation of the
  whole app.

## Decision

- **Jobs from free memory.** `ensureCargoJobs` (`scripts/lib/cargo.mjs`) sets
  `CARGO_BUILD_JOBS` when a build starts, unless it is set. It allows one job
  per 3 GiB of free memory after a 4 GiB reserve, with at least 2 jobs and
  at most one per hardware thread. Free memory is measured per OS:
  - Windows: the smaller of free physical memory and the free commit charge.
  - macOS: at least half of the total memory, because the OS counts the file
    cache as used.
  - Linux: `MemAvailable`.

  Called by `npm run check`, `release:prepare`, `build-installer.mjs` and
  `build:desktop`. A `CARGO_BUILD_JOBS` that is already set still wins.
- **The e2e CLI from the test build.** `cargo test --workspace` already builds
  `target/debug/nail`. The release checks therefore run `build:desktop` and
  `e2e:only` after the tests, instead of `npm run e2e`. Run on its own,
  `npm run e2e` still builds the CLI itself.

## Consequences

- Builds use the machine. With about 28 GiB free, cargo runs 8 jobs instead
  of 2; when memory is short, it still falls back to 2.
- The release build stays bound by `nail-api`, which compiles four times in
  release. Open options:
  - move what `nail-mapi` needs (the MAPI protocol, `WindowsArch`, the
    registry names) into a small crate, so the two DLL builds skip
    `nail-api`;
  - find the code that makes ThinLTO slow in `nail-api`.
- Linux and macOS use the same scripts; only the measured free memory
  differs per OS.
