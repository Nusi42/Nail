# 0228 — WR7: rule list multi-select, combined Exchange badge, values button

Status: accepted (2026-11-21)

## Context

ADR 0227 fixed the maintainer's rules window decisions of 2026-11-21: one
combined Exchange badge per rule row, "Values from this mail" right after a
condition's value field, and a multi-select rule list whose Run and Test act
on every selected rule. Building them raised a few questions the contract
left open.

## Decision

### The list is a listbox whose options hold nothing interactive

- The rule list is an `ol` with `role="listbox"` and
  `aria-multiselectable="true"`; each rule is an `option` (`aria-selected`,
  roving `tabindex`), named by its name and triggers (`aria-labelledby`).
- ARIA makes an option's children presentational, and axe's
  `nested-interactive` fails any focusable control inside one (even with
  `tabindex="-1"`). So the R2 row controls stop being controls:
  - the drag handle is a pointer-only `span` (`draggable`, `aria-hidden`);
    the keyboard reorders with Alt+Up / Alt+Down on the option (the R2
    "plain arrows on the handle" is gone: plain arrows now move the focus);
  - the On check box becomes the **On mark**, a pointer-only `span` with a
    check icon; the keyboard switches a rule in the editor header's On
    switch. The row's description says "Off" for a rule that is off;
  - the Exchange badge is no button: its lines are part of the row's
    description (`aria-describedby`), and its popover shows on hover and
    while the row has the keyboard focus.
- Rejected: a `grid` with `aria-multiselectable` (rows of cells holding
  real controls). It keeps the controls but needs a two-dimensional
  keyboard model (cells), and ADR 0227 named a listbox.

### Keyboard and pointer model

`ui/src/features/rules/selection.ts` (pure, shared by keys and pointer):
Click selects one rule; Ctrl+click (⌘) adds / removes one; Shift+click and
Shift+Up / Down extend from the anchor; Up / Down / Home / End move the
focus and select only that rule; Space adds / removes the focused rule;
Ctrl+A selects every listed rule; Escape goes back to the focused rule
alone (and goes no further, so it does not also close the window; with only
that rule selected Escape bubbles as before). The editor shows the focused
rule. A pick that moves the focus to another rule asks "Discard changes?"
first while the shown rule has unsaved changes; the selection changes only
with the answer (it waits in the store's `pending`).

- Run and Test act on the selection only when it has two or more rules that
  the filter lists; otherwise on the shown rule (so Ctrl+clicking the shown
  rule away leaves Run on the shown rule, as before).
- "Run on" is kept in the rules store (`runOn`), not per editor, so
  choosing the target and then picking more rules keeps the target. Opening
  the window from a message (`setOrigin`) starts it over from its default.

### Run, Test, Preview with several rules

As fixed in ADR 0227: one `RunRules` with the selected ids (file order);
"Run N rules" / "Save and run N rules" (the shown rule saved first when it
is one of them); a destructive run asks once with the summed preview count
of all selected rules ("matches", since a message matched by two rules
counts twice). Test sends one `TestRule` per rule on the same target; the
Preview has a total summary (it takes the focus), a one-line note that each
rule was tested on its own, then one group per rule (heading, summary,
"Apply to these N", matches). Save, Delete and the Text view act on the
shown rule only.

### Combined Exchange badge

- One ⓘ per rule; the number beside it is the **total number of reasons**
  the rule stays in Nail, summed over every Exchange account (not the
  number of accounts): it says how much there is to read, and with one
  account it is the R2 number. No number when the rule runs on the server
  everywhere.
- The popover has one line per account: "Contoso: in Nail" and its reasons.
  The list of lines has no name of its own: Chromium reads a named element
  in a description by its name instead of its content, so the row's
  description would say "Where this rule runs on Exchange accounts" and
  nothing else (`exchange-rule-badges-label` is no longer used).
  Its colour follows the most local placement over all accounts
  (server < client < not expressible), as R2 coloured each account.
- The popover lies under its row, as wide as the row (absolutely
  positioned; visually hidden, not removed, while closed), so opening it
  moves nothing and the list's edge never clips it.
- Each account's status comes from the existing `useExchangeRules` hook in
  a renderless probe per account that reports its line to the badge, so the
  query stays defined once (`exchange/queries.ts`).

### Values from this mail

`ValuesFromMessage` is passed into the condition's fields: right after the
text of a comparison (address, subject, body, header, mailing list), before
"Match upper and lower case"; after the operator while a header or list
condition has no text ("exists"); after the picker of tag, account and
folder conditions. Action rows are unchanged.

## Consequences

- Screen reader and keyboard users switch a rule on or off in the editor,
  not in the list; the list's row still tells them it is off.
- `exchange.rule-badges` (the R2 list of per-account badges) and
  `exchange.rule-badge.button` are no longer rendered; `exchange.rule-badge`
  is the one badge, `exchange.rule-badge.reason` a line of its popover.
- Parts added: `rules.list.name` (was `rules.list.select`),
  `rules.list.about`, `rules.list.off`, `rules.list.enabled`,
  `rules.test.note`, `rules.test.group`, `rules.test.group.title`.
- Strings: `rules-enable-rule` replaced by `rules-list-off` /
  `rules-list-toggle`; new `rules-run-many`, `rules-save-and-run-many`,
  `rules-confirm-run-many`, `rules-test-summary-many`,
  `rules-test-separately`, `rules-select-many-hint`.
