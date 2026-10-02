# packaging/

Distribution tooling of the Windows app (plan M4b WP112, ADR 0087). Build
output never goes into the repository (it lives in OneDrive): everything
writes to `%LOCALAPPDATA%\nail-build\` or `NAIL_BUILD_DIR`, like
`scripts/`.

Planned contents (WP112):

| Path | What |
| --- | --- |
| `winget/` | the winget manifest generator (wraps `Nail-setup.exe`) and its templates |
| `update-manifest/` | per-channel signed update manifests (`<base>/<channel>/latest.json`, Tauri updater format; `nail_api::desktop::UpdateChannel::manifest_path`) |

Conventions shared with the app: channel names `stable` / `beta`
(`nail_api::desktop::UpdateChannel`), the update key is separate from the
Authenticode certificate, signing steps run only when a certificate /
key is configured.
