# 0223 — The redesigned rules window

Status: accepted (2026-11-21)

## Context

Wave R2 (ADR 0222) asked for every item of the 2026-11-20 design review of
the rules window: a fixed footer with one problem badge, "Run on" with Run,
Test and Save, a lower panel with Preview and Recent runs, a compact
WHEN / WHERE / IF / THEN form, confirmations inside the app, a window that
follows the screen, a segmented Editor / Text switch, an empty state with
starter rules, a list filter, drag reordering, syntax highlighting with line
numbers, and one look in Settings and in the window. The contract added the
draft read (`RuleFromText { draft }`), run targets, the shared `Picker` and
the `ValuesFromMessage` slot.

## Decision

- **Structure.** `RuleManager.tsx` (list, filter, empty state),
  `RuleEditor.tsx` (state and requests), `RuleForm.tsx` (header and the
  four rows), `TextView.tsx`, `LowerPanel.tsx`, `Footer.tsx`, `Confirm.tsx`,
  `runOn.ts` (the "Run on" choices), `describe.ts` (actions with values, run
  targets, why a run cannot be undone), `starters.ts`, `kdlTokens.ts`,
  `closeGuard.ts`. R1's run panel (`RunPanel.tsx`) is gone: its targets are
  the footer's "Run on" select. The window and the Settings card render the
  same component with the same `rules.*` parts.
- **Running several rules from the window is dropped.** R1's "Run" check
  boxes in the list are gone (the review's row is name / triggers / On); the
  "Run rule" menus keep "All rules that are on". The run panel's "checked
  rules" text and `picked` in the rules store went with them.
- **Test on the selection.** `TestRule` tests a folder or the rule's scope;
  for "Selected messages" the window tests the listed folder (up to
  `COUNT_LIMIT` = 1000 messages) and keeps the selected ones. A
  `TestRule.message_ids` would make this exact; it is a follow-up for the
  core. The question before a run that moves, archives or deletes mail
  counts the same way.
- **Unsaved changes.** The editor reports `dirty` to the rules store.
  `select` / `startNew` wait in `pending` while it is set; closing the
  window (its button, Escape, a command) is caught like the composer's
  close confirmation: a store subscription (`guardRulesWindow`, mounted by
  the feature's `effects`) puts the window back in the same update and
  asks. All questions are an `alertdialog` over the window whose content is
  `inert` meanwhile; the safe choice has the focus and Escape answers it.
- **Editor / Text.** The shared `Segmented` radio group (arrow keys move and
  switch). Switching back reads edited text as a draft, so an unfinished
  rule survives; text that does not read stays, with "Discard text
  changes". Switching to Text writes the newest rule (the race where typing
  during the switch was lost).
- **Syntax highlighting without a dependency.** A forgiving KDL tokenizer
  (`kdlTokens.ts`, ~100 lines) colours a hidden copy of the text under a
  transparent `textarea`; a gutter numbers the lines and marks the ones the
  core reports. Rejected: CodeMirror / Prism (licences fine, but far larger
  than six token kinds need, and an editor component would replace the
  plain textarea that screen readers and undo handle well).
- **"+ Add condition / action"** reuse the app's menu button
  (`features/pim/PimMenu`, the WAI-ARIA menu button pattern). It shows an
  icon only; a visible label needs an option on that component (follow-up).
- **Exchange badges.** One compact ⓘ per Exchange account with the number
  of reasons the rule stays in Nail; where it runs is its accessible name
  and hover text, the reasons its description, shown beside it on hover and
  focus by the rules skin.
- **Window size.** `layout.kdl`: `size="1100x760"` (the window manager fits
  it to smaller screens); the rules skin's container query stacks the list
  over the editor below 760 px.

## Consequences

- The footer is always in view (sticky in the Settings card), so Save, Run
  and the problems never scroll away; Test results no longer appear far
  below the buttons.
- Other scenarios that drive the rules editor use the radio group, the
  footer and the "+ Add" menus (`e2e/support/rules.ts`: `saveRule`,
  `addFromMenu`, `rulesFooter`). The window-manager scenarios
  (`e2e/tests/m5c.spec.ts`) use the rules window as their sample window:
  they start from its new size and shrink it before the edge checks; the
  responsive sweep (`p1-windows.spec.ts`) discards its unsaved sample rule.
- In Settings the page scrolls, so there the footer ends the editor instead
  of sticking (the page's bottom padding would show the form under it).
- The `defaults/` snapshots (nail-config fixtures) change with the theme and
  layout; the integrator regenerates them once for the wave.

## Integration

- The editor's `PickerField.tsx` became a thin adapter over the shared
  `widgets/pickers` `PickerField` (folder paths, a placeholder id, the
  problem badge named only while the field is invalid), so there is one
  labelled picker.
- "+ Add condition" / "+ Add action" show their name through the shared
  menu button's `showLabel` (`features/pim/PimMenu`) instead of a CSS
  `::after` with the aria-label.
- Picked values from the mail use `applyToCondition` / `applyToAction` of
  ADR 0225; the editor's own `withPickedValue` was dropped.
- The responsive skin's old rules entries (`rules.views`, the two-pane
  grid, `rules.add` as a field row) were removed: the window stacks through
  `features/rules/skin.ts`.
