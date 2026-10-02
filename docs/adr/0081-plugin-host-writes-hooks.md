# ADR 0081: plug-in host — writes, hooks, extensions, background jobs

Status: accepted · 2026-10-02 · M4a WP102

## Context

WP102 fills `crates/nail-plugins` against the M4a contract (ADR 0079): plug-in
writes behind the new permissions, mail hooks with time budgets, composer /
attachment / link / MCP extensions, background jobs with a CPU quota,
`nail.l10n`, binary `nail.http.fetch` bodies and panel action values checked
against the rendered tree. The crate still depends on no store, core or
network: everything goes through `PluginServices`, which nail-core implements
in WP105. Every new trait method has a default (`unsupported` or a no-op), so
the core compiles unchanged until then.

## Decisions

### Writes: one path, the MCP tools' argument types

- `nail.mail.update / delete / createDraft / updateDraft / send`,
  `nail.calendar.createEvent / updateEvent / deleteEvent / respond`,
  `nail.contacts.upsert / delete`, `nail.tasks.create / update / delete`
  parse the plug-in's camelCase arguments (unknown fields refused) into
  `writes::PluginWrite` — the `nail_api::mcp::tools` argument structs
  (`UpdateMessagesArgs`, `CreateDraftArgs`, …) plus three PIM deletes MCP has
  no tool for. The core can run plug-in and agent writes through one
  implementation.
- `PluginWrite::kind()` gives the `ActionKind` whose policy applies (Trash /
  permanent delete / PIM deletes always ask; invitations when an event write
  mails attendees or an answer is sent), `permissions()` what the gate checks
  (all of them, e.g. invitations need `calendar.write` and `mail.send`).
- The gate refuses a missing permission (`forbidden`, logged); messages a write
  names must be readable (`mail.read`), those it changes within
  `mail.organize` ∩ `mail.read`, attachments it copies within
  `mail.read.body` and never end-to-end encrypted. Then
  `PluginServices::write(PluginWriteRequest {attribution, kind, operation,
  write, scope})` runs it; `scope` is the account set of the mail permission
  (organize scope, `mail.send` scope for drafts / sending / invitations, all
  accounts for PIM writes) the core must stay inside.
- The plug-in gets the core's `McpWriteResult` as camelCase
  (`{status: "done", opIds, created, confirmationId?}`, `denied`, `expired`,
  `vetoed {reasons}`); a denied or expired write resolves, it does not throw.
- Attribution: `ActionAttribution {actor: plugin, name: the manifest name in
  the core's language, on_behalf_of}` — `on_behalf_of` is the MCP client while
  the plug-in serves one of its exposed tools (the turn's *origin*, carried
  through pending calls, timers and dialogs like the UI client id).
- Collections: `nail.calendar.calendars()` (`calendar.write` or
  `calendar.read`), `nail.contacts.addressBooks()` / `find(email)`,
  `nail.tasks.lists()` (the write permissions include listing what they write
  to). Types `PluginCollection` / `PluginContact` live in this crate for now
  (apiRequest: move them to `nail_api::plugins` for generated SDK types).

### Handlers registered in code: one "invoke" mechanism

Hooks, banner buttons, composer actions, Markdown blocks, attachment previews
and actions, link actions, jobs and MCP tools are registered in code
(`ctx.hooks.register(kind, fn)`, `ctx.hooks.onBannerAction(fn)`,
`ctx.composer.registerAction / registerBlock`,
`ctx.attachments.registerPreview / registerAction`, `ctx.links.registerAction`,
`ctx.jobs.register`, `ctx.mcp.registerTool`; the same on `nail.*`). Each must
be declared in the manifest (else `invalid_request`); the grant is checked when
the host calls it, not at registration, so a plug-in whose permission was not
granted still activates.

The host calls a handler with `ThreadMsg::Invoke`: the handler gets its input
(and bytes as a second `Uint8Array` argument), and its awaited result or its
error goes back with `invokeResult`. **A throw never stops the plug-in** (unlike
a command handler): hooks that fail are skipped and counted. The waiting side
applies the wall-clock limit (hook budget, `reply_timeout`, job wall clock, MCP
tool timeout); invocations nobody waits for any more are dropped before they
run and forgotten while pending. A handler that loops is still stopped by the
per-turn CPU budget, which stops its plug-in (`cpu-budget`) as before.

