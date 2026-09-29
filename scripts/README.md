# scripts/

Build and release helper scripts (plan M2b WP44, ADR 0036): SBOM generation
(CycloneDX), the installer smoke test (install to a temporary directory,
start, uninstall), licence and advisory checks run by CI (`cargo-deny` with
`deny.toml` at the repository root).

Owned by WP44 during wave M2b. Scripts must not write build output into the
repository (it lives in OneDrive); use `%LOCALAPPDATA%\nail-build\` or the
`NAIL_BUILD_DIR` override like the e2e suite does.
