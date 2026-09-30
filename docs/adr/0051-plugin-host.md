# ADR 0051: the plug-in host — runtimes, gate, API, packages, dev mode

Status: accepted · 2026-09-30 · M2d WP61

## Context

WP61 fills `crates/nail-plugins` against the contract of ADR 0050: one
QuickJS runtime per plug-in with memory and CPU limits, a permission gate
in front of the core, the preview API, `.nailplugin` packages, install /
uninstall / enable / disable, dev folders with hot reload, and crash
isolation. The core wires it in WP64; this crate depends on `nail-api`
only, never on the store or the core.

## Decisions

### Threading: one OS thread per plug-in

rquickjs runs without its `parallel` feature, so a `Runtime` is `!Send`.
Each running plug-in gets its own thread (`nail-plugin-<id>`, 8 MiB
stack) that owns its `Runtime` and `Context` for its whole life and runs an
event loop over messages (`ThreadMsg`). Nothing QuickJS ever crosses a
thread. The interrupt handler uses atomics (`Budget`), so it would also
satisfy `parallel` if that is ever enabled. API calls that need the core are
spawned on the host's Tokio runtime (`Handle` given to `PluginHost::new`);
results return to the plug-in thread as messages. A panic of a plug-in
thread is caught and treated as a crash.

Alternatives: rquickjs' `AsyncRuntime` on a shared executor (all plug-ins
would share the lock and a runaway interrupt would stall others' futures),
one thread for all runtimes (no isolation of stalls). Threads are cheap at
the expected plug-in counts.

### Turns and budgets

Every entry into plug-in code is a **turn**: arm the CPU deadline, call
the prelude's `dispatch` (or import the main module at start), drain the
promise jobs, disarm. `HostLimits` (defaults): activation 2 s CPU (every
turn until `activate` settled) and 30 s wall clock, 1 s CPU per call — a
command, a resolved API call, a dialog result, a UI action, a panel render,
a timer, a settings change (M4 hooks get the same per-call budget), 64 MiB
heap, 1 MiB JS stack (deep recursion throws a catchable `RangeError`),
64 pending API calls, 5 open dialogs, 100 timers, 20 notifications per
minute, 200 log lines per call, 1 MiB request / 5 MiB response for HTTP,
500 search results, 500 modules.

Crash policy (design: "a plug-in that throws or times out is skipped and
flagged"): the interrupt (uncatchable in QuickJS, so `try` cannot swallow
it) → `cpu-budget`; "out of memory" → `memory-limit`; an error escaping a
handler (synchronous throw or rejected promise of a command, panel, action,
timer or `activate`) → `error`. The thread ends, the slot becomes
`crashed` with a localized `plugin-stopped-<reason>` error, open dialogs
close, `PluginStopped` + `PluginsChanged` are emitted — before the waiting
caller gets its answer. A rejected promise nobody handles (a floating API
call) is only logged (`plugin-host-unhandled-rejection`). Refused UI trees
and denied calls reject the plug-in's promise; they are not crashes.

### The sandbox and the plug-in API (`prelude.js`)

`Context::full` gives the language and its standard objects; QuickJS's
`std` / `os` modules are never registered. The module loader resolves only
the manifest's `main` and relative `./` / `../` imports inside the package
(`.js` / `.mjs`, safe paths); bare specifiers (`os`, `node:fs`, `lodash`)
fail with "bundle dependencies". Plug-ins see no `require`, `process`,
`fetch`, `XMLHttpRequest` or each other's globals (separate runtimes).

A prelude script builds the API over one native function
`hostCall(op, json) -> json` that stays in its closure (captured `JSON`
too). Globals (frozen, non-configurable): `nail`, `console`,
`setTimeout` / `clearTimeout`, `queueMicrotask`. The shape — mirrored by
WP63's `NailApi` in `@nail/plugin-api` — is:

