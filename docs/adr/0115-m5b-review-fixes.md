# ADR 0115: M5b review fixes: the PIM rules the theme lost, free time per working day, the calendar's view menu at narrow widths, menus without disabled entries, localized folder names, Contacts / Tasks headers

Status: accepted · 2026-10-03 · M5b review

## Context

The M5b reviews of the wave integrated by ADR 0114 found these defects
(each checked against the running `nail serve --seed demo-multi` build, at
1024 / 1280 / 1440 px, before fixing):

- **PIM styles lost with the M3a theme rules.** ADR 0114 removed the M3a
  PIM `part` rules from `defaults/theme.kdl` because they overrode
  `features/pim/skin.ts`, but several had no counterpart in the skin or in
  `parts.ts`: the week's day heads lost `display: flex; flex-direction:
  column` ("Sun9", visible in every regenerated calendar baseline), the
  current-time line lost its height (invisible), the month cells lost their
  height, border and the `outside` / `today` / `drop-target` /
  `focus-visible` states (no grid lines, rows a third of the view, chips
  running into the next day and past the window edge), subtasks lost their
  indent, done tasks their strike-through, the current task its background,
  and the contact card's avatar its 72 px size. The `calendar` part's
  remaining theme rule (`display: grid` with rows `auto auto 1fr`) also put
  every view into an `auto` row.
- **Free time** sent the whole range to `FindFreeTime` (which knows
  neither days nor working hours), filed each slot under its start day and
  clamped it to the track: a calendar free for days showed one
  "12:00 AM – 12:00 AM" slot on Sunday and "No free time." on the other
  days. The range started at the locale's week start, so on a Friday or
  Saturday today was not shown, Sunday was.
- **The calendar toolbar clipped at 1024 px** (about 700 px of pane): the
  range was cut and "Agenda" / "Free time" were hidden behind "New event".
- **Account state hidden.** The state on a folder-pane section heading
  (the replacement for the removed status dots) was inside the title that
  is cut with an ellipsis.
- **More menu / right-click menus.** "Import Autocrypt setup message" was
  enabled for any selected message (so in More for ordinary mail, cut by
  the menu width); overflowed bar actions were appended at the bottom of a
  13-entry menu; the reader bar's menu showed no shortcut for Print (its
  key context was `global`, the binding is in `reader`); right-click menus
  showed greyed-out actions (Labels…, Categories…, Download full message,
  Not junk for ordinary mail) against the proposal's "disabled actions are
  hidden".
- **No visible entry point** for the command palette, the local filter
  rules and the Outbox after the main toolbar left the default layout.
- **Contacts / Tasks** were bare widget forms (no title, no pane padding,
  four text buttons including a disabled Delete, a misaligned quick-add).
- **Raw folder names in server order** ("ARCHIVE, DRAFT, INBOX, SENT,
  SPAM, TRASH") although the folder's role was known.
- **The visual harness** covered only the week view at 1280 px, so all of
  the above passed "100 screens passed".
- Low (also fixed): Tab from the mail search dropped focus to `<body>` (the
  hints unmounted on the input's blur); the year view had no visible month
  names; the calendar view had two primary buttons and an inline task form
  in its sidebar; the filter-chip scenario never pressed Flagged or
  Attachments.

## Decisions

- **The PIM skin owns the parts it styles.** `PIM_SKIN_CSS` gains the day
  heads' column layout, `calendar.now { height: 2px }`, the month grid
  (`calendar.month { height: 100% }` inside a new `calendar.month.frame`
  that takes the view's height, cells with `border`, `height`, `overflow:
  hidden`, the four states, weekday heads, chips cut with an ellipsis,
  "+ n more"), the year's month captions (`calendar.year.month.title`),
  task levels / done / current, the quick-add row, the contact card's
  avatar and the new view header. The remaining theme rules of those parts
  (`calendar.month`, `.weekday`, `.events`, `.more`, `task-list.add`) and
  the `calendar` part's grid leave `defaults/theme.kdl`; the `calendar`
  part is a flex column in `parts.ts` (the view grows, the toolbar keeps
  its height). A unit test reads `defaults/theme.kdl` and fails when it
  styles a part the PIM skin styles again.
- **Free time per working day.** `viewRange("free-time")` starts at the
  first day on or after the anchor that is no weekend day in the UI locale
  (`isWeekend` of `@internationalized/date`) and spans as many days as it
  takes to cover `days` working days; the view leaves the weekend days out
  (`workingDaysOf`) and cuts every free stretch the core returned to each
  day's `work-start` – `work-end` in the view's zone, dropping pieces
  shorter than `duration` (`freeStretchesOfDay`). The core's `FindFreeTime`
  stays a plain interval search.
- **The view menu replaces the segmented switch when it does not fit.**
  `useNarrowViewSwitch` (`features/pim/switchFit.ts`) observes the toolbar;
  when the switch scrolls, the range is cut or the bar overflows, the
  widget shows the existing `view-switch="menu"` drop-down and remembers the
  width the segmented row needed; at that width it switches back
  (hysteresis, no flapping). `calendar.toolbar[data-switch]` says which.
- **Right-click menus hide what does not apply.** `ContextMenuHost`
  filters the resolved menu with the action dispatcher's `isEnabled` at
  open time (`withoutDisabled` in `menus/resolve.ts`: headings and
  separators normalized as in `resolveMenu`, empty submenus dropped); a menu
  with nothing left does not open. No layout option: the proposal's rule
  applies to every context menu (toolbars keep `hide-disabled=`).
- **Enablement follows the message.** `security.autocrypt.import` is
  enabled only for an Autocrypt Setup Message (an
  `application/autocrypt-setup` attachment, or the payload's
  `Passphrase-Format: numeric9x4` armour in the text, as the core accepts);
  `junk.not-junk` only in a Junk folder or for mail the classifier flagged
  (`MessageBody.junk`), `junk.mark` not in Junk. `useActionEnabled` also
  observes the selected message's cached body (a query observer with
  `skipToken`: no fetch of its own) and the active folder.
- **More lists what did not fit first**, in rank order (unranked items
  after them), then a separator and its own entries. This reverses ADR
  0109's "overflowing items are appended". Menus are as wide as their
  longest label (`width: max-content`, at most 30em or the window) and
  labels wrap to two lines instead of being cut. A toolbar outside the
  widget that owns its key context uses `TOOLBAR_KEY_CONTEXTS` (`reader`,
  `calendar`) for the shortcuts it shows.
- **Entry points.** The rail gets `item "palette.open" icon="command"`
  (tooltip "Command palette (Ctrl+K)"); Settings → Mail & junk shows the
  `rule-manager` (with settings-search labels); the folder pane's default
  favorites gain `outbox`, shown only while the outbox is not empty, with
  the number of waiting messages.
- **Folder names.** Special-use folders show the Fluent role name
  (`folder-role-*`; the server's name is the tooltip) and come first in a
  fixed order (Inbox, Drafts, Sent, Archive, Junk, Trash), the others keep
  the core's order (`widgets/folderNames.ts`; `flattenFolders(…, {
  rolesFirst })`). The list header titles a special folder the same way.
- **Account state** is a sibling tag after the title (`flex: 0 0 auto`,
  pushed to the end), still part of the section's accessible name; only the
  title is cut (its full text is its tooltip).
- **Contacts / Tasks headers.** `contact-list` and `task-list` take
  `header=#true` (nail-config option): a `pim.view-header` with the title,
  the search (contacts), "New …" as the one primary button and, for
  contacts, a "More" menu (Groups…, Find duplicates…, Delete only while a
  contact is chosen). Without the option the widgets keep their buttons but
  also hide a Delete / New task that cannot run. Both panes get the pane
  padding.
