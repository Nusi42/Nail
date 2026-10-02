# WiX (per-machine MSI)

Slot of plan M4b WP112 (ADR 0087): the MSI template / fragments for
`Nail.msi`. Properties for silent installs (`msiexec /i Nail.msi /qn …`):
`AUTOSTART`, `ADDTOPATH`, `DISABLEUPDATES` (default 1: IT deploys
updates; written as `DisableUpdates` under `HKLM\Software\Nail\Install`),
`MAPI` (the Simple MAPI component).
