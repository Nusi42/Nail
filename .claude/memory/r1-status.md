---
name: r1-status
description: "Wave R1 (rules on demand, live search; 2026-11-20): what was integrated (run panel, Run rule menu, live search, shared ProblemIndicator; ADRs 0219-0221) and the pitfalls met"
metadata:
  type: project
---

**Integrated (ADRs 0219 contract, 0220 live search, 0221 running rules from the UI + shared indicator):**

- Rules window: run panel under the rule list (`features/rules/RunPanel.tsx`): the edited rule or the rules checked
  in the list, on their own mail / the open folder / the selected messages; outcome line always reserved; Undo of
  every run of the last click (`runTargets.ts`).
- "Run rule" in `context:message-list.row` and `context:folder-tree.folder`: layout action `rules.run-menu` (with its
  icon in `defaults/layout.kdl`); `runMenu.ts` replaces the open menu in place with the saved rules; `RunToast.tsx`.
- Editor / Text switch of a new rule: core `render_rule` no longer writes `id=""` (round-trip test in
  `nail-config` `rules::text_form_of_one_rule`); the UI also skips re-reading unedited text.
- Live search: `features/search/liveSearch.ts` (commit on the settled pause or Enter, `CheckQuery` first).
- One compact problem indicator for both: `ui/src/widgets/ProblemIndicator.tsx` (toggle mode = button + native
  popover in the rules window; follow mode = host-controlled details in the search field), look in `theme/skin.ts`
  `PROBLEM`; `RULES_SKIN_CSS` registered in `theme/compile.ts`.

**Pitfalls:**

- Every layout menu item needs an `action` entry with a bundled Lucide icon in `defaults/layout.kdl`
  (`defaults::every_default_action_has_a_bundled_lucide_icon`), even when the feature's `actionInfo` has the icon.
- Changing `defaults/layout.kdl` changes three kinds of snapshots: `NAIL_UPDATE_SNAPSHOTS=1 cargo test -p
  nail-config --test integration` (default-layout fixture), `NAIL_UPDATE_SNAPSHOTS=1 cargo test -p nail-api --test
  integration ts_vectors::` (menus vectors) and `NAIL_UPDATE_GOLDEN=1` for the write-back golden files that copy a
  default window.
- Python on Windows writes CRLF unless the file is opened with `newline=''`; a CRLF `layout.kdl` breaks the golden
  tests. Prettier must not run over `docs/**/*.md` (not prettier-managed; it rewrites `*x*` and wrapped code spans).

**Review fixes (done):** `run_rules` needs `message_ids` for rules that send and a target for grants with excluded
folders; run panel under the editor's buttons with "Save and run" (`runPlan`), folder target off during a search
(`listTitle`), rule list grid with a header row, Escape stops at the problem popover, shared `widgets/Toast.tsx`
(one toast at a time), `expandTargets` in `actions/mailTargets.ts`, search live region `search-bar.status`, wrapping
query line, hoverable explanation, `searchOwnerFor` (last focused field commits). Committed with the
run panel's text-view fix: the editor reads edited text after a typing pause, so the panel names and judges the rule
Save would write; a save from the text view writes the saved rule back as the text (Vitest in `runOnDemand.test.tsx`).
The rule text field is lower (`rules.text.source` min-height 120) so "Save and run" stays in view at 1024x700. Full
suites green at commit: `npm run check` (17 steps), `npm test`, `cargo test --workspace` (4100 passed), `npm run e2e`
(306 passed, 4 skipped, no flakes).

**Deferred:** rules window redesign (incl. the misplaced Test rule results), a hover submenu for Run rule (dynamic menu source in `features/menus`), rule names starting with `@`
read as Fluent ids in that menu, query hints still push content down, a folder change keeps the typed search text.
