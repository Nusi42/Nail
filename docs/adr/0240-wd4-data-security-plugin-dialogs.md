# 0240 — R6 WD4: data, security, plug-in and system dialogs

Status: accepted (2026-11-22)

## Context

ADR 0239 sets one dialog pattern and leaves the feature dialogs to the
packages. WD4 owns the portability wizards (import, export, back up,
restore, the diagnostics bundle, the job report), the recovery key, the
smartcard PIN prompt, the Autocrypt import, pairing and re-authentication,
the plug-in install wizard, the plug-in manager and plug-in dialogs, the
connection log, error details and the application log, the confirmation
center, the command palette, About Nail and the first-run screen. Some
cases were open after the contract:

- A wizard's job step: the job runs in the core and goes on when the
  window closes; its progress view has its own Cancel (it cancels the
  job).
- Plug-in dialogs: the plug-in API marks only `primary`; many plug-ins
  (the documented example included) add their own `{ id: "cancel" }`.
- The connection log's Clear and removing a plug-in or the recovery key
  cannot be undone by the core.
- The first-run screen had Get started and Skip, which did the same; ×
  left it to come back at the next start.

## Decision

- **Wizards** end every step in `DialogFooter`: the step's verb as the
  primary, Back as the secondary, Cancel. The job step and every result show
  only Done (`done`): the job keeps running in the background and stays in
  the job lists, so closing is safe; the progress view's own Cancel still
  cancels the job. The diagnostics bundle becomes a wizard (options → Next
  → the exact preview with Save bundle / Back → the saved file with Done).
  "Restore now" is a destructive primary: red, never a submit button, and
  the plan step focuses Cancel.
- **Information windows** keep × only: the job report, error details and
  the application log lose their footer Close.
- **Confirmations without Undo**: Clear… of a connection log
  (`connection-log-clear`), Remove plug-in… (`plugin-uninstall`) and
  Remove recovery key (`security-recovery-remove`) open a destructive
  `ConfirmDialog` window; the pending target is kept per UI store and
  forgotten however the question ends (like "Delete permanently?"). The
  inline "Are you sure" rows go.
- **Plug-in dialogs** keep the API: the first `primary` button is the
  primary role, every other button secondary (the first one in the
  footer's secondary slot, further ones between its start and end). Nail
  adds Cancel, which answers `button: null` like × and Escape. A
  non-primary plug-in button whose id is `cancel` (any case) is that
  Cancel: it keeps the plug-in's label and answers `"cancel"`, so existing
  plug-ins show one Cancel and see the same result as before. The body
  scrolls inside a new `plugin.dialog.form` so the footer stays at the
  bottom; `plugin.dialog.buttons` / `plugin.dialog.button` go.
- **Pairing**: Create code and Cancel; the code step shows Revoke code (red,
  beside the code, no question: a pending code is cheap to make again) and
  only Done. The settings list of pending codes (WD3) keeps "Cancel code".
- **Re-authentication and the login page** share the sign-in steps, so both
  use `DialogFooter` (the login page without Cancel); the step's primary
  takes the focus when a step replaces another.
- **The install wizard's placements step** uses the footer with "Not now"
  as its Cancel; Done or Not now close the wizard. In the plug-in manager
  the same component keeps its inline row (it is a section of a list, not a
  window's end).
- **First run**: one Get started; closing the window (× or Esc) completes
  the first run too (`CompleteFirstRun`), unless the window only
  re-mounted.
- **PIN prompt**: "Unlock" (the security catalog's existing verb).

## Consequences

- Plug-in authors read in `userDocs/en/plugins/components.md` that Nail
  orders their buttons and adds Cancel; the generated API reference keeps
  its wording until the integrator updates the doc comments in `nail-api`.
- `WizardButtons` (`features/portability/Fields.tsx`) stays only for the
  print dialog (WD1); `wizard.buttons` leaves the theme once that moves.
