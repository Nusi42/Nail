# Windows packaging sources

Installer templates and hooks of the desktop app (plan M2b WP44, M4b
WP112; ADRs 0040, 0087, 0089). Built by the Tauri bundler from the
`bundle` section of `../tauri.conf.json` merged with `bundle.json` here (and
the per-build overlay `scripts/build-installer.mjs` writes) — never by hand.

| Path | What |
| --- | --- |
| `bundle.json` | bundle settings every release build adds: the WiX fragment, its feature, the fixed MSI upgrade code |
| `nsis/` | the per-user `Nail_<v>_<arch>-setup.exe`: Nail's copy of the Tauri NSIS template, hooks, strings |
| `wix/` | the per-machine `Nail_<v>_<arch>_en-US.msi`: fragment and strings; properties `AUTOSTART`, `ADDTOPATH`, `DISABLEUPDATES`, `MAPI` |

Both installers write the installation facts the app reads
(`nail_api::desktop::registry`: `Software\Nail\Install` with `Installer`,
`Scope`, `InstallDir`, `Version`, `Mapi`, and for the MSI `DisableUpdates`;
`HKCU` for NSIS, `HKLM` for the MSI) and, with the optional MAPI component,
the Simple MAPI client registration (`nail_api::mapi::registry`, DLLs at
`mapi\<arch>\NailMapi.dll`).

`../tests/installer.rs` checks these sources against the app and
`nail_api`; `scripts/installer-smoke.ps1` and `scripts/msi-smoke.ps1` run
the built installers.
