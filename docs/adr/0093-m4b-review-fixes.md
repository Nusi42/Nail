# ADR 0093: M4b review fixes — protected MAPI copy, test-only overrides, staging root, scoped update key

Status: accepted · 2026-10-02 · M4b review (amends ADRs 0088, 0089, 0090, 0091)

## Context

The M4b reviews found six must-fix problems and three smaller ones. Most
share one theme: code or data that a normal user (or any program the user
runs unelevated) controls ended up trusted by something with more rights —
an elevated program, another user, the administrator's policy, or the
auto-updater's root of trust.

## Decisions

### Simple MAPI from the per-user installer: a protected copy (amends 0089)

The NSIS MAPI option wrote `HKLM\Software\Clients\Mail\Nail` with
`DLLPath` / `DLLPathEx` = `$INSTDIR\mapi\<arch>\NailMapi.dll`, a file in the
user's profile. Every program that uses Simple MAPI loads the registered
DLL: elevated programs of the same user (an elevation path for malware
that already runs as the user) and other users of the PC (whoever
installed last ran code as everyone else). The `.reg` file the elevated
`reg.exe import` read lived in `%TEMP%`, open to a swap between writing and
importing.

- The elevated step is now **one `cmd.exe /d /v:off /s /c "…"`** whose whole
  work is on its command line, fixed when the user consents: create
  `%ProgramFiles%\Nail MAPI\mapi\<arch>` and `…\mapi\x86` (administrators
  only), copy the DLLs there from `$INSTDIR`, then `reg add` the client's
  name, `DLLPath` and `DLLPathEx` for both views (`/reg:64`, `/reg:32`)
  pointing at the **copy**. Nothing in `%TEMP%` is read elevated.
- HKLM gets **no `shell\open\command`** from the per-user installer (it
  would name the per-user `Nail.exe`). The DLL finds Nail through
  `app_exe_from_dll` (`…\Nail MAPI\Nail.exe`, absent) and then the calling
  user's own `HKCU\Software\Nail\Install\InstallDir`, so each user starts
  their own Nail.
- Every path is quoted; an installation folder with `%` (cmd.exe would
  expand it) or longer than `NAIL_MAPI_MAX_DIR` (110; the command must fit
  `NSIS_MAX_STRLEN` = 1024 of the Tauri NSIS build) is refused with a
  detail line. A test expands the command with a 110-character folder and
  checks the length; the command was also run unelevated against an HKCU
  key and a temp folder with `&`, spaces and parentheses in the paths.
- Ownership for removal: the protected copy, or (earlier dev builds) this
  installation's own DLL. Removal is another fixed command (`reg delete`
  both views, `rd` the copy). Per-user installs of several users share the
  one machine registration (as before: last writer wins, but now every
  writer points at the same protected copy); uninstalling one removes it.
- Known limits: the copy is made from `$INSTDIR` at consent time, so
  malware that already replaced the user's files before the prompt is
  copied too (the same trust the user gives the installer itself; signing
  the DLLs narrows it). Updates (`/UPDATE`) do not refresh the copy; the
  pipe protocol is versioned (`MAPI_PROTOCOL_VERSION`) and an old DLL gets
  `MAPI_E_FAILURE` from a newer app rather than undefined behaviour. A DLL
  that a running program has loaded cannot be replaced or removed until
  that program exits.

### Test-only environment overrides are compiled out of shipped builds (amends 0088, 0090)

- `nail-mapi` honoured `NAIL_MAPI_PIPE` / `NAIL_MAPI_APP` from the host
  process: any program could be started from inside an elevated caller.
  `nail-app` honoured `NAIL_REGISTRY_ROOT` (the policy and install facts
  read from a user-controlled key) and `NAIL_UPDATE_ENDPOINT` /
  `NAIL_UPDATE_PUBKEY` (a loopback endpoint with any key).
