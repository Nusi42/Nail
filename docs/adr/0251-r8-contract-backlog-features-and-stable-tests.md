# 0251 — R8 contract: backlog features and stable tests

Status: accepted (2026-11-24)

## Context

After R7 the backlog (`docs/backlog.md`) holds many small and medium items that add to or fix existing features:
free time that ignores the working hours, rules that cannot run on a whole list, tasks from agents without
reminders, quick create from text only in the calendar, missing right-click menus on PIM items, several composer,
settings and window details. Two calendar defects were left from R7: in the demo seeds the "Personal" calendar's
colour (`#3366cc`, a CalDAV `calendar-color` of the mock server) is hard to tell from the palette blue another
calendar gets, and the title of a short meeting ("1:1 with Grace" in the demo week) is cut in the week view although
the card has room for it. Several tests fail now and then under full-suite load (they pass alone); each needs its
cause fixed, not a longer time-out.

The wave runs as a contract, four parallel packages (calendar and PIM UI, mail and windows UI, core, tests and
stability) and an integration.

## Decision

### Scope decisions

1. **Free time follows the working hours (core).** `Request::FindFreeTime` gets
   `hours: FreeTimeHours` (`nail_api::pim::calendar_view`, default `any`): `any` (around the clock, as before),
   `working-hours` (the calendar preferences' `work_hours` / `work_days`, the one source of truth of ADR 0250 that
   the views shade) or `custom { work_hours, work_days }`. With anything but `any` the core cuts every free
   stretch to one day's hours on the chosen days in the request's zone (`nail-core` `pim/free_time.rs`
   `within_hours`: local wall times, so a summer-time day keeps its hours; a skipped time moves past the gap) and
   drops pieces shorter than the request's length. The UI shows the slots as they come and no longer cuts them per
   day (`time.ts` `freeStretchesOfDay` goes). The MCP tool `find_free_time` builds `hours` from `working_hours`,
   `day_start` and `day_end` (`day_start` / `day_end` still win over the preferences; `24:00` is accepted as the
   end of the day) and no longer cuts on its own. Invalid custom hours are refused with `invalid_request` and a
   `pim-issue-*` id (`FreeTimeHours::issues`).
2. **Rules on a whole list (core + mail UI).** `Request::RunRules` gets `list: Option<ListTarget>`, the list
   target of the R5 bulk actions (ADR 0235: the list as the message list pages it, `except` rows, `as_of`). A list
   run excludes `message_ids` and `folder_id`; a conversation row stands for its messages in the listed folder (the
   "listed" reach, as for picked rows in `runTargets.ts`); only messages the core held at `as_of` are touched. The
   core runs each rule in parts like a large mail action (ADR 0236) and records one `RuleRun` per rule with a
   `messages` target (the count), undone as a whole. A remote session needs a full grant for a list run. Until the
   core package lands, the contract's core refuses a list run with `invalid_request` (never runs it on less); the
   UI mock implements it (`api/mockMailActions.ts` `listTargetMessageIds`).
3. **Reminders on tasks from programs (core).** `UpsertTaskArgs.reminders: Option<Vec<TaskReminderArg>>` (left
   out = unchanged, `[]` removes them, at most `MAX_TASK_REMINDERS` = 10); a reminder is exactly one of
   `minutes_before` (before the due time, `RELATED=END`) and `at` (an RFC 3339 UTC instant);
   `TaskReminderArg::to_alarm` is the one conversion to a display `Alarm`. The plug-in task writes take the same
   field (`nail-plugins` `writes.rs`, `prelude.js` docs); the core package wires both paths.
4. **Quick create from text elsewhere (core + calendar UI).** MCP tool `create_event_from_text`
   (`CreateEventFromTextArgs`: `text`, `calendar_id`, `tz`, `locale`, `dry_run`, `idempotency_key`;
   `CREATE_EVENT_FROM_TEXT_TOOL`): read with `ParseEventText`, created like `create_event` (policy kind
   `calendar`, `calendar` scope, through the operation queue); text without a time is refused and creates
   nothing; `dry_run` answers what would be created. The calendar of a new event follows the quick-add rule
   (preferred, else the first writable). The core package adds the tool to `MCP_TOOLS`, its handler and its docs.
   The command palette gets "New event from text" (action `calendar.new-event-from-text`, registered by the PIM
   feature through `Feature.actionInfo`), with the same chips as quick add.
5. **Calendar colours that stay apart (calendar UI).** The seed calendars of the mock CalDAV server and the demo
   seeds get colours that differ clearly from each other and from the palette defaults. On top, one shared helper
   in `ui/src/features/pim/colors.ts` measures the perceptual distance of two colours (OKLab); when a visible
   calendar's colour is too close to an earlier visible calendar's, the later one is shown in the next distinct
   palette colour (display only; the stored colour is unchanged and a colour the user picks in Nail is shown as
   picked). The threshold and the rule are recorded in the package's ADR.