### Hooks (`PluginHost::arrival_hooks / before_send_hooks / open_hooks`)

- All plug-ins' hooks run at once (one Tokio task each, their own threads);
  results are ordered by plug-in id. The calls never fail: the report lists
  each run (`HookRun`: outcome ok / vetoed / failed / timed-out, time, error).
- A hook applies when the plug-in has the hook's permission (arrival / open:
  intersected with `mail.read`) for the message's account and its `filter`
  matches (`PluginServices::message_matches`; an error skips it). Arrival and
  open hooks get `{message, body?}` (body only with `mail.read.body`, never
  end-to-end encrypted); before-send gets `PluginOutgoingMessage`.
- Budget = the manifest's `timeoutMs` or the kind's default.
- Answers are validated (`problems()`, arrival: ≤ 20 actions, non-empty tags);
  an invalid answer is a failure. Every call is recorded
  (`PluginServices::record_hook_call`); failures are logged at most once a
  minute per plug-in and hook (`plugin-host-hook-failed` / `-timed-out`).
- Arrival actions become one `UpdateMessagesArgs` (later actions win) and go
  to `PluginServices::write` (kind `organize`, operation `hooks.arrival`,
  scope = the message's account) **without being awaited**: delivery never
  waits for a confirmation. Without `mail.organize` for the account they are
  dropped and logged (`plugin-host-hook-actions-denied`).
- Before-send: vetoes name the plug-in (`PluginSendVeto`); headers and added
  content per plug-in; added HTML is sanitized with `nail_mime::sanitize_html`.
  The core keeps the veto map and "Send anyway" (WP105).
- Open: banners / badges with the plug-in's name; the host remembers which
  banner buttons each plug-in showed per message (bounded, 1000 pairs), and
  `ui_action` with `PluginUiTarget::Banner` is accepted only for those (else
  `not_found`); the plug-in's `onBannerAction` gets `{messageId, bannerId,
  buttonId}`. What an open hook raises goes to the UI that opened the message.

### Extensions

- `run_action(action, target, client_id)`: composer actions (need
  `composer`; get `{action, composer: {draftId, selection, mode}}` with the
  mode from `PluginServices::draft_mode`), attachment actions (`attachments`
  for the account; bytes ≤ 10 MiB; type / extension must match), link actions
  (no permission; `http(s)` / `mailto` only, host patterns as for `network:`;
  the message id only when the plug-in may read it). Returns once the plug-in
  got the call; failures are logged (`plugin-host-handler-failed`).
- Composer edits `nail.composer.insert / replaceSelection / replaceAll(text,
  {format?, draftId?})` only on a draft a composer action of this plug-in ran
  on in the last 5 minutes (`HostLimits::composer_edit_window`); default draft
  = the latest of the current UI. They emit `ComposerEditRequested` with the
  plug-in's attribution and the action's UI client id. Not a store write (the
  composer applies them like typing), so no confirmation.
- `render_attachment_preview`: `attachments` for the account, never encrypted
  mail, the renderer's `maxBytes`, type / extension match. A tree answer goes
  through `prepare_tree` (validated like a panel, package images inlined) and
  `fill_tree`; an image answer must be PNG / JPEG / GIF / WebP base64 of
  ≤ 5 MiB (a returned `Uint8Array` is converted to base64). Anything else
  (`kind: "html"`, SVG) is `invalid_request`.
- `render_markdown_block(info, source, purpose)`: `composer`; HTML ≤ 256 KiB,
  sanitized with `nail_mime::sanitize_html` (remote images dropped, scripts
  removed); `not_found` when no running plug-in renders it.
- `mcp_tools()` lists tools of running plug-ins with `mcp.expose` granted
  (`<plugin>__<tool>`, description, schema — default an empty object schema —,
  scopes); `call_mcp_tool(name, args, client)` checks the arguments against
  the schema's top level (object, `required`, declared property types,
  `additionalProperties: false`), passes **the arguments themselves** to the
  handler (aligned with the SDK), limits the result to 1 MiB and 60 s.

### Background jobs

- Schedules use `CronSchedule` from nail-api; `jobs::next_run` walks local days
  (zone from `PluginServices::local_time`, clock from `PluginServices::now`, so
  the core's adjustable clock drives them; DST gaps skipped). A scheduler task
  per host ticks every `job_tick` (15 s); `PluginHost::run_due_jobs()` checks
  at once (the core calls it after moving its clock).
- At start (or when `background` is granted later) the stored next runs are
  loaded (`job_states`); a schedule missed while Nail was not running runs
  once, then the next run is stored (`set_job_next_run`).
- Events (`deliver_event`) are collected per job for `event_coalesce` (2 s)
  into one run; message ids only for plug-ins with `mail.read` that may read
  them (others still run, without ids). `app.started` is delivered by the host
  when a plug-in starts.
- A job never runs twice at once (`overlap` recorded; `run_job` answers
  `conflict`); a run ends at the wall-clock limit (60 s, pending promises
  included; `timeout`); a throw is `error`. Every run is recorded
  (`record_job_run`) and announced (`PluginActivityChanged`).
- **CPU quota.** The host measures the duration of every turn of a plug-in
  (QuickJS turns are synchronous on its own thread, so this is its CPU time
  plus scheduling noise) in a per-minute meter kept across restarts; over
  `cpu_quota_per_hour` (120 s) in the last hour its jobs are skipped
  (`cpu-quota`, `run_job` answers `rate_limited`) until the hour has passed.
  Hooks, commands and UI calls keep their per-call budgets and are not
  skipped. Usage is reported to the core about once a minute
  (`record_cpu`). `PluginHost::cpu_last_hour` / `job_running` /
  `job_next_run` feed `GetPluginActivity`.

### `nail.l10n`, binary bodies, panels

- `nail.l10n.locale` (the core's language, `PluginServices::locale`, default
  `en`), `t(id, args)` and `has(id)` over the plug-in's own catalogs
  (`PluginCatalogs`, per-key English fallback, then the id).
- Bytes cross the boundary out of band: the prelude hands `Uint8Array` /
  `ArrayBuffer` / views to `hostPutBytes` and puts `{"$nailBytes": id}` in
  the JSON; bytes for the plug-in are stashed and taken with `hostTakeBytes`.
  Both stashes live for one turn (at most `max_blob_bytes`, 32 MiB, per turn).
  `nail.http.fetch` takes string or binary bodies (≤ `http_max_request_bytes`)
  and its response gains `bytes()` / `arrayBuffer()` (≤
  `http_max_response_bytes`, also checked in the host). Copying a typed array
  out of QuickJS needs rquickjs' `unsafe as_bytes`; one function
  (`copy_bytes`) allows `unsafe_code` and copies at once while no JavaScript
  runs (rquickjs' documented condition), like the other FFI exceptions in the
  workspace.
- Panel actions: the host remembers the inputs and buttons of the tree each UI
  was last shown (its render, or a later `updatePanel` for all UIs; without
  either, the panel's latest tree; bounded to 256 trees). A press of anything
  but a button of that tree answers `not_found`; values are cleaned to that
  tree's inputs like dialog values.

### Dependencies

`nail-mime` (workspace crate) for sanitizing plug-in HTML. No new external
crate; no licence change.

## Consequences / for WP105

- Implement the new `PluginServices` methods in `CoreServices`: `write` (policy
  → confirmation → op queue → audit, same code as the MCP write tools; the
  core must enforce `scope`), `calendars` / `address_books` / `task_lists` /
  `find_contacts`, `attachment` (never decrypted end-to-end content),
  `draft_mode`, `message_matches` (search language on one message), `locale`,
  `now` / `local_time` (the triage clock), `job_states` / `set_job_next_run` /
  `record_job_run` / `record_hook_call` / `record_cpu` (store v14 APIs).
- Call the host: `arrival_hooks` after the user's arrival rules (then
  `deliver_event(message.received, ids)`), `before_send_hooks` in `SendDraft`
  (vetoes → `SendVetoed`, keep the veto map; apply headers / additions to the
  outgoing message), `open_hooks` for `GetMessagePluginAnnotations`,
  `run_action`, `render_attachment_preview`, `render_markdown_block` (composer
  preview and send), `mcp_tools` / `call_mcp_tool` (nail-mcp), `run_job`,
  `deliver_event` (`message.sent`, `selection.changed`, `event.reminder`),
  `run_due_jobs` after clock changes, `cpu_last_hour` / `job_running` /
  `job_next_run` for `GetPluginActivity`.
- The host emits `PluginActivityChanged` for job runs only; for hooks the core
  decides (they run often).