- Both crates get a **`test-overrides` cargo feature**. Only with it are the
  variables read (`nail_mapi::windows::client::TEST_OVERRIDES`,
  `nail_app_lib::shell::test_env`); without it a set variable is logged and
  ignored. `e2e/scripts/build-desktop.mjs` enables both; `nail-mapi`'s own
  tests enable it through a dev-dependency on the crate itself (never part
  of `cargo build -p nail-mapi`); `build-installer.mjs` and the `mapi` CI
  job's release builds never do (script test).
- **Install facts: HKLM first.** When both hives name the running copy's
  folder, the machine's facts win, so a forged `HKCU\Software\Nail\Install`
  cannot turn an MSI install (`DisableUpdates`) into a self-updating one.

### Elevated callers and the pipe's integrity (amends 0088, 0090)

- The DLL never starts `Nail.exe` when the caller runs elevated or above
  medium integrity (`TokenElevation`, `TokenIntegrityLevel`; unknown counts
  as elevated): it reaches a Nail that already runs, or fails. Test builds
  starting the harness are exempt (CI runners are elevated).
- The shell's pipe gets a mandatory label at its own integrity level with
  no-write-up (`S:(ML;;NW;;;<level>)`, `win::server_pipe_sddl`): an
  elevated Nail takes no messages (attachment paths it would read with its
  rights) from the user's unelevated programs.

### The staging root is checked and pinned (amends 0088)

`%TEMP%\nail-mapi` (for a service `C:\Windows\Temp\nail-mapi`, which any
user can pre-create) was swept with `remove_dir_all` for everything older
than a day, following a junction at the root.

- `windows::staging_root::RootGuard` opens the root with
  `FILE_FLAG_OPEN_REPARSE_POINT` and refuses a reparse point, a non-directory
  or an owner other than the caller, the Administrators group or SYSTEM
  (an admin's elevated programs create it owned by Administrators; nobody
  else can make those the owner). The handle is held, without
  `FILE_SHARE_WRITE` / `FILE_SHARE_DELETE`, for as long as the call works
  below the root and until its copies are removed: the root cannot be
  renamed, deleted or turned into a junction meanwhile.
- The sweep removes only folders named like the DLL's own
  (`<pid>-<hex nanos>-<n>`) and skips junctions and symlinks (checked from
  the listing, not by following them).

### Release workflow: the update key reaches `tauri bundle` only

`release.yml` set the minisign private key in the job's `env`, visible to
`npm ci`'s dependency install scripts. It is now step-level on "Build
installers", and `build-installer.mjs` removes it from every child process
except `tauri bundle` (`withoutUpdaterSecrets`; cargo build scripts, the UI
build and `tauri build --no-bundle` never see it). Checked by script tests.

### CI runs the M4b desktop scenarios

The `desktop` job ran only `desktop.spec.ts`; `desktop-m4b.spec.ts` skips
itself in CI without `NAIL_E2E_DESKTOP`, so none of WP115's scenarios ran in
CI. The job now runs both; a script test fails when a spec that is gated on
`NAIL_E2E_DESKTOP` is missing from the job's filter.

### Smaller fixes

- **"Start with Windows" managed by the machine.** `Response::DesktopSettings`
  carries `start_with_windows_managed` (`DesktopHost::start_with_windows_managed`,
  default `false`; the shell reports the MSI's HKLM Run value). The UI
  locks the switch and explains that only an administrator can turn it off
  (settings and first run; `system-desktop-start-with-windows-managed`).
- **Release procedures moved to `docs/release.md`** (update key handling
  and rotation, Authenticode signing, the release steps, who may see the
  key, test-only features); `packaging/README.md` points there.

## Consequences

- The real elevated NSIS MAPI step and the MSI still run only in CI / by
  hand (they need a UAC prompt); the command is covered by the expansion
  test and a local unelevated run against HKCU.
- Anyone adding an environment-driven test hook to the app or the DLL puts
  it behind `test-overrides`.
- Follow-ups: refresh the protected copy on updates that change the pipe
  protocol (needs an elevated step, so a normal re-install); ask the shell
  to start Nail de-elevated for elevated callers instead of failing.
