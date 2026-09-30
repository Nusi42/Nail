# ADR 0052: Plug-in UI — renderer, dialogs, commands, panels, install dialog, manager

Status: accepted · 2026-09-30 · M2d WP62

## Context

Plan section 7, WP62: a React renderer for the declarative component set of
ADR 0050, plug-in dialogs, toolbar / menu / context-menu / key placements
from `plugin.<id>.<command>`, `plugin-panel` widgets, the install dialog
with permissions, the plug-in manager, and placements proposed on install
through the merge flow. The package owns only `ui/src/features/plugins/`
and `locales/{en,de}/plugins.ftl`; everything else is requested from the
integrator (end of this ADR).

## Decisions

### Rendering trees (`tree/`)

- One component per `PluginUiNode`, native labelled controls only (input,
  textarea, select, checkbox, `date` / `datetime-local`, radio / checkbox
  rows for selectable message lists and tables), so keyboard and screen
  readers work without custom widgets. Values follow the contract's table
  (`tree/values.ts`); `today` / `tomorrow` defaults resolve locally (with
  `time`: 09:00).
- **No plug-in HTML, ever.** Text is text. The `markdown` node is parsed
  by a small CommonMark subset parser in the UI into React elements
  (`tree/markdown.tsx`): headings, paragraphs, lists, quotes, fenced code,
  rules, code / strong / emphasis, links. Raw HTML stays literal text,
  images show their alternative text, only `http(s):` links become links,
  and a link opens outside Nail only after an inline confirmation (the
  contract's "links open outside Nail after a confirmation"). The contract
  has no core-rendered HTML field; rendering to elements in the UI is safer
  than injecting sanitized HTML and needs no new API. If richer Markdown
  (tables, nested lists) is wanted later, the core could render it and the
  UI would still draw elements, not HTML.
- `image` renders only `data:image/{png,jpeg,gif,webp};base64,…` (the
  host already refuses others; the UI checks again). Width / height are
  HTML attributes, not styles.
- **Parts.** Every element carries a generic part (`plugin.button`,
  `plugin.dialog.title`, … — the list is `PLUGIN_PARTS` in `parts.ts`)
  plus `data-plugin="<id>"` and, for inputs, `data-id="<node id>"`. A
  theme styles all plug-ins by the generic parts and one plug-in with a
  condition (`part "plugin.button[plugin=\"follow-up\"]"`). The design's
  namespaced names `plugin.<id>.<part>` cannot be data-part values next to
  the generic ones (one `data-part` per element, exact match), so the
  theme compiler maps them: `pluginPartSelector("plugin.follow-up.button")`
  = `[data-part="plugin.button"][data-plugin="follow-up"]` (requested
  below). Text styles are `data-appearance` (a `data-style` attribute would
  trip the source rule against `style=`).

### Dialogs (`PluginDialogs.tsx`, an effect)

- `PluginDialogOpened` for this UI's `client_id` (or `null`) opens a
  dialog; the same id again replaces its spec and keeps what the user
  typed for ids that still exist (`mergeValues`), so a plug-in can
  re-render after a `button` action. `PluginDialogClosed` and
  `PluginStopped` close it.
- Modal (default): a backdrop, `aria-modal`, `data-key-modal` (main-window
  keys do not fire), Tab trapped in the topmost modal (the window layer's
  `trapTab`). Non-modal: a floating `role="dialog"` without trap.
- Escape and the close button answer `button: null`. A primary button
  (also Enter) validates first: `required`, input kinds email / url /
  number, `maxLength`, date min / max; invalid fields get `aria-invalid`,
  an error text via `aria-describedby`, `data-state="invalid"` on the
  field and dialog, a summary alert, and focus moves to the first one.
  Other buttons answer without validation.
- The header names the plug-in ("from Follow-up") so a dialog cannot pose
  as Nail's own.

### Commands in toolbars, menus, context menus and keys (`commands.ts`)

The action registry is a static record; plug-in commands are dynamic. The
UI therefore adds, for every command of every installed plug-in, an entry
to the resolved layout's `actions`: label / icon from the manifest (the
user's `action "plugin.x.y" label=… icon=…` wins), `alias_of:
"plugins.run-command"`, `args: ["plugin.x.y"]`
(`withPluginActions`, hook `usePluginLayout`). The dispatcher's existing
alias resolution then reaches one static handler that sends
`RunPluginCommand` with the UI context, the surface and this UI's
`client_id`. Toolbars, context menus (labels via `resolveMenu`), key
bindings and later the command palette work unchanged. The handler is
enabled only while the plug-in runs (enabled, not crashed / incompatible)
and the command's contexts are satisfied (list / reader need a selected
message, composer an open composer; no contexts = everywhere). Commands of
missing plug-ins keep no handler: disabled buttons, inert keys (as the
registry already documents). This deviates from `ActionDef.alias_of`'s doc
("`null` for plug-in actions") only in the UI's in-memory layout; nothing is
written to `layout.kdl`.

### Panels (`PluginPanel.tsx`)

`plugin-panel "<plugin>.<panel>"` fetches `GetPluginPanel` with the UI
context (again when account / folder / message / thread / draft change),
follows `PluginPanelChanged`, keeps entered values across re-renders, and
sends `PluginUiAction {target: panel}` for buttons. Without an enabled
plug-in declaring the panel it shows the "not installed or not enabled"
notice and asks the core nothing. The pre-M2d widget form `plugin.<id>.<name>`
is **dropped**: the UI cannot register dynamic widget kinds and
`plugin-panel` covers it (request below).

### Notifications (`PluginNotifications.tsx`, an effect)

`PluginNotification` for this UI becomes a toast naming the plug-in;
errors stay until dismissed (inner `role="alert"`), others go after 8 s.
`system: true` also shows a Web Notification in the browser when allowed
(the desktop shell's OS notification is WP64's). `PluginStopped` with a
crash reason (error, CPU budget, memory limit) shows the core's
`plugin-stopped-*` text as an error toast and closes the plug-in's dialogs.

### Install dialog (`InstallDialog.tsx`, dialog `plugin-install`)

Source (file path, https URL, development folder) → `PreviewPluginInstall`
→ review: name, version, author, description, homepage as text, size and
fingerprint, update note, what it adds, manifest problems (texts from
`core-plugins.ftl`; installing is impossible while there are any), and
every declared permission with **Allow / Deny** radios (default allow;
denying `mail.read` also denies bodies; mail permissions can be limited to
chosen accounts, never to none) and "Turn it on right away". Install sends
`InstallPlugin` with the previewed `package_sha256` (a `conflict` answer
says the package changed) or `LoadDevPlugin` for folders. Then, when the
manifest suggests placements or keys, the placements step.

### Placements through the merge flow (`Placements.tsx`)

`PreviewPluginPlacements` returns the user's layout with the placements
added plus the merge dialog's `ConfigMergePreview`. The step lists the
differing items (KDL text and conflicts) with a checkbox each — all
proposed items preselected — and applies with the merge dialog's own
request `ApplyConfigMerge` (explicit `theirs` / `mine` per item, default
`mine`), which backs up `layout.kdl`; "Undo" reverts to that backup
(`RevertConfig`). "Not now" writes nothing. The same step is reachable
later from the manager ("Place in layout"). The customization slot's
`MergeDialog` has no way to start from a given text, so the step reuses its
helpers (`differs`, `backupsKey`) and requests instead of the component.

### Plug-in manager (`PluginManager.tsx`)

Widget `plugin-manager` and dialog `plugin-manager` (same content): each
plug-in with name, version, state, source, crash reason; an on / off
switch; sections Permissions (grant editor → `UpdatePluginGrants`),
Settings (generated from the schema, core-validated, `noValidate` so the
core's localized error shows), Log (level filter, live `PluginLogLine`,
clear), Place in layout; Remove with an inline confirmation. "Install a
plug-in…" / "Load a development folder…" open the install dialog in the
matching mode. Actions `plugins.install`, `plugins.load-dev`,
`plugins.manage` do the same from toolbars / menus / keys.

### Mock host (`mockHost.ts`)

The mock core now simulates the host (default on; `host: null` restores
`unsupported`): packages by source (`file:` / `url:` / `dev:`; the example
"Follow up" plug-in and a broken package), install with hash check and
grant check, commands written in TypeScript against a small gated API
(dialogs resolving with the user's answer, notifications, storage, logs,
panels, `mail.read` message view), refusals logged like the host,
`crashPlugin` for tests. `PreviewPluginPlacements` builds the proposed
layout text with `placementText.ts` — the reference algorithm for
`nail-config/src/plugins.rs` — and compares it through the customization
mock (which merges keys only).

## Consequences

- Plug-in UI works on the desktop and the remote UI alike; only trees and
  ids cross the API.
- Until the integrator applies the requests below, plug-in toolbar buttons
  render disabled (no action definitions), the manager is reachable only as
  the dialog `plugin-manager`, and dialogs / toasts are unstyled and not
  positioned.

## Requests to the integrator (WP64)

1. `ui/src/App.tsx` (and `ui/src/test/harness.tsx` `Providers`): wrap the
   resolved layout before it reaches `LayoutContext`, `ActionsProvider`,
   `WindowView` and `WindowLayer`:
   `const layout = usePluginLayout(config.data.layout);` (`usePluginLayout`
   from `features/plugins`). This makes plug-in commands work in toolbars,
   menus, context menus and keys.
2. `ui/src/theme/compile.ts`: append `PLUGIN_BASE_CSS` (from
   `features/plugins`) to `BASE_CSS`, and in `elementSelector` use
   `pluginPartSelector(part) ?? [data-part=…]` so `plugin.<id>.<part>`
   theme rules work.
3. `crates/nail-config/src/widgets/plugins.rs`: register
   `widget("plugin-manager", &[], &[], false)`; refuse the pre-M2d kind
   `plugin.<id>.<name>` in `layout.rs` with a message pointing to
   `plugin-panel "<id>.<name>"` (and adjust `merge.rs`'s `plugin.` kind
   handling).
4. `crates/nail-config/src/plugins.rs`: the placement writer, following
   `ui/src/features/plugins/placementText.ts` (format preserving on the
   user's file; panels into the window tree after / before the first widget
   of the named kind, else at the end of the main window).
5. `defaults/layout.kdl`: a Settings tab `plugin-manager id="plugin-manager"
   label="@plugin.manager.tab"`; optionally actions `plugins.manage`,
   `plugins.install`, `plugins.load-dev` with labels `@plugin.action.manage`,
   `@plugin.action.install`, `@plugin.action.load-dev` (ids exist in
   plugins.ftl); then regenerate the nail-api fixtures.
6. `defaults/theme.kdl`: default styles for the plug-in parts, e.g.

   ```kdl
   part "plugin.dialog.backdrop" { padding "$space.xl"; background-color "$color.scrim"; }
   part "plugin.dialog" {
       width "min(560px, 100%)"; border "$border.thin"; border-radius "$radius.lg"
       color "$color.fg"; background-color "$color.bg"; box-shadow "$shadow.overlay"
   }
   part "plugin.dialog[size=\"small\"]" { width "min(400px, 100%)"; }
   part "plugin.dialog[size=\"large\"]" { width "min(900px, 100%)"; }
   part "plugin.dialog.header" { gap "$space.sm"; padding "$space.sm" "$space.lg"; border-bottom "$border.thin"; background-color "$color.bg-subtle"; }
   part "plugin.dialog.title" { font-size "$font-size.lg"; font-weight "$font-weight.bold"; }
   part "plugin.dialog.plugin" { color "$color.fg-muted"; font-size "$font-size.sm"; }
   part "plugin.dialog.body" { gap "$space.md"; padding "$space.lg"; }
   part "plugin.dialog.buttons" { gap "$space.sm"; }
   part "plugin.dialog.error" { color "$color.danger"; }
   part "plugin.stack" { gap "$space.sm"; }
   part "plugin.field" { gap "$space.xs"; }
   part "plugin.field.error" { color "$color.danger"; font-size "$font-size.sm"; }
   part "plugin.field[state=\"invalid\"] plugin.input" { border-color "$color.danger"; }
   part "plugin.text[appearance=\"heading\"]" { font-size "$font-size.lg"; font-weight "$font-weight.bold"; margin 0; }
   part "plugin.text[appearance=\"muted\"]" { color "$color.fg-muted"; }
   part "plugin.text[appearance=\"error\"]" { color "$color.danger"; }
   part "plugin.button[variant=\"primary\"]" { color "$color.accent-fg"; background-color "$color.accent"; }
   part "plugin.button[variant=\"danger\"]" { color "$color.danger"; }
   part "plugin.notification" { gap "$space.sm"; padding "$space.sm" "$space.md"; border "$border.thin"; border-radius "$radius.md"; background-color "$color.bg"; box-shadow "$shadow.overlay"; }
   part "plugin.notification[level=\"error\"]" { border-color "$color.danger"; }
   part "plugin.notification[level=\"success\"]" { border-color "$color.success"; }
   part "plugin.notification.plugin" { color "$color.fg-muted"; font-size "$font-size.sm"; }
   part "plugin-panel" { padding "$space.md"; gap "$space.sm"; }
   part "plugin-logs.entry[level=\"error\"]" { color "$color.danger"; }
   part "plugin-logs.list" { font-family "$font.mono"; font-size "$font-size.sm"; }
   ```

   plus the `form.*`-like spacing for `plugin-install.*`, `plugin-manager.*`,
   `plugin-permissions.*`, `plugin-placements.*` (they reuse `form.button`,
   `form.field`, `form.label`, `form.input`).
7. WP64 core: `RunPluginCommand` / `PluginUiAction` answer `Done` at once
   and run the plug-in asynchronously (the UI does not wait on dialogs);
   `RespondPluginDialog` for an unknown dialog answers `not_found` (the UI
   then just closes it); `PluginDialogOpened` with an existing id updates
   the dialog; `PluginPlacements.incoming` is empty with an empty preview
   when nothing would change; crashed plug-ins carry
   `localized = plugin-stopped-<reason>` with `$plugin`.
8. Desktop shell: show `PluginNotification` with `system: true` as an OS
   notification (the web UI already uses Web Notifications).
9. WP63 / user docs: `userDocs/en/plugins/` should describe the install
   dialog (Allow / Deny, account limits, the fingerprint), the manager and
   the placements step; theming plug-in parts (`plugin.<part>`,
   `[plugin="<id>"]`, `plugin.<id>.<part>`).
