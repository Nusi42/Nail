# Changelog

All notable changes to Nail. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow
[Semantic Versioning](https://semver.org/). The user-facing release notes
are in [`userDocs/en/release-notes.md`](userDocs/en/release-notes.md);
decisions behind each change are in [`docs/adr/`](docs/adr/).

## [Unreleased]

### Added — a calendar that reads well, contract (ADR 0246)

- The calendar palette is core data (`CalendarColor`, `CALENDAR_PALETTE`)
  with one rule for default colours (`next_calendar_color`: the first
  unused colour, else the least used).
- Calendar preferences in the app settings (`GetCalendarPreferences` /
  `SetCalendarPreferences`, `CalendarPreferencesChanged`): working hours
  and days, week start, ISO week numbers, the view zone and a second zone,
  showing declined events.
- `ListTasksDue` lists open tasks due in a range for the calendar views.
- `ParseEventText` reads a sentence as an event (crate `nail-event-text`;
  the reader itself follows).
- `EventInstance.my_status` carries the user's own participation status.

### Added — a calendar that reads well, time grid (ADR 0247)

- Overlapping events cascade (indented, on top, at least 60 % of the
  column; events starting together side by side, free columns used); events
  under 45 minutes are one-line cards.
- Drawing a new event on empty slots (15-minute steps, live time label,
  inline title: Enter saves, More options opens the editor); a click takes
  the 15-minute slot under the pointer; Shift + arrows and Enter from the
  keyboard. A click on an event opens the quick look, a double-click the
  editor.
- Today's date in a filled circle without a column shade; the now line
  (2 px with a dot on today, faint across the week, time in the gutter)
  under the events.
- Card icons (repeats, people, reminders, read-only), hatched free time,
  provisional unanswered invitations, declined ones struck through or
  hidden (`show_declined`).
- Working hours hatched, days off tinted, the grid opens at the working
  day's start, ISO week number in the corner; tasks due in the all-day row
  with a check box; a task dropped on the grid creates an event.

### Added — the quick look and the compact editor (ADR 0248)

- A quick look beside an event (time with length and zone, calendar,
  place, people with their answers, Join, Edit / Delete / Send update,
  Accept / Maybe / Decline with the overlaps of an unanswered invitation).
- The compact event editor: a large title, one time row with the day as
  text and the length, option chips (repeat, reminder, calendar, busy,
  call link), "More options" for the rest.
- The invitation card's segmented answer, mini day strip, one-line
  attendee summary and "More" menu; the iCalendar part is no attachment
  of its own while the card shows it.
- The scheduling lanes: a lane per person with its source, a draggable
  meeting frame (arrow keys too), suggestion buttons without the chosen
  time, consistent texts and hidden role labels.
- The reminder toast's relative time, calendar colour and name, Dismiss
  first and a split snooze button.

### Added — calendar views and shell (ADR 0249)

- Calendar toolbar: quick add from text with a live preview of the parsed
  parts (`ParseEventText`; Enter adds, Tab opens the editor), event search
  over `SearchPim` grouped upcoming / past (`/` focuses it), and a time zone
  chip that saves the view zone and a second zone in the calendar
  preferences.
- Narrow windows: the calendar window's narrow breakpoint in `layout.kdl`
  drops the sidebar; the calendar then offers it as a drawer, floats New
  event, and falls back to three days, then one, when day columns would get
  narrower than 90 px.
- Views: month bars across days and "dot, time, title" lines, agenda time
  column, sticky Today / Tomorrow headers, now marker and "until …" for
  midnight-crossing events, year heat shading, free-time axis with labelled
  busy blocks and daily totals, empty states with the nearest event,
  mini-month band, dots and week numbers.
- Settings: the calendar preferences (working hours and days, week start,
  week numbers, declined invitations) with the settings save bar; a "Due
  this week" panel under the calendar list (`ListTasksDue`).
- Event cards use a 20 % tint of the calendar colour; the new-calendar
  dialog no longer picks a colour itself.
- The week start preference applies to the week, month, year and
  mini-month views, their keyboard Home / End and "Due this week"; quick
  add's Tab hands a parsed repetition to the editor; one rule picks the
  calendar of a new event (`defaultEventCalendar`) and one hides declined
  occurrences (`visibleInstances`); recurring events show the `repeat`
  icon and the call link chip `video`.

### Added — a calendar that reads well, core (ADR 0250)

- `nail-event-text` reads English sentences: today / tomorrow / weekdays /
  "next Friday" / "Dec 4" / numeric dates, times and ranges with am / pm,
  lengths, "all day", "every …" repetitions and "at …" places, with UTF-16
  spans.
- `EventInstance.my_status` is filled from the user's identities and
  account addresses; own free/busy reads it.
- New and discovered calendars without a colour get `next_calendar_color`;
  schema v25 colours existing ones once; Graph `hexColor` is kept.
- MCP `find_free_time` takes `working_hours`; `list_tasks` takes
  `due_after` / `tz`.

### Fixed — calendar review (ADRs 0246–0250)

- Overlapping events cascade only when the earlier title stays visible (the
  exposed height in pixels, `sideBySideMinutes`), else stand side by side;
  titles wrap onto two lines on medium cards; on short cards the time gives
  way before the title; High contrast keeps room for the card borders.
- The shared popover places itself again when its content grows or the
  window changes size; Delete in the quick look moves the focus to the next
  card or the active slot.
- Month dot lines keep the calendar colour, dot first.
- `ParseEventText` without `now` reads the core's clock; the quick-add
  chips show the resolved day, times, repetition, place and calendar.
- `SearchPim` gives a repeating event's next occurrence (else its last).
- New `ListEventOverlaps`: the quick look's "Overlaps …" uses the
  invitation card's conflict rule, which now also skips declined
  occurrences.
- One people summary (organizer panel and invitation card included); the
  scheduling grid and Find a time follow the working hours; Accept is the
  filled primary answer.
- Due tasks: one shared item for the all-day row and the panel (the raw
  title on a drag) with a Plan button that uses the chosen slot.
- Free time merges overlapping busy blocks and never cuts a stretch's
  times; one length wording (`lengthText`) for the drawn range and the free
  total; one week-number abbreviation; the working-hours rule pinned to
  `WorkHours::is_valid` by test vectors.
- `nail-event-text`: a repetition's start moves to its first named weekday
  on or after a given date; lengths are added on the instant timeline
  (summer time); a time beside a day count stays in the title.
- A cancelled keyboard range no longer stays highlighted.

### Fixed — deferred writes are retried soon (ADR 0245)

- A replay pass that defers writes retries the account's queue within
  seconds (2 s doubling up to a minute, five times, or the server's
  `Retry-After`) instead of at the next poll.

## [0.8.10] — 2026-11-23

### Added — select all and large mail actions (ADR 0235)

- A mail action can name a whole message list except some rows: the new
  target kind `list` (`ListTarget` with `MailList`: a folder's
  conversations or messages, or a search with the filter chips' terms) is
  resolved in the core, so selecting all never sends thousands of ids.
  `ResolveMailTargets` answers with the true count (`messages`) and an
  `as_of` mark that keeps mail arriving after the confirmation out of the
  action.
- `Ctrl+A` in the message list selects the whole list, rows not loaded yet
  included; `Ctrl` / `Shift` + click deselects rows, `Esc` clears.
- Every action on a whole selected list asks once with the true count and
  names the action ("Archive all"); Select all is in the command palette
  and the message list's right-click menu (ADR 0237).
- Under select all, `Ctrl+Space` leaves the selected row out (or takes it
  back) and `Shift+↑` / `Shift+↓` leave out a range; screen readers hear
  "All N selected" and "Selection cleared" (ADR 0238).
- `MailActionResult::left_out` says how many messages a very large action
  could not change when a later part failed; the toast reports it.

### Changed — no limit on mail actions (ADR 0235)

- No limit on the conversations or messages one mail action touches:
  `MAX_MAIL_ACTION_TARGETS` (now 4 096, derived from the largest request
  the web socket accepts, `MAX_REQUEST_BYTES`) only caps the rows named one
  by one, and actions above `MAIL_ACTION_PART_MESSAGES` (10 000) messages
  run in parts with one undo. The issue `too-many-messages` is gone; the
  new issue `nested-list` refuses a list among a list's exceptions.
- Actions on very large selections (tens of thousands of messages) run in
  the background in parts and are still undone with a single Undo. Mail
  that arrives after you confirm a Delete or a move is left alone.
  Archiving and moving many messages is about four times faster (ADR 0236).

### Added — one dialog pattern (ADR 0239)

- Shared dialog pieces in `ui/src/widgets/dialog/`: `DialogFooter` orders
  buttons by role for the viewer's system (Windows / Linux primary first,
  macOS mirrored), `ConfirmDialog` (a destructive verb is red, never focused,
  never run by Enter), `DialogForm` (Ctrl+Enter in multi-line fields),
  `SettingsSaveBar` (Save / Discard while a settings section has unsaved
  changes).
- The window layer's close guard (`ui/src/layout/closeGuard.ts`): ×, Esc and
  Cancel of a window with unsaved changes ask with one prompt ("Save changes
  to “X”?" — Save / Don't save / Cancel); leaving a settings section or
  closing Settings asks the same way.
- "Delete permanently?" and New / Rename theme follow the pattern; New theme
  asks before a typed name is lost.
- The mail dialogs follow the pattern (ADR 0240): closing a composer with unsaved
  changes asks with the shared prompt (Save draft / Don't save / Cancel;
  the composer's own close confirmation is gone), Discard asks "Discard
  draft", `Enter` in Subject no longer sends (`Ctrl+Enter` does); folder,
  Gmail label and Exchange category / mailbox editors ask before changes are
  lost, and their deletes focus Cancel; "Apply" became "Save" in Apply
  labels / categories; Automatic replies has one footer (Save closes);
  Print offers Print, Save as PDF… and Cancel; the message source has no
  footer Close; the move question before Trash, Drafts or Junk goes back to
  the picker on Esc; an Outbox draft's Discard asks first; the plug-in send
  veto's "Send anyway" is red and Cancel has the focus.
- Rules, appearance, customization, calendars, contacts, tasks and
  scheduling follow the pattern (ADR 0241): the rules window's unsaved rule
  asks with the shared prompt and its delete / run questions are modal
  confirmations; Settings → Appearance and the toolbar / column editors
  save with the section's bar (no Apply, no live sliders); the merge
  wizard has Next / Apply theme / Back / Cancel and Done; the event, task
  and contact editors have a pinned footer with `Delete…` (asks first) and
  ask before unsaved changes are lost; deleting a contact group and
  removing a calendar source ask; Cancel meeting is a red "Cancel meeting"
  with "Keep meeting" focused; the recurring-event question names its verb
  ("Save changes" / "Delete events").

### Fixed — dialog pattern review (ADR 0244)

- Deleting contacts asks (`pim-delete-contacts`: the card, the list, the
  Delete key, and the contact editor's new `Delete…`).
- `MailConfirm` is gone: the bulk, open-link and open-attachment questions
  are `ConfirmDialog`s (a normal bulk action's verb has the focus); the
  window sweep fails on a confirmation outside `ConfirmDialog` and on a
  modal window without the shared footer.
- The key-binding editor, the plug-in manager (on / off, permissions,
  settings; its own window has the save bar) and the Exchange inbox-rule
  switches are settings drafts; deleting an inbox rule asks in a question
  window.
- Calendar settings, New calendar and the contact group editor ask before
  unsaved changes are lost; the invitation answer's confirmation and the
  Propose / Forward forms use the shared footer.
- Popping a window out or returning it asks first while it has unsaved
  changes (the composer hands its draft over); Cancel during the prompt's
  Save keeps the window.
- One question window (`askQuestion` / `QuestionWindow`) and one pending
  slot (`pendingSlot`) for every confirmation; one `sameJson` for plain-data
  equality.
- The settings save bar announces its state politely and moves the focus to
  the section's title after Save or Discard.

### Changed — settings save with the bar (ADR 0242)

- Every settings section applies nothing before the bar's Save, switches and
  choices included (Desktop app, Updates, key lookup and indexing, folder
  sync, junk filter, notifications, server rules and vacation reply,
  accounts and identities, virtual accounts, MCP access and limits, remote
  access and users' access, key trust, smartcards); the widgets' own Save,
  Save access, Save limits, Save trust and Undo changes buttons are gone.
  Choosing another account, client, user, folder or key inside a section
  with unsaved changes asks first.
- Remove account…, Delete virtual account…, Reset the filter…, Revoke…,
  Delete key…, turning encryption at rest on or off and the remote access
  actions ask in the shared confirmation instead of an inline row.
- The Add account wizard has Cancel on every step and Done at the end, and
  asks before typed details are thrown away; the folder synchronization
  dialog has Save / Cancel and asks when changed.

### Changed — the data, security, plug-in and system dialogs (ADR 0243)

- The import, export, backup, restore and diagnostics wizards, the recovery
  key wizards, Pair a device, the re-authentication, Autocrypt import and
  the plug-in install wizard use the shared `DialogFooter`: Cancel on every
  step, Done on the result step. "Restore now" is destructive (never the
  default, Cancel focused); the diagnostics bundle is a wizard (Next, Back,
  Save bundle, Cancel); the smartcard PIN prompt's verb is "Unlock".
- Clearing a connection log (`connection-log-clear`), removing a plug-in
  (`plugin-uninstall`) and removing the recovery key
  (`security-recovery-remove`) ask in a destructive `ConfirmDialog`; the
  inline "Are you sure" rows are gone.
- Plug-in dialogs render their buttons through `DialogFooter`: `primary`
  buttons are the primary role, the others secondary; Nail adds Cancel
  (`button: null`), and a plug-in button with the id `cancel` takes its
  place. The plug-in API is unchanged.
- Error details and the application log have no footer Close (× only); the
  job report window has no footer; the welcome screen has one Get started,
  and closing it skips the first run.
- Theme parts `remote-login.actions`, `remote-pair.actions`,
  `plugin-install.actions`, `first-run.buttons`, `recovery-settings.confirm`,
  `plugin.dialog.buttons` and `plugin.dialog.button` are gone (the shared
  `dialog.footer` styles them); new: `recovery.tools`, `plugin.dialog.form`.

### Changed — one dialog pattern, integration (ADRs 0239–0243)

- The confirmation policy of an MCP client or plug-in (Settings → Agents
  (MCP), Plug-ins) is a settings draft too: it is written with the bar's
  Save.
- `FooterButton` can be marked unavailable but stay focusable
  (`ariaDisabled`, `describedBy`): the rule editor's Save rule and Run do
  so again while the rule has problems, naming the problem badge.
- The list of pending pairing codes says "Revoke code", like Pair a device.
- Removed theme parts no component emits any more: `wizard.buttons`,
  `rules.footer.start` / `.end`, `rules.confirm` (`.question`, `.actions`),
  `merge-dialog.actions`, `column-editor.actions`, `pim.scope.actions`,
  `scheduling.dialog.actions`, `scheduling.find-time.actions`; plus the
  packages' own removals (`gmail.dialog.actions`, `snooze.actions`,
  `plugin-veto.buttons`, `account-setup.actions`, `remote-confirm`, …).
- The window sweeps check every window's ×, Esc and shared footer (order
  for Windows, Linux and macOS, the focused button).

### Fixed — large selections (ADR 0235)

- Acting on more than 500 selected conversations failed with "Too many
  conversations at once" (confirmations asked the core about every row at
  once).

## [0.8.9] — 2026-11-22

### Fixed — bulk actions (ADR 0230)

- The status bar's "Saving N changes" counts the core's real queue of
  message writes (new request `GetOpsSummary`, event `OpsChanged` once per
  queued batch and per replayed group instead of one event per operation),
  so it no longer stays up after several hundred moves or deletes.
- A client that missed events (more than the event channel holds) gets the
  queue summary and a new `Resync` event and refetches its open message
  lists and selection, so moved messages no longer linger in other lists.
- Failed writes can be retried or dismissed all at once
  (`RetryFailedOps`, `DismissFailedOps`). The status bar names the newest
  failure's real reason (its error class and text are stored with the
  write, schema v22, so they survive a restart) and how many failed.
- Message writes no longer send one `OpStatus` event per operation;
  `OpsChanged` replaces them (folder changes keep `OpStatus`). Waiting
  folder changes, Gmail label writes and Exchange writes are counted in the
  summary. A pass that has to wait logs one INFO line.
- The status bar asks the core again when changes wait and no summary
  arrived for a while, so it returns to idle even when events are lost.
- Moved messages no longer show again in their old folder when the server
  rebuilt it (UIDVALIDITY change) while the move was still waiting.
- The Move window moves only to the highlighted folder or an exact name
  match; better matches come first, the current folder is left out, and
  moving into Drafts, Trash or Junk asks first with the count.
- Only `\Draft` messages continue in the composer; other messages in a
  Drafts folder open in the reader, with the new action Move out of
  Drafts. `EditDraftMessage` on a message without `\Draft` makes a new
  draft that never replaces or deletes it.
- A draft's server copy is deleted only when it is Nail's own copy or
  flagged `\Draft`; schema v23 unlinks drafts that 0.8.8 linked to a
  received message, which their next autosave, send or discard deleted
  (ADR 0233). Move out of Drafts moves only the selected message.
- A server message held back while a move of its Message-ID out of the
  folder waits or failed is listed (schema v23, `sync_refetch`) and fetched
  once nothing holds it back; dismissing a failed move or delete puts the
  message back where the server still has it (ADR 0233).

## [0.8.8] — 2026-11-21

### Added — rules on demand (ADRs 0219, 0221)

- Run one rule, or all rules that are on, on the selected messages or a
  folder: from the Run rule submenu in the message and folder menus, from
  the rules window's Run on choice, and for agents through the MCP tool
  `run_rules` (forwards and replies only on named messages, excluded
  folders respected).

### Changed — rules window (ADRs 0222, 0223)

- Redesigned rules window: a footer that stays in view (Run on, Run, Test,
  Save, Delete with confirmation), a problem badge instead of red error
  blocks, Preview and Recent runs tabs (actions with their values, why a
  message matched, Apply to these, run time and target), a compact
  When / Where / If / Then form, rule search, drag and Alt+Up/Down
  reordering, starter rules, and the same look in Settings. Asks before
  closing with unsaved changes.
- New rules switch between Editor and Text without errors; unsaved rules
  no longer get an empty id.

### Added — pickers and values from the mail (ADRs 0224, 0225)

- Searchable pickers for folders (a tree), accounts, tags, addresses,
  mailing lists and header names, with free text still allowed, in rules,
  virtual folders, search and settings.
- Create rule from message and Add to rule in the mail menus; condition
  fields offer the mail's sender, domain, recipients, subject, list,
  folder and tags.

### Changed — search (ADRs 0220, 0226)

- Search runs while typing (Enter still searches at once). Query mistakes
  show as a small badge with hover text instead of moving the layout;
  search hints and values float above the list.

### Changed — rules window decisions (ADRs 0227, 0229)

- Select several rules in the rules window (Ctrl+click, Shift+click,
  Shift+arrows, Ctrl+arrows then Space, Ctrl+A); Run runs them all in list order with one Undo,
  Test previews each rule, grouped by rule.
- One Exchange badge per rule: the number sums the reasons over all
  Exchange accounts; hover, or the row with the keyboard focus, lists each
  account (Escape hides that list).
- High contrast: a selected rule row keeps its handle, triggers, badge and
  On mark readable, and the rule the editor shows is marked among several.
- "Values from this mail" sits right after a condition's value, before
  "Match upper and lower case".
- The Run on choice stays while you pick other rules.
- The rule list's drag handle and On mark are pointer-only; the keyboard
  reorders with Alt+Up/Down and switches a rule in the editor's On switch.

### Changed — pickers everywhere (ADRs 0227, 0228)

- Account, folder and address fields in Settings use the shared picker
  everywhere (junk, server rules and vacation account, Also my addresses,
  searchable decrypted mail, MCP accounts and hidden folders, import and
  export, key lookup, automatic replies). Multi-value fields show chips.
- An empty address field lists your most-used addresses; the vacation
  reply's "Reply from" and "Also my addresses" offer only your own.
- Multi-value fields tell screen readers what they hold and announce each
  change; chosen options show a check mark.

### Added — Paper theme

- New built-in light theme Paper: warm paper neutrals with a blue accent.

## [0.8.6] — 2026-11-19

### Fixed — agents (MCP) and drafts (ADR 0217)

- `update_draft` and `send_message` also find a draft by the message id
  agents see when they read it, so drafts written in Nail or in an earlier
  agent session can be changed and sent.
- An agent whose own prompt was declined without asking (some agents do
  that when they cannot show it) no longer denies the send or delete: the
  request waits for your answer in Nail. The agent is no longer told that
  you denied it in Nail when you did not.

## [0.8.5] — 2026-11-18

### Fixed — continuing drafts

- A saved draft can be continued from the Drafts folder: double-click or
  Enter opens it in the composer instead of the reader, and the reader and
  the message menus offer Continue writing. Drafts written in another mail
  program open the same way; Nail takes them over as its own draft and
  replaces the server copy on the next save. The Outbox favorite also shows
  while saved drafts exist.

### Security (ADR 0216)

- ammonia 4.2.5 (mutation XSS through SVG and MathML, RUSTSEC-2026-0533),
  lettre 0.11.24 (STARTTLS reply injection, SMTP command injection, quoted
  local parts and denial of service, RUSTSEC-2026-0560 to 0566) and rmcp
  3.11.0 (OAuth SSRF filter and unbounded buffering, RUSTSEC-2026-0457,
  0461, 0462). chrono and chrono-tz are accepted as unmaintained.

## [0.8.4] — 2026-11-04

### Fixed (ADRs 0211, 0212)

- Links in HTML mail work on macOS and Linux: hovering shows the target, a
  click opens it (or asks first for a warned link). The text menu over mail
  (Copy, Select all) opens there too.
- On macOS and Linux, Nail's labels, buttons and lists can no longer be
  selected like text (the WebKit engine needs its own name for the rule).
- Open and Save on an attachment in the remote UI download the file instead
  of failing; every desktop-only request answers a remote session with
  `unsupported`, so the browser's own way is taken.
- An opened attachment with a very long name keeps its file type: the name
  is shortened before the extension.
- "Save as PDF…" in the print dialog asks for no file on macOS and Linux,
  where Nail cannot write the PDF itself, and points to the print dialog's
  PDF printer at once.
- A web address with an apostrophe in plain-text mail (`/it's-here`) is one
  link; an apostrophe around it is still left out.
- Opening a link or an attachment no longer blocks one of the core's worker
  threads while macOS starts the program.

### Changed (ADR 0212)

- Every mail frame runs one fixed script, by its hash, in an isolated origin;
  the remote UI gets Nail's text menu over mail as well.

### Security (ADR 0211)

- An opened attachment is marked as downloaded from the internet (the
  `Zone.Identifier` stream on Windows, the `com.apple.quarantine` attribute
  on macOS), so SmartScreen, Office's Protected View and Gatekeeper check it
  like a downloaded file.

## [0.8.3] — 2026-11-02

### Added (ADR 0211)

- A click on an attachment's name (or its new Open button) opens the file
  with the system's program; a flagged attachment asks first. In a browser
  the file downloads.
- URLs in plain-text mail are links.

### Fixed (ADR 0211)

- "Save as" for attachments works on macOS and Linux (native file dialogs
  through `rfd`).
- A link or file the system could not open is reported on macOS instead of
  being dropped silently.

## [0.8.2] — 2026-11-01

### Security (ADR 0209)

- Plug-in packages and backups are read with `zip` 9.2 and refused when
  their local headers disagree with the central directory or a name occurs
  twice, so no other zip reader can see different files than the ones Nail
  checked. A restored plug-in needs a valid plug-in id.
- Updated `rustls` (0.23.46), the mail HTML sanitizer `ammonia` (4.2.4),
  `rustls-webpki`, `webpki-roots`, `tokio`, `zstd` and the search engine
  `tantivy` (0.27) for published advisories; development tools updated
  (`vite`, `vitest`, `undici`, `markdownlint-cli2` and others).
- A release now stops while GitHub reports open Dependabot alerts, and
  checks the RustSec and npm advisories of what it ships (development
  setup only).

### Changed (ADR 0210)

- Update channels are gone: Nail asks one update address, the latest
  release on GitHub (`releases/latest/download/latest.json`), and
  `release:publish` adds `latest.json` to the version's own release and
  marks it Latest. Pre-releases are never offered as updates. The channel
  choice in Settings → Updates and "About Nail", the `UpdateChannel` policy
  value (now ignored) and the rolling `stable` / `beta` releases are
  removed.

## [0.8.1] — 2026-10-31

### Fixed (ADRs 0206, 0207)

- Links in HTML mail open outside the reader (the system browser in the
  desktop app, a new tab in the browser) instead of loading inside the
  message frame; the reader shows a hovered or focused link's target and
  asks before opening a link Nail warns about (ADR 0206).
- The message warnings list shows at most three entries, groups identical
  link warnings and keeps long targets on one line (ADR 0206).
- The desktop app reloads a window whose page crashed or hung, logs the
  failure and the page's errors and memory use, and shows an error screen
  instead of a blank window (ADR 0207).
- The remote-content buttons fit a narrow reader (labels end in an
  ellipsis); calendar reminder notices are capped.

### Changed (ADRs 0205, 0208)

- Only the master checkout publishes `main`, and branches are rebased,
  never merged (ADRs 0205, 0208; development setup only).

## [0.8.0] — 2026-10-26

The version is 0.8.0: the internal candidate `1.0.0-rc.1` was
never published and is renumbered; 1.0 follows once the gates in
[`docs/release-checklist.md`](docs/release-checklist.md) are passed.

### Added (ADR 0203)

- **Releases on GitHub**: `npm run release:prepare -- --version X.Y.Z`
  builds a release (version, changelog, full verification, the x64 NSIS
  installer and portable zip with update signatures, update manifest,
  release commit); `npm run release:publish` tags it, pushes it and creates
  the GitHub release. Installed copies find updates in the rolling `stable`
  and `beta` releases of the repository.
- The version has one source (`Cargo.toml`), and the release notes too:
  the GitHub release and the update dialog take their text from
  `userDocs/en/release-notes.md`.

### Changed (M5b, ADRs 0109–0114)

- **Navigation rail and views**: Mail, Calendar, Contacts, Tasks and
  Settings are views behind a rail on the left (Ctrl+1 … Ctrl+4, `g m`),
  with the unread count and pending confirmations as badges; Nail reopens
  the last view.
- **Mail view**: a folder pane with New message, Favorites, one section per
  account (with its colour) and smart folders; a list header with search,
  counts and the filter chips All / Unread / Flagged / Attachments; one
  ranked action bar with "More" and matching right-click menus.
- **Settings** are one searchable page grouped into General, Look & feel,
  Security and Extensions (the old tabs open as links to their section).
- **Calendar**: a one-line toolbar with a segmented view switch, calendar
  menus instead of buttons, readable event cards.
- Layouts of your own keep working; Nail offers the new default once
  through the merge dialog.

### Changed (post-M4c, ADR 0101)

- **Faster mail sync**: fetching, storing raw mail and indexing now overlap;
  a first sync of a 100,000-message mailbox takes about a third of the time,
  checking for new mail in a large mailbox about a quarter.
- **Replacing files from the Save dialog**: when Windows asks "Replace?"
  and you confirm, backups, recovery kits, diagnostics bundles and PDF
  files replace the existing file (atomically); a typed path to an existing
  file is still refused.

### Fixed

- A plug-in's CPU budget and hourly quota counted elapsed time, so on a busy
  PC a well-behaved plug-in could be stopped for "using too much CPU" while
  it was only waiting for a processor. They now count the CPU time of the
  plug-in's own thread (ADR 0204).
- A message you marked as junk could be reported as "marked by the server"
  when its arrival was classified after your mark (its `$Junk` keyword then
  looked like the server's verdict); your own verdict now always stands.
- The look-alike sender warning ran its two sentences together and showed
  its advice in the monospace style of link addresses.
- `nail serve --seed demo-multi` keeps its secrets in the demo keychain
  file, so encryption at rest survives a restart there too.
- Plug-in hot reload in `nail plugin dev` could stop watching a folder, or
  restart a plug-in that was just disabled, when changes arrived quickly.

## [1.0.0-rc.1] — 2026-10-02 (internal, renumbered 0.8.0)

The first release candidate for Windows: everything planned for version 1.0
(plan milestones M0–M4) is in. What still stands between this candidate and
1.0 are the human gates in [`docs/release-checklist.md`](docs/release-checklist.md)
(security review, a signed build, real-account tests, the first green CI
run, the release update key).

### Added — M4c: data portability, release hardening, leftovers (ADRs 0094–0099)

- **Import** from mbox (mboxrd / mboxo / mboxcl, streamed, any size), folders
  of `.eml` files, Maildir (Dovecot / Courier keywords, flags), Thunderbird
  profiles (local folders, POP3 mail, address books from `abook.sqlite` /
  `history.sqlite` / Mork `.mab`, accounts and filters offered as proposals,
  never passwords), `.ics` and `.vcf`; mail goes into **Local Folders** or an
  existing folder of an account (through its operation queue); repeated
  imports skip what is already there; per-item reports.
- **Export** of folders and messages to mbox, `.eml` or Maildir, calendars to
  `.ics`, address books to `.vcf`; an exported folder imports again
  byte-identically.
- **Backups**: one encrypted `.nailbackup` file (Argon2id + XChaCha20-Poly1305)
  with settings, theme, layout, rules, accounts (without secrets), local mail,
  local calendars and contacts, plug-ins and — only when chosen — keys;
  checking a backup, a dry run before restoring, restores that replace
  configuration files (kept as configuration backups) and merge the rest.
  Restored settings are checked like any change; smartcard, remote access
  and AI agent (MCP) settings stay on their computer.
- **Encryption at rest of the whole database file** (SQLite3 Multiple Ciphers)
  and the **vault recovery key**: shown once, checked, and the way back to the
  data after the key in the Windows Credential Manager was lost.
- **Smartcards and tokens (PKCS#11)** for S/MIME and OpenPGP signing and
  decryption, with PIN prompts in the UI; PINs are never stored.
- **Command palette** (Ctrl+K): every action, fuzzy search, shortcuts,
  recently used actions first.
- **Printing** of messages, conversations, events and agendas with a print
  look of the theme (`print` mode, `print.*` parts); **Save as PDF** in the
  desktop app (WebView2), the print dialog's PDF printer in a browser.
- **Diagnostics bundle** for support: exactly what goes in is shown first;
  passwords, tokens and keys are always removed, addresses, subjects and the
  user name in paths by default; mail only when picked.
- File dialogs of Windows for every path in the desktop app.
- Exchange and Gmail leftovers from M3: large messages (over 4 MB) go up to
  Microsoft Graph in pieces with send progress; send as / on behalf of shared
  mailboxes; server conversations as Nail threads; folder sync policies
  (headers only, recent days) for Exchange folders with bodies on demand;
  moving messages between Exchange mailboxes and between Exchange / Gmail
  accounts.
- A 100k-message performance benchmark with budgets (initial and incremental
  sync, search, list paging, cold start, memory, first render, scrolling).

### Fixed — M4c

- Initial synchronization was quadratic: the subject-fallback threading query
  scanned every message instead of its index.
- Moving a message from an IMAP or POP3 account into an Exchange or Gmail
  folder now files it on the server (it was only kept locally).

### Added — M4b: Windows integration and distribution (ADRs 0087–0093)

- Installers: per-user NSIS (x64 / ARM64, offline WebView2 variant), per-machine
  MSI with policies (`DISABLEUPDATES`, `AUTOSTART`, `ADDTOPATH`, `MAPI`), a
  portable zip, SBOMs, winget manifests.
- Updates with signed manifests (stable / beta channels, disabled by policy for
  MSI installs), tray icon, close to tray, start with Windows, default apps.
- Simple MAPI ("Send to → Mail recipient" from other programs): the composer
  opens with the files, nothing is ever sent without the user.
- The welcome screen and "About Nail".

### Added — M4a: agents and full extensibility (ADRs 0079–0086)

- AI agents (MCP) may write — drafts, flags, tags, moves, rules, calendar —
  through the operation queue; destructive and outgoing actions need a
  confirmation in Nail; policies per client; an activity log.
- Plug-ins: write permissions, mail hooks (arrival, before send, open),
  composer extensions, attachment previews, link actions, background jobs,
  MCP tools; the SDK and three example plug-ins.

### Added — M3: calendar, contacts, tasks, Gmail API, Exchange (ADRs 0058–0078)

- Calendars, contacts and tasks: local, ICS subscriptions, CalDAV / CardDAV;
  repeating events with this / this-and-following / all, reminders, contact
  merging, tasks with subtasks.
- Invitations and scheduling (iTIP / iMIP, free/busy, server-side scheduling).
- Gmail through the Gmail API (labels as tags, categories).
- Microsoft Exchange: Exchange Online (Graph) and Exchange Server (EWS, Windows
  sign-in), Autodiscover, shared mailboxes, categories, Focused Inbox,
  automatic replies, server rules, rooms, Microsoft To Do.

### Added — M2: security, remote UI, server-side rules, plug-in preview (ADRs 0029–0057)

- OpenPGP (with Autocrypt) and S/MIME: signing, encryption, key management,
  trust, warnings; encryption at rest of the data directory.
- The remote UI: HTTPS, users with passwords, two-step verification and
  passkeys, paired devices, a security log; remote content through a privacy
  proxy.
- Server-side rules (Sieve / ManageSieve) and vacation replies, junk filtering,
  snooze, notifications, folder sync policies and bodies on demand.
- Plug-ins in a sandbox (preview), the declarative plug-in UI, the plug-in
  manager.

### Added — M1: daily-driver mail (ADRs 0012–0028)

- IMAP (IDLE, CONDSTORE / QRESYNC), POP3, SMTP, OAuth2 sign-in, account
  discovery, the offline operation queue, the outbox with undo send, virtual
  (combined) accounts.
- The composer (plain text, HTML, Markdown), filter rules with undo, the MCP
  server for AI agents (read access), customization (inspector, toolbars,
  shortcuts, columns, shared themes and layouts).

### Added — M0: foundations (ADRs 0001–0011)

- The Rust core, the store, search, the React UI rendered from `layout.kdl` and
  styled by `theme.kdl`, the web mode (`nail serve`) and the Tauri desktop
  shell.

[0.8.10]: https://github.com/Nusi42/Nail/releases/tag/v0.8.10
[0.8.9]: https://github.com/Nusi42/Nail/releases/tag/v0.8.9
[0.8.8]: https://github.com/Nusi42/Nail/releases/tag/v0.8.8
[0.8.6]: https://github.com/Nusi42/Nail/releases/tag/v0.8.6
[0.8.5]: https://github.com/Nusi42/Nail/releases/tag/v0.8.5
[0.8.4]: https://github.com/Nusi42/Nail/releases/tag/v0.8.4
[0.8.3]: https://github.com/Nusi42/Nail/releases/tag/v0.8.3
[0.8.2]: https://github.com/Nusi42/Nail/releases/tag/v0.8.2
[0.8.1]: https://github.com/Nusi42/Nail/releases/tag/v0.8.1
[0.8.0]: https://github.com/Nusi42/Nail/releases/tag/v0.8.0
[1.0.0-rc.1]: docs/release-checklist.md
