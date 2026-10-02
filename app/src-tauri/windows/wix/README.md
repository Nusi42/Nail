# WiX (per-machine MSI)

`Nail_<version>_<arch>_en-US.msi` for IT (Intune, Group Policy,
Configuration Manager): plan M4b WP112, ADR 0089. Built by the Tauri
bundler (WiX 3.14, downloaded on first use) from its own `main.wxs`
template plus Nail's fragment; the settings are in `../bundle.json`
(`fragmentPaths`, `featureRefs`, the fixed `upgradeCode`), which
`scripts/build-installer.mjs` merges over `tauri.conf.json`.

| File | What |
| --- | --- |
| `nail.wxs` | the fragment: properties, installation facts, registration, Run key, PATH, Simple MAPI |
| `languages/en-US.wxi` | every user-visible string of `nail.wxs` (`$(var.Name)`) |

Tauri's template already makes the package per-machine (`Program Files`,
`InstallScope="perMachine"`), installs the files (app, `bin\nail.exe`,
`ui\`, `mapi\<arch>\NailMapi.dll`, `mapi\x86\NailMapi.dll`), shortcuts and
WebView2, and handles upgrades (`MajorUpgrade`).

## Properties (silent installs)

```bat
msiexec /i Nail_0.2.0_x64_en-US.msi /qn ADDTOPATH=1 MAPI=1
```

| Property | Default | Effect |
| --- | --- | --- |
| `AUTOSTART` | `0` | `1`: every user's sign-in starts Nail in the tray (`HKLM\…\Run\Nail` = `"<dir>\Nail.exe" --autostart`) |
| `ADDTOPATH` | `0` | `1`: `<dir>\bin` (only the `nail` CLI) on the machine `PATH` |
| `DISABLEUPDATES` | `1` | `0`: allow the in-app updater; written as `DisableUpdates` under `HKLM\Software\Nail\Install` |
| `MAPI` | `0` | `1`: register `NailMapi.dll` as a Simple MAPI client (`HKLM\Software\Clients\Mail\Nail`, 64-bit view = package arch, 32-bit view = x86 DLL) |
| `INSTALLDIR` | `%ProgramFiles%\Nail` | installation directory |

A property given on the command line wins; otherwise an upgrade keeps the
previous installation's value (read back from `HKLM\Software\Nail\Install`
before `AppSearch` overwrites it — the "remember property" pattern), so
the Tauri updater's plain `msiexec /i … /passive` changes nothing.

## What it writes (HKLM)

- `Software\Nail\Install`: `Installer=msi`, `Scope=per-machine`,
  `InstallDir`, `Version` (MSI ProductVersion), `DisableUpdates`, `Mapi`
  (DWORDs; `nail_api::desktop::registry`), plus `Autostart` / `AddToPath`
  (remembered choices) and `PathEntry`.
- ProgIDs `Nail.Url.mailto`, `Nail.Url.webcal`, `Nail.eml`, `Nail.ics`,
  `Nail.vcf` under `Software\Classes` (`"Nail.exe" -- "%1"`), "Open with"
  entries, `Software\Nail\Capabilities`, `Software\RegisteredApplications\Nail`
  — the user picks Nail in Settings > Default apps
  (`ms-settings:defaultapps?registeredAppMachine=Nail`); nothing becomes a
  default by itself.
- Uninstall removes all of it; user data (`%APPDATA%\Nail`,
  `%LOCALAPPDATA%\Nail` of each user) is never touched by the MSI.

Group Policy can still force updates off or a channel through
`HKLM\Software\Policies\Nail` (`DisableUpdates`, `UpdateChannel`); the app
lets the policy win over everything.

`../../tests/installer.rs` checks this fragment against `nail_api` and the
per-user installer; `scripts/msi-smoke.ps1` installs, upgrades and removes
a built MSI (elevated; CI).