- `nail.plugin {id, version}`, `nail.apiVersion`;
- `nail.mail.get(id)`, `getBody(id)`, `search(query, {limit})`,
  `thread(threadId)` → `PluginMessage` / `PluginMessageBody` (camelCase);
- `nail.ui.registerCommand(name, handler)` (also `nail.commands.register`
  and `ctx.commands.register`), `registerPanel(name, render | {render,
  onAction})` (also `ctx.panels.register`), `showDialog(spec, {onAction})`
  → `Promise<PluginDialogResult>`, `notify(spec | title)`,
  `updatePanel(name, tree)`; names must be declared in the manifest;
- `nail.storage.get / set / delete / keys(prefix) / clear / usage`
  (`usage` = `{keys, usedBytes, quotaBytes}`, camelCase like everything
  plug-in-facing);
- `nail.http.fetch(url, {method, headers, body})` → `{status, ok,
  headers, body, text(), json()}`;
- `nail.settings.get(key) / all() / onChange(fn)` (defaults overlaid with
  the valid stored values);
- `nail.log.debug / info / warn / error`, `console.*`.

API errors are `Error`s with `name = "NailError"` and `code` = the
`ErrorCode` wire name (`forbidden`, `not_found`, `invalid_request`,
`too_large`, `rate_limited`, `unavailable`, …). The entry module's default
export (an object with `activate(ctx)` / `deactivate()`, or a function) or
its named `activate` is started; `deactivate` runs (250 ms) on a graceful
stop.

### Permission gate

`Gate` holds the effective grants (declared permissions only;
`calendar.read` grants nothing; account lists only on the mail
permissions). Grants can change while a plug-in runs
(`PluginHost::update_grants`) and apply to the next call.

- `mail.read`: the gate expands virtual accounts through
  `PluginServices::expand_accounts`, passes the real accounts to
  `mail_search` and **filters every result again**; a message outside the
  grant is `not_found` (no probing).
- `mail.read.body`: needs `mail.read` too; the scope is the intersection;
  a message with `encrypted = true` is always `forbidden` (logged as a
  missing `mail.read.encrypted`) and `mail_body` is never called for it.
- `ui.dialogs`, `notifications`, `storage`: required for their calls.
- `network:<host>`: `https:` on the default port to a granted host
  (`http:` and other ports only for loopback hosts), no URL credentials,
  methods GET / HEAD / POST / PUT / PATCH / DELETE, no `Host`, `Cookie`,
  `Content-Length`, hop-by-hop or `Proxy-*` headers. The request reaches the
  core with the granted patterns (`HttpRequest::allowed_hosts`); the core
  must re-check redirects (`gate::host_allowed`) and refuse private
  addresses (SSRF, as `nail-proxy` does).

A denied call rejects with `forbidden` and writes an `api` log line with
`plugin-host-api-denied {call, permission}`. Command and panel contexts
are filtered: without `mail.read` nothing but the surface is passed; with
it, unreadable message ids are dropped (and the account / folder when they
are outside the grant).

### Talking to the core: `PluginServices`

An object-safe trait (boxed `Send` futures): `expand_accounts`,
`mail_get`, `mail_body`, `mail_search`, `mail_thread`,
`storage_get / set / delete / keys / clear / usage` (quota errors =
`too_large`), `http_fetch`, `emit(Event)` and `log(HostLogLine)` (the core
stores the line, numbers it and emits `PluginLogLine`). The host emits
`PluginsChanged`, `PluginStopped`, `PluginDialogOpened / Closed`,
`PluginNotification` and `PluginPanelChanged` itself. `emit` / `log` may be
called while the host holds a lock and must not call back into the host.

### Which UI: `client_id`

The `client_id` of `RunPluginCommand` / `PluginUiAction` /
`GetPluginPanel` is the turn's client. Every API call remembers the client
of the turn that made it and its resolution runs with that client again;
timers keep their creator's client. So a dialog opened after `await`s in a
command handler still carries the invoking UI's id; background work
(timers set in `activate`, settings changes) carries `null`.