- **Calendar view.** "Find a time" is no longer rank 1 (no second primary
  button), "New task" / "New contact" move into the toolbar's More, the
  sidebar's `task-list` leaves the default layout.
- **Search hints** hide only when focus leaves the whole search form.
- **Visual harness.** `Screen.viewport` (the harness resets the window per
  screen); new screens `calendar-month`, `calendar-year`,
  `calendar-agenda`, `calendar-free-time`, `calendar-1024`, `contacts`,
  `tasks`, `main-1024`.

## Not changed

- The High contrast plain-text reader still wraps 72-character hard lines
  once more at 1280 px (low finding): reflowing plain text is a reader
  change of its own (format=flowed, quoted blocks) and is left for M5d.
- `FindFreeTime` in the core still ignores working hours; the UI cuts the
  result. A server-side option can come with scheduling work.

## Consequences

- Themes that styled `calendar.month*` or `task-list.add` in their own
  `theme.kdl` still win over the skin; the default theme no longer does.
- Visual baselines must be regenerated for the changed calendar screens
  and created for the eight new screens (5 themes × 2 densities).

## Post-verify fixes

The independent verify of these fixes found, and the same wave fixed:

- `defaults/layout.kdl` and two nail-config `write_back` goldens had been
  written with CRLF line endings in the working tree; `include_str!`
  embedded the CR and the golden comparison failed. They are LF again and
  `.gitattributes` names `*.kdl` / `*.ftl` (files the binaries and tests
  embed) `text eol=lf` explicitly.
- The reader bar's More showed no shortcut for "Print the message…": the
  bar sits inside the view host's region, whose key context is `global`, so
  the closest `data-key-context` won over `TOOLBAR_KEY_CONTEXTS`. A bar now
  looks its shortcuts up in its own context first (`toolbarKeyContext`),
  then the closest one, then `global`.
- Settings search: a setting's highlighted label is one flex item
  (`settings-page.result.label`), so the marks no longer split it into
  pieces with gaps that wrap word by word.
- Month chips show the title first and only the start time
  (`EventItem timeStyle="start"`; the label keeps the range); a container
  query hides the time when the chip is too narrow for both.
- The week view's initial scroll lands on a whole hour
  (`initialScrollTop`): a tall pane that cannot scroll to `scroll-to` no
  longer cuts the topmost hour label in half.