6. **Titles that fit (calendar UI).** A short event's title is shown in full whenever the card's width allows
   (cascade width, line clamping, the time's priority and padding are checked); a Playwright check at 1440×900
   and 1280×800 asserts `scrollWidth <= clientWidth` for every demo event title in the week view.
7. **Stable tests (tests package).** Each known flaky test is fixed at its cause: no longer time-outs, no
   retries, no skips. A race in product code is fixed in product code with a regression test (the core package is
   told when the fix touches code it also edits). Each fixed test passes ten runs in a row under parallel load.
8. **Shared texts.** `status-retry-failed` and `status-dismiss-failed` (`locales/en/app.ftl`, status bar) are the
   texts of a failed bulk Retry / Dismiss, shown beside the failure reason instead of replacing it.
9. **Slots.** No new crate and no new feature folder. New locale slot `locales/en/pim-actions.ftl` (ids
   `pim-act-*`; empty `locales/de/pim-actions.ftl`) for the calendar UI package's new texts (context menus, the
   palette command, the conflict choice, recent mail, occurrence answers). The mail and windows UI package keeps
   its texts in the catalogs of the features it owns; shared catalogs (`common.ftl`, `app.ftl`) change only through
   the integrator.
10. **Release.** The R8 items go into "Nail 0.8.12" of `userDocs/en/release-notes.md` and `CHANGELOG.md`
    `[Unreleased]` (slots prepared here); the packages add their user-visible items, the integrator merges them.

### Packages

- **P1 — calendar and PIM UI:** `ui/src/features/pim`, `ui/src/features/scheduling`, the contact card, the
  calendar parts of `defaults/theme.kdl`, the seed colour values (and their tests) in
  `crates/nail-testkit-net/src/dav.rs` and any demo seed that sets calendar colours, `pim-actions.ftl`, the
  userDocs pages of these features, e2e `r8-p1-*.spec.ts`. Items 5 and 6, the UI side of items 1 and 4, and the
  backlog rows "Answering one occurrence from the quick look", "Drawing events on touch screens", "Icons for place
  and call", "Right-click menus for PIM items" (check), "Conflict choice for events", "Recent mail on the contact
  card", "Undo for invitation answers" (check), "Quick create elsewhere" (UI), "Free time" (UI).
- **P2 — mail, composer, rules, settings and windows UI:** the other UI features (mail, composer, rules, settings,
  shell, window layer, palette — except the PIM feature's palette command), their theme parts, catalogs, userDocs
  and e2e `r8-p2-*.spec.ts`. The UI side of item 2 and item 8, and the rows "Favorites as views", "Searched folder
  in the tree", "Several unsaved composers", "Background composer report", "Composer preview height", "Monospace
  text areas" (check), "Run selected rules from the menu" (check), "Run rules on a whole list" (UI), "Retry /
  Dismiss errors", "Layout presets", "Dialog footer", "Settings drafts", "Pop-out key bindings", "Menus and
  settings page" (check), "Search hint key" (check), "Toolbar customizer", "Rail".
- **P3 — core:** the crates (except the slots of P1 and P4), `nail-api`, the MCP tool docs, their Rust tests. The
  core side of items 1–4 and the rows "Feed subscriptions", "Reminders after a time-zone change",
  "`RDATE;VALUE=PERIOD`", "Mass failure logging", "Sieve status after a restart", "Sieve details", "Inexact server
  search hits", "Unified list order", "Junk classifier", "Rule text issues".
- **P4 — tests and stability:** e2e specs and `e2e/support`, the flaky Rust tests' files
  (`crates/nail-core/src/replay.rs` tests module, `crates/nail-core/tests/exchange.rs`, the `mail_actions`
  tests, `nail-bench`, the `nail-smime` tests) and the fixes of the races they expose, `scripts/` test tooling.
  Item 7 and the rows "Rule list drag", "Auto-schedule e2e", "Dialogs without e2e", "Exchange move race" (check)
  and every "Flaky: …" / "Listing throughput budgets" row.

Packages use the contract's pieces and do not change them (`FreeTimeHours`, the `RunRules` list field,
`TaskReminderArg`, `CreateEventFromTextArgs`, the mock's `freeTimeWindows` and `listTargetMessageIds`, the shared
texts); a needed change goes into the report for the integrator. Each package removes the backlog rows it
finishes.

## Consequences

- One rule decides which hours count as free time for the views, scheduling, Find a time and agents; a change of
  the working hours in Settings changes them all.
- "Run rules on selected" after select all reaches exactly the rows the user saw, as the R5 bulk actions do.
- Agents and plug-ins write tasks with reminders, and agents create events from a sentence with the same reader
  as the calendar.
- Until the core package lands, `RunRules` with a list is refused by the core (the UI mock answers it), plug-in
  and MCP task reminders are ignored, and `create_event_from_text` is not offered.
