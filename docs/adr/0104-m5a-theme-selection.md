# ADR 0104: WP152 — theme loading, selection, Follow Windows and density

Status: accepted · 2026-10-03 · M5a WP152

## Context

ADR 0102 (M5a contract) fixed the protocol (`ListThemes`, `SetTheme`,
`SetThemePreferences`, `Event::ThemeChanged`), where themes live
(built-ins in `defaults/themes/`, user themes in `<config-dir>/themes/`),
that the selection and the overrides are part of the user's `theme.kdl`,
and the rule `nail_api::appearance::active_theme`. WP152 implements
loading, validation, selection, writing and Follow Windows in
`nail-config` and `nail-core/src/appearance.rs`.

## Decisions

### Reading theme files (`nail-config/src/theme.rs`)

- `extends` of a theme file accepts `"default"` or any theme id
  (`is_theme_id`); other text is `config-theme-extends-invalid`. Whether
  the theme exists is checked where the theme list is known. Layouts keep
  `extends="default"` only.
- A new section `appearance follow-system=<bool> light=<id> dark=<id>
  density="comfortable|compact"` (`AppearanceDef`; every property
  optional; errors `config-expected-bool`,
  `config-appearance-theme-invalid`, `config-appearance-density-unknown`).
  It is part of the theme schema and is read only from the user's
  `theme.kdl`.
- `parse_theme` / `parse_theme_all` (a file on its own, e.g. an incoming
  theme in the merge dialog) merge onto a built-in's chain when `extends`
  names a built-in, and onto the base for any other id: a file read on its
  own cannot know the user's themes.

### The theme list (`ThemeCatalog`, `nail-config/src/themes.rs`)

- Built-ins (embedded) first in gallery order, then `themes/*.kdl` by id.
  A file whose stem is not a theme id, or that cannot be read, is not
  listed but reported (`config-theme-file-name-invalid`, `config-io`).
- Every theme's chain is followed (base ← … ← theme) and resolved once
  when the list is built. A user theme is listed with a `problem` (and
  its `ConfigError`s are kept per file) when it: does not parse
  (`theme-problem-invalid`, detail = first error), has a built-in's id
  (`theme-problem-shadows-builtin`; lookups and chains always use the
  built-in), extends an unknown theme (`config-theme-extends-unknown`) or
  an unusable one (`config-theme-extends-broken`), extends itself
  (`theme-problem-extends-cycle`), or does not resolve (e.g. an unknown
  token reference).
- A user theme without its own `mode` takes the kind of its chain (a
  theme extending Graphite is dark).

### Selection and the theme the UI renders (`resolve_selected`)

- `appearance_settings(theme.kdl)` implements ADR 0102: no file or
  `extends="default"` → Follow Windows with Frost / Graphite; `extends=<id>`
  → that fixed theme with Follow Windows off by default; the `appearance`
  node overrides each value. Overrides are the base values of
  `font-size "body"`, `radius "md"` and `color "accent"` in the user's file
  (a value that is not a whole number of pixels reads as no override).
- The rendered theme is the base ← the active theme's chain ← the user's
  sparse `theme.kdl` (its `extends` / `appearance` only select), with
  `ResolvedTheme.density` from the settings. A *complete* `theme.kdl` (no
  `extends`) is used as it is, as before M5; only its density applies.
- An `extends` that names a missing or unusable theme in `theme.kdl` is not
  an error of `theme.kdl`: the selection falls back by kind
  (`active_theme`) and the choice stays in the file, so it works again
  once the theme is fixed.

### ConfigManager

- `theme()` is the rendered selected theme. Without `theme.kdl` and with
  an unknown OS state that is Frost on the base
  (`default_selected_theme()`); `default_theme()` stays the base alone (the
  fixture `default-theme.json`).
