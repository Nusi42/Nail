# Installer resources

The per-user NSIS installer of the desktop app (plan M2b WP44, ADR 0040),
built by the Tauri bundler from the `bundle` section of
`../tauri.conf.json`. Build it with `node scripts/build-installer.mjs`
(also makes the portable zip and the SBOMs), test it with
`scripts/installer-smoke.ps1`.

| File | What |
| --- | --- |
| `installer.nsi` | Nail's copy of the Tauri NSIS template; changes marked `; NAIL:` |
| `hooks.nsh` | Nail's logic: registration, options page, PATH, autostart, uninstall cleanup |
| `languages/English.nsh` | every user-visible string of `hooks.nsh` / the `NAIL` blocks |

What the installer does:

- installs to `%LOCALAPPDATA%\Programs\Nail` without admin rights:
  `Nail.exe`, `bin\nail.exe` (the CLI) and `uninstall.exe`;
- installs the WebView2 runtime (embedded bootstrapper) only when missing;
- registers Nail for `mailto:`, `webcal:`, `webcals:`, `.eml`, `.ics` and
  `.vcf` (ProgIDs, "Open with", Capabilities, RegisteredApplications) under
  `HKCU` — the user picks Nail in Settings > Default apps; Nail never takes
  a default over;
- options page, or `/AUTOSTART=0|1` and `/ADDTOPATH=0|1` for silent
  installs (`/S`): start with Windows (minimized), `bin\` on the user `PATH`;
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
