# ADR 0186: Installer smoke test beside an existing installation

Status: accepted · 2026-10-08 · amends ADR 0089 (installers) for machines with Nail installed

## Context

`scripts/installer-smoke.ps1` installs the per-user NSIS setup silently into a temporary directory, starts it,
updates and uninstalls it. A per-user installation owns fixed HKCU keys (`Uninstall\Nail`,
`Software\Nail\Install`, `RegisteredApplications`, the ProgIDs, the PATH entry, the Run key), so the script
refuses to run where Nail is already installed — the maintainer's development machine, where every wave's installer
is built. Running the setup there would update, re-register or (on cleanup) uninstall the maintainer's installation.
Building a second product identity for smoke tests would still share most of those keys (the NSIS hooks name
`Nail` literally) and would test a different build from the one the maintainer gets.

## Decision

A second, read-only smoke test, `scripts/installer-payload-smoke.ps1`, runs beside an installation and never
runs the setup:

1. **The setup file**: name (version, architecture), size, PE header, NSIS marker, SHA-256 against the build's
   `SHA256SUMS`, identical to the bundler's output in `<target>/<triple>/release/bundle/nsis`, Authenticode state
   (reported; `-RequireSignature` makes an unsigned setup fail).
2. **Its payload**: the bundler's `installer.nsi` of that build names every installed file (the main binary and
   `File /a "/oname=<target>" "<source>"`). Every source must exist; the files are staged in a temporary
   directory in the installed layout (`Nail.exe`, `bin\nail.exe`, `ui\…`, `mapi\<arch>\NailMapi.dll`). This is
   the same `Nail.exe` the setup packs (marked with its installer kind), not the portable copy.
3. **The CLI**: `bin\nail.exe --version`.
4. **`nail serve --seed demo`** on the payload's `ui\` folder (port 0, temporary directories, `--no-mcp`): the URL,
   the page and one of its scripts load.
5. **The desktop app, seeded**: `Nail.exe --seed demo` with temporary directories. Seeded runs skip the
   single-instance hand-over (they never forward to the running Nail), use an in-memory registry, no Run key and no
   MAPI pipe, and keep their WebView2 profile in the data directory; `WEBVIEW2_USER_DATA_FOLDER` points into the
   temporary directory as well. Its WebView2 must load the built UI (remote debugging on a free loopback port) and
   the app must stay up; then only this process tree is stopped.

The script reads and writes no Nail registry key and never touches an installed Nail, its profile, WebView2 data
or processes. `installer-smoke.ps1` stays the full test for clean machines and CI.

## Consequences

- Not verified beside an installation: what only running the setup shows — the NSIS script's registry writes,
  shortcuts, PATH entry, file associations, the uninstaller and update in place. The maintainer's own install / upgrade
  of each wave's setup covers them; CI runs the full `installer-smoke.ps1` on a clean runner.
- The payload check depends on the build's `installer.nsi`: run it on the build directory that produced the setup,
  before the UI is rebuilt (hashed asset names would be missing).