- New state: the catalog, the user's last valid parsed `theme.kdl`, the OS
  state and the current `Appearance`. Any input change re-resolves and
  reports `ConfigUpdate::ThemeChanged` (when the list, selection, active
  theme or OS state changed) before `ConfigUpdate::Changed` (resolved
  theme changed). `ConfigUpdate::ThemeFileErrors {file, errors}` reports a
  user theme file's errors (empty when fixed or removed) and becomes
  `Event::ConfigErrors` for `themes/<id>.kdl`; `error_events()` includes
  every user theme file that has errors. User theme errors are not in
  `errors()` (the M1a banner): a broken user theme is never active.
- `set_system_appearance(Option<SystemAppearance>)`, `reload_themes()`,
  `appearance()`, `theme_catalog()`, `theme_file_errors()`.
- Hot reload: `watch` creates and also watches `<dir>/themes/`
  (non-recursive); events there re-read the directory. If the user's
  last valid `theme.kdl` no longer resolves on a newly active theme (a
  token only the old theme defined), the last valid theme stays and a
  warning is logged.

### Writes (`apply_theme_selection`, `apply_theme_preferences`)

- Format-preserving edits with `kdl_edit` (comments, blank lines and
  property order of everything else stay byte for byte); a missing file
  starts from the merge dialog's sparse skeleton.
- `SetTheme`: `extends="<id>"`; an existing `appearance` node gets
  `follow-system=#false` (the pair is kept so turning Follow Windows on
  again remembers it).
- `SetThemePreferences`: the `appearance` node is updated in place or
  inserted as the first child of `theme` with all four properties; the
  override tokens are replaced in place (keeping the comment above and the
  trailing comment; mode variants such as `dark=` are dropped because an
  override is one value for every mode), appended to the first `tokens`
  block or a new one, and duplicates are removed. `None` removes the line.
  `font-size "body"` is written with a `compact=` value scaled like the
  base's (14 → 13), so Compact still shrinks the text after an override.
- `ConfigManager::set_theme` / `set_theme_preferences` re-read `themes/`
  first (a theme file just added can be selected before the watcher saw
  it), refuse unknown or unusable themes (`invalid_request` with
  `theme-problem-unknown` or the theme's problem) and invalid overrides
  (the first `theme-overrides-issue-*`, args `min` / `max`), refuse an
  invalid `theme.kdl` (`config`) or a result that would not resolve
  (`config-edit-makes-invalid`), back the previous file up in `backups/`,
  write atomically and return the new `Appearance` plus the updates to
  broadcast. An unchanged text writes nothing.

### Follow Windows (`nail-core/src/appearance.rs`)

- `watch_system_appearance(service, source)` subscribes to
  `ServiceOptions::system_appearance` or `platform_source()`, applies the
  current state and pushes every change into the selection; the returned
  `SystemAppearanceWatch` unsubscribes on drop. The listener holds the
  config manager and the event sender, not the `Service` (no cycle).
- `WindowsSystemAppearance`: `AppsUseLightTheme` (0 = dark) under
  `HKCU\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize` and
  `SystemParametersInfoW(SPI_GETHIGHCONTRAST)`, watched on a dedicated
  thread with `RegNotifyChangeKeyValue` on that key and on
  `HKCU\Control Panel\Accessibility\HighContrast`, plus a 5 s re-check;
  the thread starts with the first `watch` and stops when the source is
  dropped. The registry is the documented store of the setting that
  `UISettings` reads; it needs no WinRT and works in `nail serve` too.
- `report_system_appearance(service, state)` applies a state reported
  from outside (the browser's `prefers-color-scheme`, a desktop-shell
  event).
- `handle` serves the three requests; `ListThemes` re-reads `themes/`
  first (cheap; reports nothing when unchanged), so lists are current
  without a watcher.

## Consequences

- `ConfigManager::theme()` without `theme.kdl` is now Frost on the base,
  not the base alone; tests that compared it with `default_theme()` use
  `default_selected_theme()`. Existing user files that extend `default`
  get the new default look with their own tokens and part rules on top.
- Editing an override token by hand reports `ThemeChanged` as well as
  `ConfigChanged`, so the Appearance settings stay in step with the file.
- The wiring in `Service` (storing the watch guard, choosing between the
  platform source and UI reports, a request for UI reports) is the
  integrator's (WP155).
