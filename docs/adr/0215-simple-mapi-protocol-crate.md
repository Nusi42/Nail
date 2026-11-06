# 0215 — The Simple MAPI protocol in its own crate

Status: accepted (2026-11-06)

## Context

`NailMapi.dll` (`crates/nail-mapi`) is built in release for x64 and x86 for
every installer. It depended on `nail-api` for the pipe protocol
(`nail_api::mapi`), `WindowsArch`, the install registry names
(`nail_api::desktop::registry`) and `MAX_ATTACHMENT_BYTES`. A release build
of `nail-api` takes about 7.5 minutes, mostly crate-local ThinLTO (ADR 0214),
and the two architectures cannot share it. So the two DLL builds cost about
15 minutes, for a few hundred lines of protocol.

## Decision

The crate `nail-mapi-protocol` holds what the DLL and the app share:

- the pipe protocol: `MapiMessage`, `MapiResult`, `MapiPipeRequest` /
  `MapiPipeResponse`, the frame functions, the limits, the pipe and file
  names, the MAPI client registry names (`registry`);
- `WindowsArch`, the install registry names (`install_registry`) and
  `MAX_ATTACHMENT_BYTES`.

It depends only on `serde`, `serde_json`, `thiserror` and `ts-rs`.
`nail-api` re-exports it unchanged: `nail_api::mapi` is
`pub use nail_mapi_protocol::*` plus `result_for_error` (the `ApiError` →
`MapiResult` mapping, which needs the contract);
`nail_api::desktop::WindowsArch`, `nail_api::desktop::registry` and
`nail_api::compose::MAX_ATTACHMENT_BYTES` are re-exports. Every fact keeps one
definition, and the rest of Nail keeps naming it through the contract. The
TypeScript contract is unchanged (`nail-api` exports the types as before).

`nail-mapi` depends on `nail-mapi-protocol` and `nail-util`, not on
`nail-api`.

## Consequences

- A cold release build of the DLL takes 18 s (x86) and 13 s (x64) instead of
  about 7.5 minutes each.
- `MapiResult::from_api_error` became `nail_api::mapi::result_for_error`
  (a type from another crate cannot get an inherent method).
- A new fact the DLL needs goes into `nail-mapi-protocol`, never into
  `nail-api` with a copy in the DLL.