### Dialogs and panels

`showDialog` validates the spec (title, ≤ 10 buttons with value ids),
inlines package images (`assets/x.png` → `data:` URI), clears host-filled
`messages`, runs `validate_tree` (issues are logged with the contract's
`plugin-ui-issue-*` ids and reject the call), registers the dialog
(`pd-<128-bit random>`) and emits `PluginDialogOpened` after filling
`message-list` rows with what the plug-in may read. `respond_dialog`
resolves the promise with the button (only a declared one, else `null`)
and the values of the tree's inputs of the right shape (others dropped),
and emits `PluginDialogClosed`. Body buttons call `onAction`. Stopping a
plug-in closes its dialogs. Panels render through the registered
function (a tree or a promise of one) with the same preparation;
`updatePanel` emits `PluginPanelChanged`.

### Packages: kept packed, verified by hash

`PluginPackage::read` checks an archive completely before anything is
installed: ≤ 20 MiB packed and unpacked (entries are read against a
running budget, so zip bombs stop at the limit whatever the headers
claim), ≤ 1000 entries, safe relative paths only (no `..`, absolute,
drive or `\` paths, no Windows device names or trailing dots), unique
ignoring case, no symlinks, no encrypted entries, stored / deflate only,
`manifest.json` at the top; manifest issues and missing `main` / locale
files are reported as `PluginManifestIssue`s for the install dialog.
Structural problems are `PackageError`s with `plugin-package-*` ids.

**Deviation from the stub's sketch** ("unpacking to
`<data-dir>/plugins/<id>/`"): `PackageStore` keeps the approved archive as
`<data-dir>/plugins/<id>.nailplugin` (written to a temp file, renamed into
place) and the host runs it from memory after comparing its SHA-256 with
the stored `package_sha256`. Nothing is ever extracted, so no archive path
can reach the file system, uninstall is one file, and tampering after the
approval is detected at start.

### Dev mode

`PluginHost::load_dev(folder, grants, settings)` reads the folder
(`PackageFiles::Folder`: safe paths, canonicalized, links out of the folder
refused), starts the plug-in and watches the folder
(`notify-debouncer-mini`, 300 ms; `node_modules` and dot folders ignored).
A change reloads it (`plugin-host-reloading`, stop reason `reloaded`,
manifest re-read); an invalid manifest is logged
(`plugin-host-reload-invalid` + one line per issue) and the plug-in stays
stopped until fixed; a disabled dev plug-in is not revived. Dev plug-ins
also log every API call and its outcome (`api` source, debug level).
`PluginHost::subscribe_logs` streams the lines for a console.

### Texts

Host lines carry `LocalizedMessage`s from `locales/*/core-plugins.ftl`
(`plugin-host-*`, `plugin-package-*`; the crate's catalog test checks both
directions for these prefixes). `PluginCatalogs` loads a plug-in's own
`.ftl` files (fluent-bundle) and resolves `%fluent-id%` manifest texts for a
language (tag → primary language → English → the id).

## Consequences

- WP64: implement `PluginServices` in `nail-core` over the store /
  search / an SSRF-safe HTTP client; create one `PluginHost` at start-up,
  start enabled plug-ins (`PackageStore::load` with the stored hash),
  forward the runtime requests, map `PackageError` / `DevLoadError`
  through `to_api_error`, persist crash errors from `PluginStopped`
  (`set_plugin_error`), resolve `PluginInfo` texts with `PluginCatalogs`,
  read the run state from `PluginHost::state`, update the stored manifest
  of dev plug-ins after a reload (`PluginHost::manifest`), and call
  `shutdown` (dropping the last host clone does it too).
- A plug-in that loops or hoards memory costs at most one budget of CPU
  and its heap before it is stopped; others never wait for it.
- Hooks (M4) reuse the per-call budget and the crash policy; write access
  goes through new gate checks and the operation queue.
