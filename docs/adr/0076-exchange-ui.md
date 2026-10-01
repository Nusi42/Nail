# ADR 0076: Exchange UI (WP94)

Status: accepted · 2026-10-01 · M3c WP94

## Context

Plan section 8, wave M3c, WP94: shared mailboxes and delegate folders in
the folder tree, the room finder in the event editor, the out-of-office
editor for Exchange accounts, Exchange rule placement badges, the Focused
Inbox toggle / view, To Do lists in the task widget, category colours, and
the Exchange parts of account setup (Exchange / Microsoft 365 choice,
Autodiscover confirmation with a domain-redirect warning, Windows sign-in
for on-premises servers) — against the contract of ADR 0072, with realistic
mocks, built in parallel with WP91–WP93. WP94 owned only
`ui/src/features/exchange/` and `locales/{en,de}/exchange.ftl`; everything
else it needs is listed for the integrator (WP95).

## Decisions

### Mount points instead of edits to shared components

Account setup / settings, the folder tree, the reader, the event editor,
the rule list and the task widget belong to other owners, so the feature
exports components the integrator mounts:

| Component | Where | Props / behaviour |
| --- | --- | --- |
| `ExchangeAccountType` | setup, address step | `choice` (`auto` / `graph` / `ews`), `host`, `onChange(choice, host)`; with `graph` / `ews` the setup skips `DiscoverAccount` and uses `exchangeCandidate(email, engine, host)` |
| `ExchangeServerReview` | setup, servers step, when `candidate.incoming.protocol === "exchange"` | `email`, `candidate`, `onChange`, `confirmed`, `onConfirm`; shows source / engine / server / EWS address, the redirect warning with an "I trust …" checkbox (`needsRedirectConfirmation(email, candidate)` → keep "Test" disabled until confirmed), `ews-retired-online`, the engine switch (`withEngine` swaps host + auth) |
| `ExchangeSignInFields` | setup + settings, instead of the generic sign-in select for Exchange accounts | `email`, `engine`, `auth`, `onChange`, `password`, `onPasswordChange`; Graph: OAuth only; EWS: Windows user (no secret), other Windows account (`DOMAIN\user`, package), password (Basic over TLS), OAuth. Use `exchangeCredentialsReady` / `exchangeSecretsFor` (only the `incoming` endpoint; nothing for the signed-in Windows user) instead of `secretsFor` |
| `ExchangeOptionsFields` / `ExchangeStatusPanel` / `AutomaticRepliesEditor` | account settings of an Exchange account | `options` / `onChange`; `accountId`; `accountId` |
| `MailboxFolderBadge` | folder-tree rows | `folder`; a badge on another mailbox's root (`@<address>`) with kind and non-synced state |
| `MessageCategories` | list rows, reader | `message`; the message's categories (tags) as chips in master-list colours; nothing for other mail |
| `FocusMoveButton` | reader toolbar | `message`; "Move to Other / Focused" (opens `exchange-classify`) |
| `RoomFinder` | event editor, below the attendees | `calendarId` (or `accountId`), `start`, `end`, `attendees`, `onAttendeesChange`, `location`, `onLocationChange`, `tz`; nothing outside calendars of Exchange accounts |
| `ExchangeRuleBadges` | rule list rows | `ruleId`; per Exchange account "on the server / in Nail / in Nail only" with reasons |
| `TaskListChooser` / `TaskListBadge` | task widget | the list `<select>` with groups (each account's To Do lists, other mailboxes, other lists); a badge per list |
| `EXCHANGE_BASE_CSS` | `theme/compile.ts` | structure only (lists, swatches) |

The same features also work on their own through widgets, dialogs and
actions, so a layout can use them before anything is mounted.

### Widgets, dialogs, actions

- Widgets: `focused-tabs` and `automatic-replies-banner` (registered by
  WP90 as future widgets), and three new kinds the integrator registers in
  `nail-config/src/widgets/exchange.rs`: `exchange-mailboxes` (folder pane:
  other mailboxes with kind, origin, what they show, state; add / edit /
  remove), `exchange-categories` (folder pane: master list with colours and
  counts, tag views, edit / delete), `exchange-rules` (settings: state,
  sync, the `server-rules` switch, placements, the server's inbox rules
  with on / off and delete).
- Dialogs: `exchange-mailbox-editor`, `exchange-mailbox-remove` (hides an
  automapped mailbox instead), `exchange-category-editor` (25 presets +
  none as a radio group, rename warning with the message count),
  `exchange-category-delete` (the tag stays), `exchange-automatic-replies`,
  `exchange-classify` (with "always for this sender").
- Actions: `exchange.add-mailbox`, `exchange.new-category`,
  `exchange.automatic-replies ["<account>"]`, `exchange.focus
  ["focused"|"other"]`, `exchange.classify ["focused"|"other"]`,
  `exchange.toggle-focused`.

### Behaviour

- **Focused Inbox** is a search, like Gmail categories: a tab sets the
  inbox's search to `focus:focused` / `focus:other` (WP95 adds `focus:` to
  the query language and the mock search). "All" clears it. The
  "Focused Inbox" switch is a view choice per account kept in the
  feature's store (Outlook's "Show Focused Inbox"; nothing is written);
  the server's `focused-inbox-unavailable` warning replaces the tabs with a
  note. Moves go through `ClassifyFocused` with an optimistic list patch and
  the status bar's pending ops (`queued`).
