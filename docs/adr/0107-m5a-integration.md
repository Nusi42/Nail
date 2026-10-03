# ADR 0107: M5a integration — appearance wiring, Follow Windows, the default theme without superseded rules, attachments, baselines

Status: accepted · 2026-10-03 · M5a WP155

## Context

The four parallel packages of wave M5a delivered against the contract (ADR
0102): WP151 the five theme files, the base token scales and the contrast
test (ADR 0103), WP152 theme loading and selection in `nail-config` and the
core (ADR 0104), WP153 the UI primitives, Lucide icons, the mail-pane fixes
and the appearance page (ADR 0105), WP154 the visual-regression harness
(ADR 0106). Their reports left wiring, contract requests and the defaults to
the integration stage.

## Decisions

### Wiring in the core

- `Service::open` starts Follow Windows with
  `appearance::watch_system_appearance(service, options.system_appearance)`
  and keeps the guard in `Inner::appearance_watch`. `None` = the platform
  source: the Windows registry watcher (`AppsUseLightTheme`,
  `SPI_GETHIGHCONTRAST`, `RegNotifyChangeKeyValue` + a 5 s re-check). The
  desktop app embeds the core, so "OS light / dark via the desktop shell" is
  this watcher; no separate Tauri theme event is needed.
- `demo::demo_options` installs a `ManualSystemAppearance` (light) when the
  options bring none, so seeded runs, `nail serve --seed …`, the desktop
  smoke tests and the core tests never depend on the machine's mode.
  demo-multi keeps its own source (`DemoMulti::system_appearance`) and the
  test control `system-appearance {scheme, high_contrast?}` flips it.
- `resync_events` starts with `ThemeChanged` (the list and selection), then
  `ConfigChanged`, so a lagging client gets both back.
- `Request::ReportSystemAppearance {system}` (new; remote: session) is the
  browser's `prefers-color-scheme` / `prefers-contrast` / `forced-colors`.
  The core applies it only while it has **no** OS source of its own
  (`appearance_watch` is `None`: a non-Windows host); otherwise it is
  ignored and the current appearance is answered (`Response::Themes`).
  The OS source wins because a remote browser's mode must not switch the
  desktop's theme.
- The `m5a-not-ready` placeholder is gone (catalog, catalog test, UI).

### Attachments of received mail (requested by WP153)

- `Request::GetMessageAttachment {message_id, index}` →
  `Response::AttachmentData {mime, data}` (base64), capped at
  `MAX_ATTACHMENT_DOWNLOAD` (64 MiB, `too_large` / `attachment-too-large`
  above). Remote: `attachments` scope plus `check_readable` (the encrypted
  gate), like reading the message.
- `Request::SaveAttachment {message_id, index, path, overwrite}` →
  `Done`; desktop only (`DesktopOnly`), the path must be absolute,
  `nail_portability::files::write_file` writes atomically and refuses an
  existing file unless `overwrite` (the Save dialog asked).
- Both read the message as the reader does: a headers-only body is fetched
  first, encrypted mail is decrypted in memory (`readable_raw`), the index
  is `MessageBody.attachments`' (`nail-core/src/attachments.rs`).
- The UI (`widgets/attachmentSave.ts`) asks `PickPath` (save) first: a path
  → `SaveAttachment`; cancelled → nothing; `unsupported` (no desktop
  shell) → `GetMessageAttachment` and a browser download (object URL on a
  temporary link). `ui/src/api/pending.ts` and its mock are removed; the
  requests are generated types now.
- Deferred: the summary fields `attachment_count` / `invitation` /
  `warning` for the tag line (they need a store migration; the tag line
  shows "Attachment" from `has_attachments` until then), and a menu header
  entry (`MenuItem::Header`, styled already as `context-menu.header`).
  Both are M5b follow-ups.

### Defaults

- `defaults/theme.kdl` loses the 88 part rules the UI skin
  (`ui/src/theme/skin.ts`) replaces, listed by WP153 (toolbar, tabs, folder
  tree, list rows, reader header, search field, windows' look, forms,
  context menus …); `window`'s size rules and the other kept rules stay.
  While they existed they overrode the skin, so the real app looked old.
- The default theme's `icons` section is removed: Lucide renders every
  icon; a theme's own `icon` still replaces a Lucide icon of that name.
- New colour tokens `avatar-fg`, `avatar-1` … `avatar-8`, `file-*`
  (WP153's skin used them with fallbacks), black in High contrast; the
  contrast test checks white text on every one (AA; AAA in High
  contrast).
- The skin outlines inputs and the search field with
  `var(--nail-border-control, …)`, so High contrast's 2 px control border
  and a user's `border "control"` apply to them; the contrast test's
  `CONTROLS` list keeps only parts that still have default rules, and a
  skin test checks the four moved ones.
- Settings gets the tab `appearance-settings id="appearance"` right after
  Accounts; `appearance-settings` is a registered widget kind (no options).
- `crates/nail-api/fixtures/default-{theme,layout}.json` regenerated.

### Tests and tooling

- Core: `tests/m5a.rs` (first run Frost / Graphite with Follow Windows,
  OS switches incl. High contrast with `ThemeChanged` before
  `ConfigChanged`, fixed theme + density, refused themes / overrides,
  reports ignored with a watch and applied without, resync, attachment
  download and save with conflict / overwrite / relative path). Existing
  tests adjusted for the intended changes (served theme =
  `default_selected_theme()`, `ThemeChanged` before `ConfigChanged` on an
  accent override, no default icons / row rules).
- Playwright: `e2e/tests/m5a.spec.ts` (themes from the gallery written to
  `theme.kdl`, the accent override, Compact density across themes with the
  tag line hidden and smaller rows, Follow Windows through the test
  control incl. High contrast and another pair, a user theme added to
  `themes/` at run time and a broken one listed, an attachment downloaded).
- Visual regression (ADR 0106) wired: `npm run visual` /
  `visual:update` / `visual:review` / `visual:selftest`, its unit tests in
  `npm test`, `tsc -p e2e/visual` in `npm run lint`, CI job `visual`
  (non-blocking until a CI run shows the hosted runner renders like the
  reference machine). Baselines for 5 themes × 2 densities × 6 screens are
  committed under `e2e/visual/baselines/win32/` after review.
- ADRs of the packages renumbered without package suffixes: 0103 themes,
  0104 theme selection, 0105 UI primitives, 0106 visual regression.

## Consequences

- The core decides the theme everywhere; the UI only renders and reports.
  On Windows the registry is the single source of the OS mode.
- The base theme is smaller; the look of primitives lives in the UI skin
  (token-only, themeable through tokens and part rules, which still win).
  User themes that relied on the removed default declarations of those
  parts keep their own declarations; the merge dialog shows such rules as
  "removed" by the incoming default.
- Attachments can finally be saved; large attachments in the browser need
  the desktop app.
