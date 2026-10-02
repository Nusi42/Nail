# ADR 0084: M4a integration — agent and plug-in writes, hooks, extensions and jobs in the core

Status: accepted · 2026-10-02 · M4a WP105

## Context

WP101–WP104 built the MCP write tools, confirmations, rate limits and
prompts (`nail-mcp`, ADR 0080), the plug-in host's writes, hooks,
extensions and background jobs (`nail-plugins`, ADR 0081), the SDK,
examples and user docs (ADR 0082) and the UI for confirmations and
extensions (ADR 0083) against the M4a contract (ADR 0079). WP105 wires
them into the core, the UI, the demo and the end-to-end suite.

## Decisions

### One write path for agents and plug-ins

- `nail_mcp::ActorWrites` (new, `writes/actor.rs`) plans and runs a write
  for an actor that is not an MCP client with the MCP write tools' own
  plan builders: the same argument types (`ActorWrite` mirrors
  `nail_plugins::writes::PluginWrite`), the same confirmation view
  (summary, targets, before / after), the same Client API requests, so
  plug-in writes go through the operation queue and are undoable exactly
  like agent writes. The caller (the core) has checked the plug-in's
  permissions; the plan gets a synthetic grant with every write scope
  plus `read` / `attachments`, limited to the accounts the host passed in
  `PluginWriteRequest::scope` (so messages, folders, identities and PIM
  collections of other accounts are refused). The three PIM deletes MCP
  has no tool for (`nail.calendar.deleteEvent`, `nail.contacts.delete`,
  `nail.tasks.delete`) got plan builders (`PimDelete`, kind
  `pim-delete`, which always asks).
- Destructive kinds always ask; the rest follows the plug-in's action
  policy. A write that asks becomes a pending confirmation attributed to
  the plug-in (`operation` = the JS API, e.g. `nail.mail.delete`); a
  background task waits for the decision and runs it, so an approval
  given after the plug-in stopped waiting still runs exactly once. A
  denied / expired confirmation resolves the plug-in's promise with
  `denied` / `expired`.
- Plug-in writes have no MCP audit row (that table is per MCP client);
  each leaves a host line in the plug-in's log (`plugin-write-log`:
  operation and outcome, no mailbox content).
- `McpCore` (the core's `CoreAccess`) overrides `confirmations` (the core's
  broker), `render` (the core's locale) and `handle_write` (runs the
  request as the UI's own; who asked is recorded in the confirmation and
  the audit log — a per-operation attribution column in the queue would
  need a store migration and was not needed for undo).

### Confirmations: one broker in the core

- The core owns one `nail_mcp::broker::StoreBroker`
  (`Service::confirmation_broker`, created lazily, events through a weak
  service handle). The MCP server and plug-in writes share it.
  `DecideConfirmation` wakes it at once (and the forwarded
  `ConfirmationResolved` wakes the MCP server's copy, which is the same
  broker); the 500 ms store poll and the broker's own expiry stay as
  fallbacks.
- `RevokeMcpClient` and `UninstallPlugin` cancel the actor's pending
  confirmations (`ConfirmationResolved` for each; waiting calls answer
  `expired`) and remove its policies (`Service::forget_actor`).

### Hooks in the core's paths

- **Arrival:** `process_rule_inbox` runs `PluginHost::arrival_hooks` after
  the user's arrival rules for every new message not filed as junk (all
  messages of a batch at once; each hook has its budget and its actions
  go to the write path without being awaited), then
  `deliver_event(message.received, ids)`. Delivery never waits beyond a
  hook's budget; a looping hook is stopped by the per-turn CPU budget and
  the mail is already stored.