- **Categories** are tags: a category view is `tag:"Name"` (account scope);
  colours come only from `CategoryColor` presets (`PRESET_RGB` mirrors
  `CategoryColor::rgb`; text colour by WCAG luminance) through theme
  geometry, never inline styles. Tags of Exchange messages outside the
  master list show without colour.
- **Automatic replies**: plain text; status off / on / scheduled; the
  schedule uses the PIM `DateTimeInput` (React Aria segments under the UI
  locale, not native inputs) in the viewer's zone and is stored as UTC;
  the schedule is only sent with `scheduled`; external reply hidden for
  audience `none`; "not on the server yet" while `sync = pending`. The
  banner shows while replies are on or a schedule is upcoming, with "Turn
  off" and "Edit…"; it is a plain container (no empty landmark).
- **Rooms**: `ListRoomLists` / `FindRooms` for the event's range (all-day
  events: whole days in the viewer's zone); filters room list, minimum
  seats, only free; rows show seats, building / floor, accessibility,
  equipment, free / busy (with busy times) / unknown. Adding books the
  room as an attendee (`kind: room`, `role: non-participant`, RSVP) and
  fills an empty location; booked rooms show their answer and "Remove". A
  `forbidden` answer shows the core's `exchange-warning-rooms-need-consent`.
- **Mailboxes**: the editor mirrors `ExchangeMailboxSettings::issues`
  (plus duplicates) for immediate feedback; the core stays authoritative
  and its `localized` refusal is shown. A synced mailbox with mail opens
  its root folder.
- **Setup**: the redirect warning appears when the Exchange host (EWS URL's
  host first) is outside the address's domain and not a Microsoft host, or
  when discovery flagged `foreign-hosts`.

### Mocks

`mock.ts` answers all 18 requests on `data.features.exchange`: mailboxes
(directory with access, `pending` → `synced` with a folder tree under
`@<address>` / `not-found` / `access-denied`, hiding automapped ones),
categories (counts from tags, `local-…` ids replaced on "replay", rename
retags messages, delete keeps tags), `ClassifyFocused` (with sender
overrides), automatic replies (queued → synced), rules (placements of the
rules mock's rules — query / tag / age conditions stay in Nail with the
existing `sieve-reason-*` texts until WP93's `exchange-rule-reason-*`
exist —, on / off, delete, sync), room lists and rooms (daily busy windows,
an unreadable room, `Place.Read.All` missing switch, range checks with
`freebusy-issue-*`), status with pending ops, `OpStatus` events. Every
request answers `invalid_request` for non-Exchange accounts and
`unsupported` with `state.unsupported`. `seedExchangeDemo(data)`
(`mockSeed.ts`) adds the Contoso account (Graph), folders incl. the
automapped `support@` tree, Focused / Other mail with categories, a
delegate calendar, To Do lists with tasks in the `exchange:acc-exchange`
PIM source, rules, rooms and automatic replies.

## Consequences

- Everything is themeable through `data-part`s; strings are in
  `exchange.ftl` (German file stays empty); Vitest covers the model,
  every mock request, the widgets, dialogs, actions and mount points, and
  axe scans of every widget, mount point and dialog (86 tests in 7 files).
- Not built here (listed for WP95): mounting the components, nail-config
  option specs of the five widgets, theme rules for the new parts and
  `EXCHANGE_BASE_CSS` in `compile.ts`, `focus:` in `mockSearch.ts` /
  `nail-search`, `seedExchangeDemo` in `createMockData` (it adds an
  enabled account: suites that count accounts or unified rows need
  updating, as with Gmail), default layout placements, Playwright
  scenarios.
