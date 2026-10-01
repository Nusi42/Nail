# scripts/

Build and release helpers (plan M2b WP44, ADR 0040). None of them writes
build output into the repository (it lives in OneDrive): output goes to
`%LOCALAPPDATA%\nail-build\` or `NAIL_BUILD_DIR`.

| Script | What |
| --- | --- |
| `build-installer.mjs` | NSIS installer, portable zip, SBOMs, checksums → `<build dir>/dist` (Windows) |
| `installer-smoke.ps1` | install → launch → uninstall of that installer in a temp directory, per user |
| `sbom.mjs` | CycloneDX SBOMs only (Rust crates from `cargo metadata`, UI packages from `package-lock.json`) |
| `check-socket-binds.mjs` | fails on sockets bound to unspecified / non-loopback addresses outside the allowlist in `lib/socket-binds.mjs` (ADR 0057; first step of `npm test`) |
| `lib/*.mjs` | the scripts' pure helpers; tests in `test/` |
| `interop/` | engine tests against real Dovecot / Stalwart servers (own crate; see its README) |

```sh
node scripts/build-installer.mjs              # full build (UI, CLI, app, bundle)
node scripts/build-installer.mjs --skip-ui    # reuse ui/dist
powershell -File scripts/installer-smoke.ps1  # newest installer in <build dir>/dist
node scripts/sbom.mjs --out <dir>
node --test "scripts/test/*.test.mjs"         # tests of the helpers
cargo deny check                              # deny.toml at the repository root
```

`build-installer.mjs` options: `--skip-ui`, `--no-cli` (installer without
`bin\nail.exe`), `--target-dir DIR` (default `<build dir>/installer-target`),
`--out DIR` (default `<build dir>/dist`). The first run lets the Tauri
bundler download NSIS and the WebView2 bootstrapper to `%LOCALAPPDATA%\tauri`.

`installer-smoke.ps1` refuses to run when Nail is installed for the current
user. It writes only per-user registry keys, removes them again, and keeps
the app's data inside its temporary directory (portable marker).
