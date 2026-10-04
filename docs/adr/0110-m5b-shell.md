# ADR 0110: M5b WP161 — the navigation rail and the view host

Status: accepted · 2026-10-03 · M5b WP161 (package ADR; the integrator may renumber)

## Context

ADR 0109 (M5b contract) defines views (a view shows a layout window), the
layout's `rail` section, the `nav-rail` / `view-host` widget kinds, the
`view.*` actions with their fallback, `UiState.activeView` and the UI-state
key `shell.view`. WP161 fills the shell slot (`ui/src/features/shell/`):
the rail, the host, badges, the remembered view, focus and keyboard
behaviour, and Calendar / Contacts / Tasks as full views — without editing
shared files.

## Decisions

### Rail (`NavRail.tsx`)

- A `<nav>` landmark named `nav-rail-label`; entries in `rail` order: the
  app mark (`show-logo`, decorative), one button per view / action,
  spacers, separators, and the account avatar last. Separators that would
  sit first, last, next to a spacer or doubled are dropped (`railEntries`);
  views the layout does not define are skipped (nail-config refuses them
  anyway).
- View buttons dispatch `view.show "<id>"` (so the palette, keys and the rail
  share one path) and carry `aria-current="page"` plus `data-state="active"`
  on the current view (`currentView`: active, else the start view).
- Accessible name = the label, plus the badge as words ("Mail, 3 unread
  messages"); the label is also a `nav-rail.label` span, visually hidden
  unless `show-labels=#true`. Tooltip and `aria-keyshortcuts` name the first
  binding that shows the view — a binding of `view.show "<id>"` or of an
  alias of it (`view.calendar` → Ctrl+2), global bindings first.
- Up / Down / Home / End move between enabled rail buttons; every button
  stays in the Tab order (six to eight buttons — a roving tab index would
  hide them from users who do not know the arrow keys).
- The rail is its own keyboard context (`global`), so keys pressed on it do
  not reach a hidden mail list.
- Action items resolve label / icon like toolbar buttons (override, else the
  layout's action definition) and are disabled when the action is.
- Account avatar: the selected account, or the first real account while
  "All accounts" or a virtual account is selected; `role="img"` with
  "Account: `<name>`", the shared `avatar` part inside (avatar palette, High
  contrast included). Not interactive: switching accounts stays in the folder
  pane / account switcher.

### Badges (`badges.ts`)

- `unread`: the unread count of the inbox(es) of `ListFolders "all"` (the
  built-in "All accounts" account; the core sums the members' inboxes).
  Refreshed by `MailChanged` / `NewMail` like every folder query — no polling.
- `confirmations`: the confirmation center's pending query
  (`features/agents/queries.ts`, refreshed by its events).
- Visible number capped at 99 (`99+`); hidden from assistive technology
  because the button's name carries it.

### View host (`ViewHost.tsx`)

- Renders the active view's window through the shared `WindowView`, inside a
  `<section>` region named "`<View>` view" (`shell-view-region`) — not just the
  view's label, because the widgets inside (contact list, task list) already
  use that name and landmarks must be unique (axe `landmark-unique`).
- **Views stay mounted** once shown (hidden with the `hidden` attribute), so
  switching back keeps the selected message, scroll positions and the
  calendar's range. Views the layout drops are unmounted.
- **Remembered view:** written to `shell.view` on every switch
  (`SetUiState`), restored once at start when the user has not switched yet;
  a remembered id the layout no longer defines is ignored. The host renders
  nothing until `GetUiState` answered (one local request), so the start view
  never flashes before the remembered one. Restoring happens only with a
  host: in the fallback (no `view-host`, views open as windows) a window
  should not pop up at start.
- **Focus:** when the view changes while focus is not on the rail (Ctrl+1…4,
  `g m`, the palette, a menu) focus moves to the new view's region
  (`tabIndex=-1`), so it never stays inside a hidden view; a rail click keeps
  focus on the button (standard navigation pattern). Nothing is focused at
  start.
- **Keyboard contexts:** a view's region carries `data-key-context` —
  `calendar` / `contacts` / `tasks` for those views (by id or window), else
  `global`. Focusing the region sets the context, so the PIM bindings work at
  once and the hidden mail list's `list` bindings (`j` / `k`) do not run in
  another view. The mail view uses `global`, not `list`, so the reader keeps
  its own bindings without the list's falling through.
- A view whose window is missing or hosts views itself shows a status text
  (`shell-view-unavailable`) instead of recursing.
- `view.show` with a host also closes a copy of the view's window that is
  open as a modal window over it.

### Look (`skin.ts`)

Tokens only (`--nail-color-rail`, accent / accent-soft for the active view,
danger + bg for badges, the avatar palette); sizes from the proposal: a
42 px button (control + 10 px) on a ~58 px rail, a 34 px app mark, 16 px
badges. `view-host.view[hidden]` keeps `display: none` against the skin's
`display: grid`. Region focus has no ring (not an operable control; the
content inside gets the usual focus rings).

## Consequences

- WP165 only has to put `nav-rail` and `view-host` into the main window and
  point the mail view at a `mail` window; the shell's tests already run
  against that arrangement (`testing.ts` `hostLayout()`).
- Mounted hidden views keep their queries alive; with five views that is
  bounded and cheaper than refetching on every switch.
- Strings: `shell.ftl` (`shell-*`); the contract ids (`nav-rail-label`,
  `view-*`) stay in `core-shell.ftl`. `features/shell/messages.ts` lists the
  prefixes for the app-wide coverage test (to be registered there by the
  integrator; `messages.test.ts` checks the folder meanwhile).
