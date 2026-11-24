---
name: r8-status
description: "Wave R8 (backlog features and stable tests; release 0.8.12): what was integrated (ADRs 0251-0255), the shared helpers to use and the pitfalls met"
metadata:
  type: project
---

**Integrated (ADR 0251 contract, 0252 calendar and PIM UI, 0253 mail and windows UI, 0254 core, 0255 stable
tests), release notes "Nail 0.8.12":**

- Calendar colours: `ui/src/features/pim/colors.ts` is the one colour rule (`displayColors`, `colorDistance` in
  OKLab, `MIN_COLOR_DISTANCE`); palette colours show as stored, a server colour too close to another visible
  calendar's shows in the next distinct palette colour (display only). Seed colour
  `nail_testkit_net::SEED_PERSONAL_COLOR`; tests use the constant.
- Week cards: the title takes the first line, the icons sit at the end of the time line (`gridStyle.ts`);
  `r8-p1-calendar.spec.ts` checks cut / clamped / covered titles at 1440×900 and 1280×800.
- PIM menus share `PimMenuPopup` (`PimMenu.tsx`, `usePimContextMenu`); the conflict choice is `common.tsx`
  `SyncState` for contacts and events; recent mail on the contact card uses its own query key
  `pimKeys.recentMail` (the mail lists treat every `search` cache entry as their own paged results).
- Mail / windows: `useMenuButton` (`features/menus/MenuButton.tsx`) for any button that opens a menu;
  `DraftScope` for settings drafts (a window with its own drafts uses `SettingsDraftScope`); the close guard's
  `SEVERAL_GUARD`; `DialogFooter.secondary` takes several buttons.
- Core: `RunRules.list` (one `RuleRun` per rule, parts of 500, `resolve_list` shared with mail actions); task
  reminders on MCP and plug-in writes (`MAX_TASK_REMINDERS`, exported to both TS packages);
  `create_event_from_text`; `Recurrence.periods`; schema v26 (`junk_verdicts.unsure`); `JunkSettings.ham_cutoff`
  (UI field, `JUNK_DEFAULT_HAM_CUTOFF`, issue `ham-cutoff` pinned by ts-vectors).
- State kept per UI store uses `UiStoreMap` (`ui/src/state/uiStore.ts`), never a bare `WeakMap<UiStore, …>`: a
  reader card's `scopedUiStore` registers itself as a view (`registerUiStoreView`), so feature stores, open
  questions and guards are the app's. A bare WeakMap gave the card's "Move to Other" an empty classify window.
- Stable tests: WebSocket answers go out after the events their request broadcast (`nail-server` `ws.rs`);
  `useSetUiState` sets only its own key from the newest write, and `applyUiStateEvent` skips a key's events while
  a write of it is on its way; `useLayoutEdit` reads the history after each
  write; `SieveCtl::force` / `take_forced`; replay tests capture logs through one process-wide recorder; waits on
  progress (`eventually_progressing`) instead of fixed budgets.
- Default keys `ctrl+shift+o` / `ctrl+shift+m` (`popout.open` / `popout.return`, context `window`); a key-bound
  action needs an `action` line with a bundled icon in `defaults/layout.kdl` (test
  `every_default_action_has_a_bundled_lucide_icon`). `map-pin` is bundled.

**Pitfalls:**

- Four packages each took ADR 0252 and each edited the backlog: on a rebase conflict in `docs/backlog.md` both
  sides removed their own finished rows, so the resolution drops both sides of each block.
- After rebases that add catalogs or theme files delete `node_modules/.vitest-cache`.
- Under a second full suite on the same machine, WebKit specs and some long Chromium specs still run out of the
  30 s budget (backlog row); judge e2e at the default workers without other load.
