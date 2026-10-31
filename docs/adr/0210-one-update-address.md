# 0210 — One update address, no update channels

Status: accepted (2026-10-31); revises the update channels of ADRs 0087, 0089 and 0203

## Context

Since M4b (ADR 0087) an installed Nail chose an update channel, `stable` or
`beta` (Settings → Updates, or the group policy value `UpdateChannel`),
and asked `<base>/<channel>/latest.json`. ADR 0203 put those manifests on
GitHub as the asset `latest.json` of two rolling pre-releases tagged
`stable` and `beta`, which `release:publish` replaced on every release.

That is machinery without users: there are no beta builds, the two rolling
releases clutter the public release list and its tags, `release:publish`
needed a version comparison so a stable release would not overwrite a newer
beta, and the setting, the policy, the strings and the tests all carried a
choice nobody made. GitHub already answers the one question the app asks
("what is the newest release?"): `<repository>/releases/latest/download/<asset>`
is the asset of the release marked Latest, and a pre-release is never
marked Latest.

## Decision

- **One update address.** A release build asks
  `<repository>/releases/latest/download/latest.json`
  (`release:prepare` builds the app with the update base
  `<repository>/releases/latest/download`; `build-installer.mjs
  --update-base URL` makes the endpoint `<URL>/latest.json`). The shell uses
  the configured endpoints as they are: no `{{channel}}` placeholder, no
  channel part to swap.
- **The manifest is an asset of the version's own release.**
  `release:publish` creates the release `vX.Y.Z` with every built file and
  `latest.json` and marks it Latest (on a re-run: replaces the files and
  marks it Latest again). Its installer URLs point at that release's own
  downloads (`<repository>/releases/download/vX.Y.Z/…`). No other release
  is created or changed.
- **Pre-releases are never offered.** A `-beta…` version is published as a
  pre-release with `--latest=false`; it carries its `latest.json` too, but
  the latest-release address never names it. Testers install a pre-release
  by hand from its release page.
- **The channel is gone everywhere**: `UpdateChannel`, `UpdateSettings.channel`,
  `UpdateInfo.channel`, `InstallationInfo.forced_channel` and
  `UpdateStatus.forced_channel` in the contract (regenerated TypeScript),
  the `update-channel-*` strings and the error `update-channel-locked`, the
  channel select in Settings → Updates and the channel line in "About Nail",
  the CHANNELS lists and `channelAsset` / `channelOf` in the release scripts,
  `--channel` / `--existing-beta` of `update-manifest.mjs` (it writes
  `<out>/latest.json`) and `--channel` of `release-version.mjs`.
- **Old settings and policies are ignored.** Update settings stored by 0.8
  keep a `channel` field; it is ignored when read and dropped on the next
  save. The policy value `UpdateChannel` under `Software\Policies\Nail` is no
  longer read and has no effect (documented for administrators);
  `DisableUpdates` stays the policy that controls updates.

## Consequences

- One manifest per release, kept with the release it describes; no rolling
  tags in the public repository, no version comparison when publishing.
- Installed 0.8.0 and 0.8.1 copies ask the old channel addresses and will
  not be offered 0.8.2 or later automatically; this is accepted (very few
  downloads), and they update by installing a new version by hand.
- After 0.8.2 is published, the maintainer deletes the rolling releases and
  their tags by hand: `gh release delete stable --cleanup-tag --yes` and
  `gh release delete beta --cleanup-tag --yes`.
- An organization that set `UpdateChannel` gets the latest release; to hold
  updates back it uses `DisableUpdates` (or the MSI) as before.
