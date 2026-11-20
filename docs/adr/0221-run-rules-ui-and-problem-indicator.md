# 0221 — Running rules from the rules window and menus; one compact problem indicator

Status: accepted (2026-11-20)

## Context

ADR 0219 set the contract for running rules on demand (`RunRules`, one
undoable run per rule) and for live search. The maintainer asked to apply a
rule right where it is written, on the selected mail or folder, and to stop
errors from pushing the rules window and the search field around. Both the
rules window and the search field needed a compact way to show a mistake.

## Decision

- **Run panel in the rules window** (`ui/src/features/rules/RunPanel.tsx`,
  under the rule list): it runs the rule being edited, or the rules checked in
  the list (a checkbox per row, `picked` in the rules store), on one target —
  each rule's own mail (what "Run now" always did), the folder the list shows,
  or the selected messages (a selected conversation stands for all its
  messages in that folder). A target that is not available is disabled with
  the reason. The outcome line ("N rules ran: X messages matched, Y changes
  queued.") always takes its line, so a result moves nothing; Undo takes back
  every run of the last click (`runTargets.ts`).
- **"Run rule" in the row and folder menus.** The layout names one item,
  `rules.run-menu` (an action with its icon in `defaults/layout.kdl`). The
  rules are the user's data, not layout, so the action replaces the open
  context menu in place with a menu built at run time (`runMenu.ts`, through
  `menuStoreFor().show`): "All rules that are on" (with several), the rules
  that are on, then the rules that are off. `rules.run` runs the chosen rule
  on the menu's target; a toast (`RunToast.tsx`, wearing the shared
  `mail-actions.toast` parts) reports the outcome with Undo.
- **One problem indicator** (`ui/src/widgets/ProblemIndicator.tsx`, parts
  `problem*`, look in the shared skin's `PROBLEM`): a small icon, with an
  optional short label, whose details float over the content instead of
  taking room. In *toggle* mode (the rules window: core errors, text-view
  errors with line and column, missing pieces, run errors) it is a button;
  its hover text and a native popover show the details, which are also the
  button's description. In *follow* mode (the search field, ADR 0220) the
  host opens the details while the field has hover or focus, and the field
  names them in `aria-describedby`. The popover is anchored under its button
  where the engine supports CSS anchor positioning, else the browser centres
  it.
- **The new rule's text round-trip.** The core no longer writes `id=""` for a
  rule without an id (`render_rule`), so a new rule switched to Text, edited
  and switched back reads again; the UI also skips re-reading text that was
  not edited.

## Consequences

- A "Run rule" hover submenu would need a generic dynamic menu source in
  `features/menus`; the in-place menu avoids it for now. A rule whose name
  starts with `@` shows as a missing message id there, because menu labels
  read a leading `@` as a Fluent id.
- The line and column of broken rule text are in the indicator's details,
  not inline; the redesign of the rules window (including the misplaced
  Test rule results) is a later wave.
- Themes style the indicator once (`problem*`); feature parts inside the
  details (`search-bar.error*`, `rules.issues.items`) keep their names.
