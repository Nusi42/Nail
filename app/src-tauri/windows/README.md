# Windows packaging sources

Installer templates and hooks of the desktop app (plan M2b WP44, M4b
WP112; ADRs 0040, 0087). Built by the Tauri bundler from the `bundle`
section of `../tauri.conf.json` (or the overlay `scripts/build-installer.mjs`
passes) — never by hand.

| Folder | What |
| --- | --- |
| `nsis/` | the per-user `Nail-setup.exe`: Nail's copy of the Tauri NSIS template, hooks, strings |
| `wix/` | the per-machine `Nail.msi` (WiX): template / fragments, properties `AUTOSTART`, `ADDTOPATH`, `DISABLEUPDATES`, `MAPI` (M4b WP112) |

Both installers write the installation facts the app reads
(`nail_api::desktop::registry`: `Software\Nail\Install` with `Installer`,
`Scope`, `InstallDir`, `Version`, `DisableUpdates`, `Mapi`) and, with the
optional MAPI component, the Simple MAPI client registration
(`nail_api::mapi::registry`, DLLs at `mapi\<arch>\NailMapi.dll`).

`../tests/installer.rs` checks these sources against the app (owned with
this folder by WP112 in wave M4b).
