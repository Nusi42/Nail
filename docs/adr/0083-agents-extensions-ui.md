# ADR 0083: UI for confirmations and extensions — confirmation center, policies, veto dialog, plug-in banners / badges, composer / attachment / link extensions, MCP write settings, plug-in activity

Status: accepted · 2026-10-02 · M4a WP104

## Context

ADR 0079 fixed the M4a contract: confirmations and action policies
(`nail_api::agents`), MCP write scopes and rate limits, plug-in hooks,
extensions and jobs, 11 requests and 5 events. WP104 builds the UI against
it in its slots (`ui/src/features/{agents,plugins,mcp}/`,
`locales/*/{agents,plugins,mcp}.ftl`) with realistic mocks, while WP101–WP103
build the server side in parallel and WP105 wires everything.

## Decisions

### Confirmation center (`features/agents/`)

- **Attribution can't be faked.** Every confirmation and notice shows the
  kind of actor in Nail's own words next to the name — "AI agent “X” (MCP
  client)" or "Plug-in “X”", plus "for the AI agent “Y”" when a plug-in
  serves an MCP tool call. Names, target labels and before / after values
  are mailbox / actor content and are drawn as plain text.
- **Risk is visible and themeable.** `data-risk` = `destructive` (Trash,
  permanent delete, PIM deletes) / `outgoing` (send, invite) / `change` on
  cards, notices and the indicator; the approve button of a destructive
  confirmation is `data-variant="danger"` and reads "Approve and delete".
- **"Always allow" only where the contract allows it.** The checkbox is
  offered only when `can_always_allow`; otherwise the card says that
  deleting always asks. The request carries `always_allow` only together
  with `approve`.
- **Deciding happens only in the center**, never from a notice: notices
  ("something waits", effect `ConfirmationNotices`) offer "Review", which
  opens the `confirmation-center` dialog and focuses that confirmation.
  Notices disappear on `ConfirmationResolved` (decided elsewhere, e.g. by
  elicitation, or expired).
- A `conflict` answer (already decided / expired) is explained, and the
  lists refresh.
- **Widgets**: `confirmation-center` (option `show-history`, default true;
  pending oldest first, history newest first, paged by `before`, 50 per
  page) and `confirmation-indicator` (option `hide-when-empty`, default
  true; count, `data-state="waiting"`). Dialog `confirmation-center`
  (title `window-confirmation-center`), action `agents.open-center`.
- **Policies** (`PolicyEditor`): one radio pair (ask / without asking) per
  kind; picking the default sends `mode: null` so only changed defaults are
  stored (as the store does); destructive kinds are disabled with "Always
  asks". Mounted by the MCP client settings and the plug-in manager (only
  for plug-ins with a write permission, `PluginPermission::is_write`).

### Plug-in extensions (`features/plugins/`)

- `extensions.ts` mirrors the contract's matching rules (`matches_file`,
  `allows_host` for link host patterns, preview `maxBytes` capped at
  10 MiB) and only offers extensions of running plug-ins holding the needed
  grant (account-limited where `takes_accounts`); the core checks again.
- **Banners / badges** (`PluginBanners`, `PluginBadges`) share one query
  (`GetMessagePluginAnnotations`, cached a minute, under `["plugins"]` so
  `PluginsChanged` refreshes); each names its plug-in; banner buttons send
  `PluginUiAction` with the banner target. Badges carry a screen-reader
  description naming the plug-in.
- **Previews** (`AttachmentPreviewPane`): trees are drawn with the existing
  `PluginTree` read-only; images only as PNG / JPEG / GIF / WebP `data:` URIs
  with a base64 check (`previewImageSrc`), never SVG or HTML. Several
  renderers → a select. The pane names the plug-in.
- **Menus** (`ExtensionMenu`): one accessible menu button (ARIA menu
  pattern, arrows / Home / End / Escape / Tab) for composer actions,
  Markdown blocks, attachment and link actions; each item names its
  plug-in. Actions go out as `RunPluginAction` with this UI's context and
  client id; results arrive as events.
- **Composer edits**: `ComposerEditRequested` (for this UI or every UI) is
  queued per draft in the plug-in store; the composer applies it through
  `useComposerEdits(draftId, apply)` (`applyComposerEdit` for plain text)
  and shows who edited (`ComposerEditNotices`, a live region, also for MCP
  `edit_open_draft`). Markdown blocks are inserted as fences with the
  `<plugin>.<block>` info string (fence longer than any backtick run in the
  template); the composer preview draws them with `PluginMarkdownBlock`
  (source + "rendered by … when sent"), since HTML is made at send time.
- **Veto**: `sendDraftChecked(client, store, SendDraft)` replaces
  `request(SendDraft, "SendQueued")` in composers; on `SendVetoed` it shows
  the modal `alertdialog` (effect `SendVetoDialog`) and resolves with the
  outbox entry after "Send anyway" (`OverrideSendVeto`) or `null` for "Keep
  editing" (the default focus; Escape). An expired veto (`not_found`)
  leaves only "Keep editing" with an explanation.
- **Plug-in manager**: sections "Confirmations" (policy, write plug-ins
  only) and "Activity" (CPU `meter` against the hourly quota with
  `data-level` ok / high / exhausted, hook table, jobs with schedule,
  events, last run + outcome, next run, runs / failures, "Run now", and the
  full contribution list incl. MCP tool names `<plugin>__<tool>`). The
  install dialog lists every contribution kind and marks write permissions
  (`data-write`, "Can change your data"); `grants.ts` generalises the
  `mail.read` dependency to `PluginPermission::requires`.
- Structural CSS lives in `PLUGIN_BASE_CSS` (already compiled in) and
  `AGENTS_BASE_CSS` (for the integrator to append).

### MCP client settings (`features/mcp/`)

Writing scopes in their own section ("What it may change", ordered after
the reading ones), a rate-limit editor (number 1–10 000 or "no limit" per
limit, the contract's issue texts, "use the default limits"), the client's
`PolicyEditor`, and a note with a "Review" button when the client has
pending confirmations. `skip_draft_confirmation` stays hidden (ignored
since ADR 0079).

### Mocks

`extensionsMock.ts` now simulates the host's extension points
(`ExtensionBehaviour` per plug-in: open / before-send hooks, previews,
actions, jobs) with the core's checks (running, grant, declared
contribution, type / size), hook stats, job runs and CPU time;
`answerSendDraft` is `SendDraft` with before-send vetoes and veto expiry.
`extensionsSeed.ts` installs five demo plug-ins (attachment check,
link guard, file preview, snippets, daily digest); `agents/mockSeed.ts`
seeds pending and decided confirmations.

## Consequences

- WP105 / the integrator mounts the components (list in the plug-in and
  agents `index.ts` headers), routes `SendDraft` in `api/mock.ts` through
  `answerSendDraft`, appends `AGENTS_BASE_CSS`, adds theme rules and layout
  placements, gives the two widgets typed options in `widgets/agents.rs`,
  and adds Playwright scenarios.
- No new dependencies.
