# Installer resources

The per-user NSIS installer of the desktop app (plan M2b WP44, M4b WP112;
ADRs 0040, 0089), built by the Tauri bundler from the `bundle`
section of `../../tauri.conf.json`. Build it with
`node scripts/build-installer.mjs` (also makes the MSI, the portable zip
and the SBOMs), test it with `scripts/installer-smoke.ps1`.

| File | What |
| --- | --- |
| `installer.nsi` | Nail's copy of the Tauri NSIS template; changes marked `; NAIL:` |
| `hooks.nsh` | Nail's logic: registration, installation facts, options page, PATH, autostart, Simple MAPI, uninstall cleanup |
| `languages/English.nsh` | every user-visible string of `hooks.nsh` / the `NAIL` blocks |

What the installer does:

- installs to `%LOCALAPPDATA%\Programs\Nail` without admin rights:
  `Nail.exe`, `bin\nail.exe` (the CLI), `mapi\<arch>\NailMapi.dll` +
  `mapi\x86\NailMapi.dll` and `uninstall.exe`;
- installs the WebView2 runtime (embedded bootstrapper; the
  `-offline-setup.exe` variant carries the full runtime) only when missing;
- registers Nail for `mailto:`, `webcal:`, `webcals:`, `.eml`, `.ics` and
  `.vcf` (ProgIDs, "Open with", Capabilities, RegisteredApplications) under
  `HKCU` — the user picks Nail in Settings > Default apps; Nail never takes
  a default over;
- writes the installation facts the app reads
  (`HKCU\Software\Nail\Install`: `Installer=nsis`, `Scope=per-user`,
  `InstallDir`, `Version`, `Mapi`) on every install and update;
- options page, or `/AUTOSTART=0|1`, `/ADDTOPATH=0|1` and `/MAPI=0|1` for
  silent installs (`/S`): start with Windows (minimized), `bin\` on the user
  `PATH`, Simple MAPI ("Send to > Mail recipient");
- **Simple MAPI** is the one part that needs admin rights (Windows reads
  the provider from `HKLM\Software\Clients\Mail\Nail`): the installer writes
  a `.reg` file and imports it with one elevated `reg.exe` (one UAC prompt,
  also in silent mode); declining keeps Nail installed without it. Updates
  keep the registration; uninstall removes it (one more prompt) only if it
  points into this installation;
- uninstall keeps settings, mail data and stored passwords unless the
  checkbox (silent: `/DELETEDATA`) is set.

## Upgrading the Tauri CLI

`installer.nsi` is based on the template inside `@tauri-apps/cli` (its
version is in the file header). When `package-lock.json` moves to another
CLI version, `cargo test -p nail-app --test installer` fails until the copy
is renewed:

1. Take the template of the new version: it is
   `crates/tauri-bundler/src/bundle/windows/nsis/installer.nsi` in the Tauri
   repository at the CLI's tag (or build once with the stock template and
   use `…/release/nsis/x64/installer.nsi` minus the rendered values).
2. Re-apply the four `; NAIL:` blocks (default directory, options page,
   uninstall checkbox text, string include) and update the
   `NAIL-TEMPLATE-BASE` line.
3. Build and run the smoke test.

German installer strings come later (`userDocs/de` and `locales/de` are
empty for now); the installer is English only (`languages: ["English"]`).