- **Before send:** `SendDraft` runs `before_send_hooks` when a host exists
  (`Service::send_draft_checked`). A veto answers `SendVetoed`; the veto
  (draft, send options, the other hooks' headers and additions) is kept in
  memory for 10 minutes (at most 200); `OverrideSendVeto` takes it once
  ("Send anyway") and sends with the original options. Without a veto (or
  on "Send anyway") added content is written into the draft (plain text
  always; HTML or Markdown in those modes; added HTML is sanitized by the
  host) and added `X-` headers are kept per draft in memory and prepended
  to the built message by the sender (`agents::add_headers`), then
  forgotten. A restart before the send loses the headers (the content is
  in the draft).
- **Open:** `GetMessagePluginAnnotations` runs `open_hooks` for the
  asking UI. Banner buttons go through `PluginUiAction` as before.
- Hooks are counted by the host (`record_hook_call`); the core announces
  `PluginActivityChanged` for a plug-in at most every 5 seconds.

### Extensions, jobs, events

- `RunPluginAction`, `RenderAttachmentPreview` and `RunPluginJob` go to the
  host. `GetPluginActivity` adds the host's live state: running jobs, the
  host's next run (else the stored one) and the CPU time measured since
  the last report.
- `CoreServices` implements every new `PluginServices` method: `write`,
  `calendars` / `task_lists` (from `ListCalendars`; the first writable one
  in the user's order is the default), `address_books` (the Collected
  book is read-only for plug-ins), `find_contacts` (`SearchContacts`,
  exact email match), `attachment` (raw message bytes, never for
  end-to-end encrypted mail), `draft_mode`, `message_matches` (the hook
  filter as a search within the message's folder, newest first, at most
  2,000 hits looked at), `locale`, `now` (the triage clock, so the
  controlled clock drives jobs) and the store v14 activity functions.
- Events: `message.sent` after a send (the Sent copy's id when Nail filed
  one), `selection.changed` when a UI reports a different selection / open
  message, `event.reminder` when reminders fire (no ids). `app.started` is
  the host's.
- `Service::clock_moved` runs due jobs at once; `nail serve`'s test
  controls `clock-set` / `clock-advance` call it, so e2e jobs run on the
  controlled clock without waiting for the host's 15 s tick.
- Markdown blocks: when a Markdown draft is built for sending, fences whose
  info string is `<plugin>.<block>` are replaced by the plug-in's rendered
  HTML (`render_markdown_block(…, Send)`; blank lines removed so the HTML
  block stays one block); blocks no running plug-in renders stay code
  blocks.

### MCP exposure of plug-in tools

- `McpData` / `CoreAccess` gained `plugin_tools` and `call_plugin_tool`.
  `tools/list` adds the tools of running plug-ins with `mcp.expose`
  (`<plugin>__<tool>`) for clients whose grant has every scope of the
  tool; `tools/call` checks the scopes, calls
  `PluginHost::call_mcp_tool` with the client as `McpClientRef` (writes
  the plug-in makes then carry `on_behalf_of`), wraps the answer in the
  untrusted-data envelope and audits the call like any tool.

### UI

- The mock routes `SendDraft` through `answerSendDraft`; both composers
  send with `sendDraftChecked` (veto dialog; "Keep editing" keeps the
  composer open). `AGENTS_BASE_CSS` is part of the compiled theme.
- Composer: `features/composer/pluginExtensions.tsx` mounts the plug-in
  toolbar (actions; Markdown blocks in Markdown mode), applies edits of
  plug-ins and agents (plain text and Markdown at the selection / end,
  HTML appended or replaced), shows who edited, and lists plug-in blocks
  under the Markdown preview.
- Reader: badges in the message header, banners above the body, an action
  menu per attachment, a preview pane per non-inline attachment a running
  plug-in renders. `LinkActionMenu` is not mounted yet: links live in the
  sandboxed reader frame (follow-up).
- Layout: `confirmation-indicator` under the status bar (both
  breakpoints; hidden while nothing waits), a `confirmation-center` tab in
  Settings, `g a` opens the center. Theme rules for every new part
  (risk colours, banners / badges by level, veto dialog, activity, rate
  limits). `widgets/agents.rs` has typed options (`show-history`,
  `hide-when-empty`).

### End-to-end

- The global setup builds all three examples (`buildExample`).
- `plugins.spec.ts`: the full Follow-up example creates a task (six
  permissions, write marked, toast, the task in the task window, the log
  line).
- `agents.spec.ts` (demo-multi): a draft without confirmation; a send
  approved in the center with "always allow", after which the next send
  runs at once; a denied Trash and a permanent delete that always asks,
  both changing nothing; `summarize_thread`; a rate limit saved in the
  settings that trips (audited as denied); axe on the center.
- `extensions.spec.ts` (demo-multi): the before-send example vetoes and
  "Send anyway" sends (axe on the veto dialog); an arrival hook tags mail
  (the keyword reaches the IMAP server) and an open hook's banner button
  reaches the plug-in; the CSV preview renders a table; an hourly job runs
  after `clock-advance` and "Run now" runs it again; a runaway arrival
  hook is stopped while mail keeps arriving; a composer action inserts
  text attributed to its plug-in.
- `a11y-windows.spec.ts` covers the `confirmation-center` dialog and the
  new settings tab.

## Consequences

- Agent and plug-in writes share plans, policies, confirmations and the
  operation queue; a fix in one applies to both.
- Vetoes and before-send headers are in memory: a restart within the
  10 minutes loses "Send anyway" (the user sends again) and headers of a
  scheduled send.
- Follow-ups: `LinkActionMenu` in the reader (needs a link bridge out of
  the sandboxed frame); the hook filter check is a folder search (a
  per-message matcher in `nail-search` would be cheaper); task reminders
  for plug-ins and agents (`UpsertTaskArgs` has no reminder field);
  `PluginCollection` / `PluginContact` and the camelCase write inputs as
  generated SDK types; a sealed detail column if the activity log should
  name subjects; MRTR request states survive no restart (ADR 0080).
