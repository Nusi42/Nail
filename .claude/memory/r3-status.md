---
name: r3-status
description: "Wave R3 (rules window decisions, pickers everywhere, Paper theme; 2026-11-21): what was integrated (ADRs 0227-0229) and the pitfalls met"
metadata:
  type: project
---

**Integrated (ADRs 0227 contract, 0228 pickers multi-value / strict / accounts, 0229 rule list multi-select):**

- Rule list: a multi-select listbox (`aria-multiselectable`) over the shared pure model
  `ui/src/widgets/listSelection.ts` (`pick`, `gestureOf`, `stepFrom`, `pickAll`, `inListOrder`), also used by
  `features/pim/ContactList.tsx`. Run sends one `RunRules` with the selected ids in file order (one Undo), Test one
  `TestRule` per rule (Preview grouped by rule). Run on lives in the rules store (`runOn`, reset by `setOrigin`).
- Row controls are pointer-only (an ARIA option may hold no focusable control; axe `nested-interactive`): the
  keyboard reorders with Alt+Up/Down and switches a rule in the editor's On switch. A grid was rejected.
- One Exchange badge per rule (`exchange.rule-badge*`): the number sums the reasons over all Exchange accounts;
  its lines are the row's description (the badge's list stays unnamed, so `exchange-rule-badges-label` was removed).
- Pickers: `multiple` (chips `picker.chip*`, `PickerListField`), `strict`, `accounts`; converted junk, server rules,
  vacation, Also my addresses, search index, MCP grants, export / import target, key lookup, automatic replies.
  Left as they are: plug-in folder-picker nodes, Gmail parent label, Exchange shared-mailbox address and room
  lists, composer From, scheduling attendees, import preview folders, calendar / book / list selects.
- Paper theme: `defaults/themes/paper.kdl` (light), registered in `nail-config` `themes.rs` and
  `nail_api::appearance::BUILTIN_THEMES`; theme counts in tests derive from `BUILTIN_THEMES`.

**Pitfalls:**

- Theme KDL accepts only the supported CSS subset: `list-style` is not in it. Plain lists go into the list in
  `ui/src/theme/compile.ts` (base CSS) or a feature's `*_BASE_CSS`.
- Vitest's transform cache (`node_modules/.vitest-cache`) is keyed on a module's text: an `import.meta.glob` list
  (the built-in theme files) stays stale after a new file arrives by rebase. Delete the cache.
- A user theme may not shadow a built-in id: config tests that used a user theme `paper` now use `parchment`.
- The duplicate check flags identical test fixtures (`const ORDER = [...]`) in two test files: keep pure tests of a
  shared module next to the one fixture, or share the fixture.
