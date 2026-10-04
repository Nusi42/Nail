# ADR 0055: M2d integration — the plug-in host in the core, placements, `nail plugin dev`, e2e

Status: accepted · 2026-09-30 · M2d WP64

## Context

Wave M2d built the plug-in preview in parallel packages against the
contract of ADR 0050: the host `nail-plugins` (ADR 0051), the plug-in UI
(ADR 0052), the SDK, template, example and plug-in user docs (ADR 0053),
and the M2c carry-overs (ADR 0054). WP64 wires them together (plan
section 7): the host inside `nail-core` behind the permission gate, HTTP
through the core, notifications through the existing paths, plug-in
actions dispatched from the UI through the core, the plug-in UI mounted,
placements written to `layout.kdl`, `nail plugin dev`, and the Playwright
scenarios.

## Decisions

### The host inside the core (`nail-core/src/plugin_host.rs`)

- `PluginsCtl` (in the service's `Inner`) owns one `PluginHost`, created on
  first use inside the Tokio runtime with `ServiceOptions::plugin_limits`
  (`HostLimits::default()`), the `PackageStore` in `<data-dir>/plugins`
  (none for `Service::from_parts`), the plug-ins' own catalogs (cached by
  package hash / dev folder) and the plug-in HTTP client.
  `Service::start` starts every enabled plug-in (installed: package loaded
  and **verified against the stored hash**, a changed package is recorded
  as the plug-in's error `plugin-package-hash-mismatch`; dev: `load_dev`);
  `Service::shutdown` stops them; dropping the service drops the host.
- `CoreServices` implements `PluginServices`. It holds the service
  **weakly** (the service owns the host). Mail reads use the store and the
  search index: `mail_get` with the reader's `message_is_encrypted` and the
  `Cc` header from the raw message (never decrypted); `mail_search` runs the
  local index once per account of the scope (no server search, like MCP),
  newest first, at most 500; list results mark `encrypted` from the cached
  security status or the raw structure. `mail_body` checks
  `message_is_encrypted` before **and after** `GetMessage` (a headers-only
  message may be downloaded by the call) and refuses when the reader's
  status says encrypted, so decrypted text never reaches a plug-in.
  Storage maps to `Store::plugin_storage_*` (`QuotaExceeded` →
  `too_large`, now also in `CoreError::code`).
- `log` hands lines to one writer task (stored in order, then broadcast as
  `PluginLogLine`); `emit` broadcasts at once and never blocks a plug-in
  thread: crash stops are recorded with `set_plugin_error` on a task;
  `PluginsChanged` triggers a task that follows dev plug-ins' manifests
  into the store (a reload may change them; grants of permissions no
  longer declared are dropped); `system` notifications are delayed onto a
  task that clears `system` when notifications are off or quiet hours
  apply (the toast always shows). The host names plug-ins by their raw
  manifest name, so `PluginStopped` / stored errors get their `plugin`
  argument resolved from the plug-in's catalog (`%name%` → "Follow up").
- Events are broadcast to every UI; each UI shows only dialogs and
  notifications with its own `client_id` (or none). Remote sessions get
  plug-in events only with a grant for every account, log and stop events
  only as admins (ADR 0050, unchanged).

### HTTP for plug-ins (`nail-core/src/plugin_http.rs`)

`nail.http.fetch` requests (already checked by the gate) run in the core
like `nail-proxy`'s fetcher, with per-hop checks: the gate's URL rules
(https on the default port, http / other ports only for loopback hosts, no
credentials, host in the `network:` patterns via `gate::host_allowed`), IP
literals only when a pattern names them, DNS names resolved once with
**every** answer public (`nail_proxy::is_public_ip`) and the connection
pinned to them (peer re-checked), redirects as hops (≤ 5; 303 and
301/302-after-POST become GET), no cookies / Referer / system proxy, a
fixed `User-Agent: Nail-Plugin`, `max_response_bytes` → `too_large`, the
request's timeout → `network`. Packages installed from URLs are downloaded
with the same client (host = the URL's host, ≤ 20 MiB, 60 s).

### Requests (`nail-core/src/plugins.rs`)

- Preview reads the file (≤ 20 MiB) or URL and `PluginPackage::read`s it;
  the manifest is localized with its catalogs; `installed_version` comes
  from the store.
- Install checks the bytes against the previewed hash **before** anything
  is written (`conflict` + `plugin-package-hash-mismatch`), the manifest,
  the grants (`check_plugin_grants`), then `PackageStore::install` (checks
  again) and the store; it starts the plug-in when `enable` (a failing
  start leaves it installed and crashed) or stops a running older version.
- `LoadDevPlugin` needs an absolute folder, reads the manifest first (issues
  → `invalid_request` with the first issue's text), checks the grants,
  stores it as a `dev` plug-in and starts it with hot reload.
- Enable clears the stored error and starts; disable stops; uninstall stops,
  removes the rows and the package file; grants and settings reach a running
  plug-in at once.
- `RunPluginCommand` / `PluginUiAction` check that the plug-in runs and
  declares the command (`not_found` / `unavailable`), then **answer `Done`
  at once** and run the plug-in on a task: a runaway command never blocks
  the UI (the host stops it after its CPU budget). `RespondPluginDialog`
  (`not_found` when closed) and `GetPluginPanel` go to the host directly.
- `PluginInfo.state` / `error` come from the host when it knows the plug-in
  (the store's `enabled` flag wins), else from the store.

### Placements and the merge dialog (`nail-config/src/plugins.rs`)

`placement_layout(current, plugin, manifest)` edits the user's
`layout.kdl` in place (like the write-back; `ui/.../placementText.ts` is
the reference): toolbar / menu items after / before their anchor else at
the end (missing toolbars / menus are created, items never duplicated),
`bind` lines only for keys neither the context nor the global keys bind,
`plugin-panel "<id>.<panel>"` in every breakpoint of the main window after /
before the first widget of the anchor kind (else as the last child of the
window's root container) with the parent's `sizes` / `min-sizes` /
`max-sizes` / `collapsible` extended (`280px` in a split, `auto` in a
stack, `none`, `true`). `None` when nothing would change: the core then
answers an empty `incoming` and preview. `ConfigManager::preview_placements`
returns the text and the merge preview; the merge preview knows the
installed plug-ins (`InstalledPlugins`, refreshed by the core), so their
commands and panels are not flagged "not installed". The pre-M2d widget
form `plugin.<id>.<name>` is refused (`config-plugin-widget-renamed`,
pointing to `plugin-panel`); merge conflicts about plug-in widgets now look
at `plugin-panel` panels.

### UI, defaults, theme

`usePluginLayout` wraps the resolved layout in `App.tsx` and the test
harness (plug-in commands become actions, so toolbars, menus, context menus
and keys work); the theme compiler appends `PLUGIN_BASE_CSS` and maps
`plugin.<id>.<part>` rules to `[data-part="plugin.<part>"][data-plugin="<id>"]`.
`defaults/layout.kdl` gets a Plug-ins settings tab (`plugin-manager`, now a
registered widget) and the actions `plugins.manage` / `install` /
`load-dev`; `defaults/theme.kdl` styles every plug-in part. Plug-in dates go
through `i18n/format.ts` (WP65's rule, no exception left).

### `nail plugin dev` (`nail-cli/src/plugin.rs`)

`nail serve` writes `<data-dir>/ui-endpoint.json` (`{url, pid}`, removed on
exit). `nail plugin dev <folder> [--url URL] [--data-dir DIR]` connects to
that Nail's local WebSocket, sends `LoadDevPlugin` with every declared
permission for all accounts, prints "Loaded …" and then the plug-in's log
lines (localized host lines, its own lines, API calls with outcomes) until
Ctrl+C; the plug-in stays loaded. The desktop app has no local WebSocket;
there the manager's "Load a development folder…" does the same (a
follow-up could give the desktop an endpoint).

### Desktop

`system` plug-in notifications become OS notifications in the Tauri shell
(`Service::plugin_notification_texts`: the plug-in's title, its body and
"from `<plug-in>`").

### Fixes found while integrating

- The host logged "unhandled promise rejection" for API calls that failed at
  once and were awaited inside `try` (QuickJS reports the rejection before
  the `await` attaches its handler). Rejections are now collected per turn
  and only those still unhandled when the turn ends are logged
  (`runtime.rs`; test `awaited_immediate_rejections_are_not_reported`).
- reqwest with `rustls-no-provider` needs a crypto provider even for
  `http:`; the plug-in client always passes the ring-based TLS config.

### End-to-end scenarios

`e2e/tests/plugins.spec.ts` (private `nail serve --seed demo` each):
install the example through Settings → Plug-ins with its four permissions,
place it (layout.kdl gets the toolbar item after `mail.forward` and the
Shift+F binding), run it from the reader toolbar, save a reminder (toast;
the dialog then offers "Remove reminder" and the note — the data was
stored), axe on the plug-in dialog and toast; a plug-in with only
`mail.read` refused `getBody` (toast + the manager's log line); a runaway
plug-in stopped (toast "…took too long", manager `data-state="crashed"`,
button disabled) while another message opens; `nail plugin dev` loading a
folder, its console, a command from the manager's placement, and a hot
reload to a new version and code. The example is built by the Playwright
global setup (`e2e/support/plugins.ts`: esbuild + `packPlugin`, output
under the build directory, never the repo). `a11y-windows.spec.ts` also
scans the install dialog.

## Consequences

- The plug-in preview of the design runs end to end on the desktop and in
  the browser; the remote UI uses the host's plug-ins under ADR 0050's
  rules.
- Follow-ups (plan "M2d follow-ups"): a desktop endpoint for
  `nail plugin dev`; panel action values filtered against the last rendered
  panel tree; a cumulative CPU quota / wall-clock limit for long async
  chains; binary / streaming HTTP bodies; a runtime API for plug-ins' own
  catalogs (`nail.l10n`); `Cc` only in `nail.mail.get` (lists carry none);
  events routed to one UI by `client_id` in the transports instead of
  filtered in the UI; the SDK's `api.ts` mirrors `prelude.js` by hand.
