# 0240 — R6 WD3: settings drafts, settings questions and the Add account wizard

Status: accepted (2026-11-22)

## Context

ADR 0239 decided that nothing in a settings section applies until Save:
every section gets the pinned "Unsaved changes" bar, leaving it asks with
the shared prompt. The contract added the page's side (`SettingsSaveBar`,
`useSettingsDraft`, the page's close guard). The settings widgets still
used three models: switches and selects that wrote at once (Desktop app,
Updates, lookup and indexing), forms with a Save row of their own (folder
sync, junk filter, notifications, ManageSieve, vacation reply, smartcards,
identities, MCP grants and limits, remote access, remote users' access,
key trust, accounts, virtual accounts), and inline "Are you sure" rows
(remove an account, delete a virtual account, reset the junk filter,
revoke an MCP client, delete a key, encryption at rest, the remote access
actions).

## Decision

- **One form helper.** `useSettingsForm(saved, { save, canSave?, equal?,
  onSaved?, onError? })` (`ui/src/features/settings/draft.ts`) keeps a
  widget's edits apart from the saved value and registers them with the
  page (`useSettingsDraft`): `value`, `set`, `dirty` (the edit differs
  from the saved value, compared as JSON unless `equal` says otherwise),
  `discard` and `revision` (bumps on save and discard, a key for
  sub-forms with typed text of their own). `save` writes the value and
  resolves once the saved value shows it (the widget sets or refetches its
  query first); it throws, or resolves `false` when it already showed why,
  and the edits and the bar stay. Every settings widget uses it; no widget
  shows a Save, Apply or Undo row of its own, and each section writes once
  per widget on Save (one `UpdateAccount`, one `SetDesktopSettings`, …).
- **Validity** is live: a widget's problems show while editing and its
  `canSave` turns the bar's Save off (the prompt then offers Discard
  changes / Keep editing), instead of a check after pressing Save.
- **Another item inside a section** (account, virtual account, sync
  account or folder, junk / ManageSieve / vacation account, MCP client,
  remote user, key) asks like leaving the section: `useLeaveDrafts()` runs
  `askToLeave` with the page's guard. The page's guard is one hook,
  `useSettingsGuard(sectionName)`, used by the page and by the test host.
- **Actions are not settings.** Creating an MCP client, setting a remote
  password, enrolling TOTP, uploading Sieve scripts, sending a test
  notification and the like still act when chosen. Where they lose
  something they ask in the shared `ConfirmDialog`, in a modal
  confirmation window of their own kind: `askSettingsQuestion(ui, kind,
  question)` and `SettingsConfirmDialog`
  (`ui/src/features/settings/confirm.tsx`). A question has its text,
  detail, verb, `risk` (destructive by default: red, Cancel focused, Enter
  never runs it; turning encryption at rest on is a normal question) and
  an optional required check box (`confirmCheck`, "Also delete my secret
  key"). The verb runs `run`: the window closes when it succeeds, and
  stays with the error inline when it fails. Kinds: `remove-account`,
  `delete-virtual-account` (settings feature), `reset-junk-filter`
  (triage), `revoke-mcp-client` (mcp), `switch-vault-encryption`,
  `delete-key` (security) and `remote-confirm` (every `ConfirmButton` of
  the remote settings). Window titles are `window-<kind>`.
- **Add account wizard** (`widgets/AccountSetup.tsx`): every step's
  buttons are the shared `DialogFooter` (Continue / Test connection / Add
  account as the primary, Back as the secondary, Cancel); the result step
  shows only Done (it opens the new account). Once a name or address is
  typed, ×, Esc and Cancel ask; the wizard cannot save half an account, so
  the prompt offers Discard changes / Keep editing (a running OAuth
  sign-in is cancelled on discard).
- **Folder synchronization dialog** (`sync-folder`, from the folder menu):
  not a settings section, so it is a dialog of the pattern — Save writes
  and closes, Cancel / × / Esc ask with the shared prompt when changed.
- **Tests.** Vitest renders a settings widget the way the page shows it
  with `renderSettings` (`ui/src/features/settings/testing.tsx`: the bar,
  the page's guard and the window layer) and presses Save with
  `saveSettings` / `saveSettingsDone`; e2e specs press the bar with
  `saveSettings(page)` and answer questions with `answerQuestion(page,
  verb)` (`e2e/support/settings.ts`).

## Scope decisions

- Undo rows go: Discard (before Save) replaces "Undo changes" of the
  remote access form. Nothing that had an Undo after writing was a setting
  of these sections.
- Settings widgets of other packages are left to them (the appearance
  section's accent, the column / toolbar editors, the key bindings: WD2;
  plug-ins and portability: WD4). The agents feature's confirmation policy
  inside the MCP clients section (`features/agents/PolicyEditor`) still
  writes at once; it belongs to the agents feature and is a follow-up. The
  layout card's presets (layout editor) apply at once with their own Undo
  and are not drafts either.
- Remote access passwords, TOTP and passkey renames keep their own verbs
  (Set password, Turn on, Save name): they are credentials and actions,
  not settings that a Discard could take back.

## Consequences

- Switches and choices in Settings no longer act at once; e2e scenarios
  that changed a setting now press the bar's Save (`saveSettings`).
- Labels that open a question end in "…" (Remove account…, Delete virtual
  account…, Revoke…, Delete key…); the question's verb has none.
