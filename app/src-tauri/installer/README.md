# Installer resources

NSIS installer hooks and templates for the Tauri bundler (plan M2b WP44,
ADR 0036): per-user install by default, the WebView2 bootstrapper,
`mailto:` / `webcal:` protocol handlers and `.eml` / `.ics` / `.vcf`
associations via RegisteredApplications and Capabilities, optional start
with Windows, optional `nail` CLI on `PATH`, uninstall that keeps data
unless the user opts in. A portable zip is built next to the installer.

Owned by WP44 during wave M2b, together with the `bundle` section of
`../tauri.conf.json`, `.github/workflows/`, `deny.toml` and `scripts/`.
