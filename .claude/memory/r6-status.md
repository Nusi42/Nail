---
name: r6-status
description: "Wave R6 (one dialog pattern everywhere; release 0.8.10): what was integrated (ADRs 0239-0243), the shared helpers to use and the pitfalls met"
metadata:
  type: project
---

**Integrated (ADR 0239 contract, 0240 mail, 0241 rules / appearance / PIM, 0242 settings, 0243 data / security /
plug-ins), release notes "Nail 0.8.10":**

- Every dialog ends in `DialogFooter` (roles, order per viewer system), asks with `ConfirmDialog` (destructive: red
  verb, Cancel focused, Enter never runs it) and guards unsaved changes with `useCloseGuard` (prompt window
  `unsaved-changes:<key with _ for :>`). `FooterButton` has `ariaDisabled` / `describedBy` (focusable but marked:
  the rule editor's Save rule and Run while the rule has problems).
- Settings: every widget keeps its edits in `useSettingsForm` (`features/settings/draft.ts`), the agents feature's
  `PolicyEditor` too; questions with `askSettingsQuestion` (`features/settings/confirm.tsx`). The layout card's
  presets (Settings → Layout & toolbars) still apply at once with Undo, like the theme editor and the toolbar
  customizer (tool actions, scope decision of R6).
- Mail helpers: `widgets/DialogStates.tsx` (`EmptyDialog` = one Done, `useCloseNow`, the tag dialogs' guard and
  footer). `CloseButton` ("Close") is left only for the print view's toolbar and the plug-in manager's placements
  row.
- Shared texts: `common-next`, `common-remove-question`; gone: `common-apply`, `common-ok`, `common-undo-changes`,
  `common-save-access`.

**Tests to use:**

- Vitest: `renderSettings` / `saveSettings` / `saveSettingsDone` / `discardSettings` / `confirmSettingsQuestion`
  (`features/settings/testing.tsx`); windows with the guard through `withCloseGuard`.
- E2e: `saveSettings(page)` / `discardSettings(page)` / `answerQuestion(page, verb)` (`e2e/support/settings.ts`),
  `pickTheme` (`e2e/support/appearance.ts`), `openMailAs`, `unsavedPrompt`, `emulateViewerOs`,
  `expectDialogFooter` (`e2e/support/windowTour.ts`).
- The a11y window sweep runs each mode as another system (default Windows, high contrast Linux, dark mode macOS):
  `expectDialogPattern` checks every visited window's ×, its footer order, no focused destructive button and
  `TOUR_FOOTERS` (roles and focus of the known dialogs); its `close` uses Esc. A new dialog in the tour with a
  fixed focus belongs in `TOUR_FOOTERS`.

**Pitfalls:**

- Removing a `defaults/theme.kdl` part makes `nail-config defaults::default_theme_matches_the_fixture` fail until
  `NAIL_UPDATE_SNAPSHOTS=1 cargo test -p nail-config --test integration defaults::` regenerates
  `crates/nail-api/fixtures/default-theme.json`; regenerate it once after rebasing, never merge it by hand.
- `ui/src/i18n/common.test.ts` fails on a `common-*` text no code uses any more; remove it with its last user.
- New confirmation windows go into the `CONFIRMATIONS` set of `ui/src/features/composer/instances.test.tsx`.
- Under heavy machine load, e2e specs such as m5d "customize layout", a11y-windows' connection-log step and
  r2-rules-window timed out in parallel runs and passed alone.
