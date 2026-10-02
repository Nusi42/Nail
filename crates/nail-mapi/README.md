# nail-mapi

Nail's Simple MAPI provider (plan M4b WP111; contract ADR 0087, decisions
ADR 0088). Built as `nail_mapi.dll`, shipped as
`mapi\<arch>\NailMapi.dll` (x64, x86, ARM64).

- `src/windows/exports.rs` — the exported Simple MAPI functions.
- `src/windows/decode.rs`, `memory.rs`, `text.rs` — reading the caller's
  `MapiMessage(W)` without trusting any pointer.
- `src/forward.rs`, `src/staging.rs` — checks, private copies of the
  attachments, forwarding over a `Transport`.
- `src/windows/client.rs` — the named-pipe client (same-user server check,
  starting `Nail.exe`, timeouts).
- `src/windows/server.rs` — reference / fake pipe server (tests, harness).
- `src/registration.rs` — the registry values the installers write.
- `src/bin/harness.rs` — `nail-mapi-harness`: calls the DLL like a program
  (`send`, `documents`, `exports`), prints the `.reg` file (`registry`), and
  without arguments plays a fake `Nail.exe` on `NAIL_MAPI_PIPE`.

```sh
cargo test -p nail-mapi                                   # x64
cargo test -p nail-mapi --target i686-pc-windows-msvc     # x86 (runs under WOW64)
cargo check -p nail-mapi --target aarch64-pc-windows-msvc # ARM64 (linking needs the MSVC ARM64 tools)
```
