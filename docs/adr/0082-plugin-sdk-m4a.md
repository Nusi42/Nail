# ADR 0082: plug-in SDK for the full API, scaffolder templates, examples, plug-in and MCP user docs

Status: accepted · 2026-10-02 · M4a WP103

## Context

Plan section 9, WP103: update `@nail/plugin-api` for the M4a contract
(ADR 0079) — generated types, the JSX runtime, typed hook helpers; the
scaffolder's templates for hook / composer / attachment plug-ins; the
design's full "Follow up" example (creates a task), a before-send checker
and a CSV preview example, each with build + pack scripts and tests
against the SDK test host; `userDocs/en/plugins/` for every extension point
and permission; `userDocs/en/mcp.md` for write tools, confirmations,
policies, prompts and rate limits. WP102 wrote the host (prelude.js) in
parallel; the contract fixes the Rust types but not the JavaScript
surface.

## Decisions

### The runtime API follows WP102's prelude

`src/api.ts` is still hand-written to mirror `crates/nail-plugins/src/prelude.js`
(ADR 0053). It was written against the contract first and then aligned
with WP102's prelude as it appeared in the working tree:

- **Writes** on `nail.mail` (`update`, `delete`, `createDraft`,
  `updateDraft`, `send`), `nail.calendar` (`calendars`, `createEvent`,
  `updateEvent`, `deleteEvent`, `respond`), `nail.contacts`
  (`addressBooks`, `find(email, {limit})` → list, `upsert`, `delete`),
  `nail.tasks` (`lists`, `create`, `update`, `delete`). Arguments are the
  camelCase forms of the MCP write tools' argument structs
  (`nail-plugins/src/writes.rs`, `deny_unknown_fields`); message ids may be
  one id or a list. Every write resolves with `PluginWriteResult` = the
  contract's `McpWriteResult` in camelCase (`done {opIds, created,
  confirmationId?}` / `denied` / `expired` / `vetoed {reasons}`) — a denied
  or expired confirmation is an answer, not a rejection. Collections are
  `{id, name, default, readOnly}`; contacts `{id, addressBookId, fullName?,
  emails, organization?}`.
- **Tasks have no reminder field**: `UpsertTaskArgs` has none and the host
  refuses unknown fields. A task links to one message (`links:
  [message.uri]` or `relatedMessageId`); `list` / `listId` pick the list.
- **Registrations** exist on the activation context and on the global
  (`nail.hooks.register(kind, handler)`, `nail.hooks.onBannerAction`,
  `nail.composer.registerAction` / `registerBlock`,
  `nail.attachments.registerPreview` / `registerAction`,
  `nail.links.registerAction`, `nail.jobs.register` (the contract's
  `ctx.jobs.register`), `nail.mcp.registerTool`); `ctx.l10n` too.
- **Handler inputs**: arrival / open hooks get `{message, body?}`
  (`PluginHookMessage`), before-send the `PluginOutgoingMessage`, banner
  presses `{messageId, bannerId, buttonId}`, composer / attachment / link
  actions `(PluginActionContext, bytes?)`, previews `(PluginAttachmentRef,
  bytes)`, Markdown blocks `PluginMarkdownBlockInput`, jobs
  `PluginJobTrigger`, MCP tools the arguments object. A throw fails only
  that call.
- **Composer edits** `nail.composer.insert / replaceSelection / replaceAll(text,
  {draftId?, format?})` (default draft: the latest composer action's).
- **`nail.l10n`** `{locale, t(id, args?), has(id)}`; **binary fetch**:
  request bodies `string | Uint8Array | ArrayBuffer`, responses `bytes()` /
  `arrayBuffer()`.

### `definePlugin` with declared handlers

`definePlugin({activate?, deactivate?, commands, panels, hooks: {arrival,
beforeSend, open, onBannerAction}, composer: {actions, markdownBlocks},
attachments: {previews, actions}, links, jobs, mcpTools})` registers every
declared handler (host context first, the global `nail` as fallback)
before `activate` runs, so plug-ins can be written as data. A host without
a registry (an M2d Nail) answers `unsupported` naming `ctx.<registry>` only
when such a handler is used. Preview renderers may return JSX / nodes; the
SDK wraps them as `{kind: "tree"}` (`toPreview`).

### "JSX runtime for new components"

The contract added no node types; the M4a surfaces that take component
trees are previews. The runtime's node table is now a `Record` over the
generated `PluginUiNode["type"]` plus a compile-time check against
`UiIntrinsicElements`, so a component added in Rust fails to compile in
the SDK until the runtime knows it. `previewTree` / `previewImage` /
`banner` / `badge` / `veto` / `addContent` / `mergeBeforeSend` build the
other answers; `arrivalProblems` / `beforeSendProblems` / `openProblems` /
`previewProblems` / `markdownBlockProblems` mirror the host's `problems()`
/ `image_ok()` (the host stays authoritative).

### Sandbox helpers

QuickJS has typed arrays but no `TextDecoder`, `TextEncoder`, `atob` or
`btoa`. `src/bytes.ts`: a WHATWG-exact UTF-8 decoder (checked against
`TextDecoder` on random bytes), `isUtf8`, `decodeLatin1`, `decodeText`
(UTF-8 else Windows-1252), `encodeUtf8`, `toBase64` / `fromBase64`.

### Test host

`createTestHost` grew with the host's rules: all 18 permissions,
account-limited grants (`accounts`), writes with the default policies
(destructive kinds always ask, send / invite / rule ask by default) and a
scripted user (`confirm` → approve / deny / expire; `policies` changes),
other plug-ins' vetoes (`sendVetoes`), records (`writes`, `confirmations`,
`composerEdits`, `messages` updated by `nail.mail.update` and arrival
actions, `drafts`, `tasks`, `calendarEvents`, `contactsById`); hooks with
budgets from the manifest (`runArrival` / `runBeforeSend` / `runOpen` →
`{outcome: ok | vetoed | failed | timed-out | skipped, result, problems}`,
`clickBanner`); `runComposerAction` (edits only on the action's draft),
`renderMarkdownBlock`, `renderPreview` (manifest `maxBytes`, answer
checks), `runAttachmentAction`, `runLinkAction`, `runJob` (wall-clock
limit), `callMcpTool`; `nail.l10n` over given catalogs with a small Fluent
formatter (`src/l10n.ts`: messages, terms, multi-line values, variables,
literals, references, select expressions with `Intl.PluralRules`; no bidi
isolation marks; `filter` of hooks is not evaluated). `loadCatalogs(dir)`
in the tools reads a plug-in's catalogs.

### Tools

`checkManifest` now mirrors `PluginManifest::validate` for M4a:
`PERMISSIONS` (18) / `FUTURE_PERMISSIONS` (4), `missing-required-permission`,
`contribution-needs-permission`, hooks, extensions, jobs, MCP tools, counts
(incl. locales), with the host's paths and "only valid entries count for
duplicates". `tools/cron.mjs` ports `CronSchedule::parse` (same issues:
`field-count`, `invalid-field`, `too-frequent`) and adds `cronMatches` /
`nextCronRun`. `PREVIEW_PERMISSIONS` is gone (private package).

### Docs generator

`scripts/gen-docs.mjs` gained sections (writing data, mail hooks,
composer, attachments and links, background jobs, MCP tools, bytes, test
types, the M4a manifest types) and labels union variants by their tag
(`type`, `kind`, `status`).

### Scaffolder templates

`--template command | hook | composer | attachment`. `template/` stays the
shared base (and the `command` starter); `templates/<name>/` replaces
`manifest.json`, `src/index.tsx` and `locales/en.ftl`. The new templates use
`nail.l10n` for every text.

### Examples

- `follow-up` 2.0: creates a task (`tasks.write`, linked to the message,
  list chosen in the dialog, read-only lists left out), remembers the
  follow-up in storage, a `remind` job (`@hourly` + `app.started`,
  `background`) notifies once when one is due (the "reminder" — tasks have
  no reminder field), the panel lists follow-ups; denied / expired writes
  create nothing; every text from `locales/en.ftl`.
- `attachment-check`: a before-send veto when the subject or own text (no
  signature, no quotes unless set) mentions an attachment but none is
  attached; keyword lists per language in `en.ftl` / `de.ftl`
  (`attachment-keywords`, `*` = prefix) plus the user's words; Unicode
  word boundaries, case-insensitive (`iu`).
- `csv-preview`: RFC 4180 parsing, delimiter detection, UTF-8 or
  Windows-1252, header row and row limit settings (≤ 499 rows, ≤ 50
  columns, cells ≤ 500 characters), a `<table>` tree. No PDF preview:
  rendering PDF in QuickJS needs a large dependency; the plan's "PDF/CSV"
  is met with CSV (the task named CSV).

Every example uses the template's scripts and tsconfig (tested), passes
the manifest check, has every `%id%` and every `t("…")` id in `en.ftl` and
the same ids in other catalogs (tested), and is built only into temp
folders by the tests.

### User docs

`userDocs/en/plugins/`: README (all extension points, manager additions),
permissions (all of them, account limits, what plug-ins never do),
security, writing-data, hooks, composer, attachments-and-links,
background-jobs, mcp-tools, localization; manifest and getting-started
extended; references regenerated. `userDocs/en/mcp.md`: writing
permissions, the write tools and whether they ask, results, idempotency,
confirmations (center, elicitation, headless bridge, 20 pending),
per-client policies, prompts, limits, plug-in tools. Wording follows the
UI catalogs as of WP104's working tree (Confirmations, Approve / Deny,
Ask first / Without asking, Limits).

## Consequences

- `e2e/tests/plugins.spec.ts` (integrator) still expects the M2d
  follow-up (storage reminder, "Save reminder", four permissions); WP105
  updates it to the task flow with the M4a scenarios.
- `api.ts` must keep following `prelude.js`; differences found while
  aligning are listed for WP105.
- No new dependencies.
