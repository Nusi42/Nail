---
name: r2-status
description: "Wave R2 (rules dialog redesign, shared pickers, values from the mail, R1 leftovers; 2026-11-21): what was integrated (ADRs 0222-0226) and the pitfalls met"
metadata:
  type: project
---

**Integrated (ADRs 0222 contract, 0223 rules window, 0224 pickers, 0225 values from the mail, 0226 menu sources and
search hints):**

- Rules window: `RuleEditor` / `RuleForm` / `LowerPanel` / `Footer` / `Confirm` / `TextView` in
  `ui/src/features/rules/`; `RunPanel.tsx` is gone; Run on / Run / Test / Save in the footer; the store guards
  unsaved changes (`dirty` / `pending` / `guard`, `closeGuard.ts`). Window size 1100x760 (pinned in
  `nail-config` m5c_windows, `windowSizes.test.ts`, `popout.test.tsx`, `e2e/tests/m5c.spec.ts`).
- One picker: `ui/src/widgets/pickers` (`Picker`, `PickerField`); `features/rules/PickerField.tsx` is only an adapter
  (folder paths, placeholder id, describedBy only while invalid). Picker inputs are role `combobox` (tests find
  "Folder path" as a combobox).
- Values from the mail: `ValuesFromMessage` / `ActionValuesFromMessage` in the condition / action rows use the
  second `onPick` argument (`applyToCondition` sets comparison and address field). The editor's own `withPickedValue` was dropped.
  "Add to rule…" (`rules.add-to-rule`) is registered in `features/rules/index.ts` with its menu source; not in the
  reader card menu (that menu acts on the card, the rules entries on the selection).
- Menus: hover submenus from `Feature.menuSources` ("Run rule", "Add to rule…"); `MenuSourcesContext` comes from
  `App.tsx` `FeatureEffects`; the test harness does not provide it (wrap like `dynamicMenus.test.tsx`).
- Search: combobox with a floating hints popup; the field's values (`useSearchValues`) open in the same popup only
  when there are values; Escape and Enter hide them for that text.
- Shared in the integration: `PimMenu` `showLabel` (the "+ Add" buttons show text; no CSS `::after`),
  `state/mailOps.ts` `cachedFolder`, `state/queries.ts` `foldersQueryOptions`, `state/queryText.ts`
  `searchValueText`, `e2e/support/rules.ts` (WR3 and WR6 had each extracted the same e2e helpers).

**Pitfalls:**

- Two packages extracting the same helper in parallel: keep one at the rebase (the duplicate check catches it only
  when both land).
- After `defaults/layout.kdl` / `theme.kdl` changes regenerate `default-*.json`, `ts-vectors/menus.json` /
  `views.json` and the `layout_edit` golden (`NAIL_UPDATE_GOLDEN=1`) — a new reader-toolbar item changes it.
- The Bash tool collapses double backslashes inside heredocs: write files with backslashes through the Write / Edit tools.
- Python on Windows writes CRLF in text mode: open with `newline=''`.
