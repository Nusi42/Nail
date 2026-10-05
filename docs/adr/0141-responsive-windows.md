# ADR 0141: P1 WP191 — responsive windows, dialogs and settings

Status: accepted · 2026-10-05 · P1 WP191 (windows and dialogs; integrated by ADR 0144)

## Context

The P1 contract (ADR 0139) defined the responsive standard and the helper
`e2e/support/responsive.ts`. WP191 owns the window manager and every window and
dialog outside the mail and PIM views, plus the settings page. Sweeping them at
the eight standard sizes, at their min-size, resized larger and maximized found
two kinds of fault:

- mechanical ones the helper reports: every window kept its place when the page
  shrank, so at 1024 and 800 px its title bar, Maximize and Close stood off the
  right edge (an `offscreen` finding on every window but the smallest); the composer, the
  plug-in installer and several wizards overlapped at their min-size; label
  editors, the snoozed list, the cancel-meeting dialog and the rule editor
  scrolled sideways; the account settings form was cut off in a narrow card;
- visual ones only screenshots show: settings rows with ragged control columns
  and labels not centred beside their control, checkboxes glued together on one
  line, button rows without gaps, unstyled view switchers and file pickers, the
  free/busy rows broken into the wrong grid cells, dialogs that stop at a fixed
  width in a wide window, button rows that scroll out of reach, doubled
  paddings in small windows, "All configuration files are valid." in the error
  colour.

## Decisions

### Windows placed on the page stay on it

`shownRect` (`features/windows/geometry.ts`) takes `placedOnPage`: a window
that lay wholly on the page when it was placed (opened, moved or resized) is
shown wholly on a page that shrinks — moved in from the right and bottom edges
as far as needed — and gets its remembered place back when the page grows. A
window the user dragged partly off the page keeps `WINDOW_MIN_VISIBLE` pixels on
it as before (ADR 0117). `WindowFrame` remembers the page size a geometry was
placed on (state adjusted during render, no effect) and passes the flag. The
remembered geometry itself never changes on a resize.

### One responsive skin, container queries, no class names

`theme/skinResponsive.ts` (the contract's slot, emitted after every feature
skin and before the theme's part rules) holds the shared rules; feature rules
stay with their feature where one has a skin or parts file
(`features/settings/skin.ts`, `features/composer/skin.ts`,
`features/plugins/parts.ts`, `features/scheduling/parts.ts`). Layout adapts to
the width of the card or window, not the viewport: window bodies and settings
cards are size containers (`nail-fields`), the settings content is one too
(`nail-settings-content`). Where a rule must beat a default `theme.kdl` rule of
the same part, its selector names the part's place (inside a window body, a
settings card, a list), which is more specific than the theme's bare part
selector; a user theme can still override it with a more specific rule. Where a
rule must lose to the theme, it uses `:where()`.

- **Window bodies** pad; the dialog root inside does not pad again and may
  shrink below its content's width (it wraps instead of scrolling sideways).
  Dialogs with a theme `max-width` (`scheduling.dialog`, `gmail.dialog`) grow
  with the window.
- **Button rows.** Every `….actions` / `….buttons` row of buttons wraps with a
  gap. The last button row of each dialog (`DIALOG_BUTTON_ROWS`, only when it is
  its parent's last child) sticks to the bottom of the window while the body
  scrolls; it reaches into the body's bottom padding and fills it
  (`bottom: -pane`, `padding-bottom: pane`), so nothing scrolls by below it
  whether the window body or the dialog itself is the scroller. The composer's
  footer does the same with the composer's padding. Nested dialogs keep only
  the outer row sticky (two sticky rows would cover each other).
- **Settings cards.** One control column per card (`16 × space.lg`): every
  control starts at the same left edge and has the same width; the label is
  centred beside a control without a hint. Below `30rem` of card width the label
  goes above its control. A widget's sections without a layout of their own
  stack their rows with a gap. The page pads less below `40rem` / `30rem`.
- **Two panes** (account settings, filter rules) stack below `36rem` of their
  card or window; the account list then wraps.
- **Per feature:** key rows in aligned columns and the key's facts as a
  two-column list (security); inline field rows (key filters, "Client name
  [ ] Create"); Waiting / History and Editor / Text as segmented switchers;
  "Load a file" as a button whose input is visually hidden instead of
  `display: none` (the keyboard reaches it now); portability choice rows
  centred; the free/busy rows (person and source in the name column, the
  track beside both, the hour scale aligned with the tracks) and day buttons;
  the plug-in installer's source form; the valid-configuration status in the
  success colour; the setup steps with their numbers inside each step.
- The cancel-meeting confirmation (resizable) gets a min-size of 400 × 340
  instead of the confirmation default 320 × 160.

### The sweep checks itself and what the helper cannot see

`e2e/tests/p1-windows.spec.ts` sweeps five groups (mail-area windows, settings
sections, settings dialogs, scheduling, Gmail) in parallel, each on its own
`nail serve --seed demo-multi`. A coverage test reads the dialog list of
`a11y-windows.spec.ts` from its source and requires every layout window and
dialog to be in a group or in the list of windows other P1 packages check, and
each group asserts it checked exactly its windows. On top of the helper the
spec reports panes that scroll sideways (the helper skips what such a pane
hides), sticky rows outside their window, and sticky rows on top of each other;
for the overlap and clipping checks it puts sticky rows back into the flow, since
covering the content that scrolls beneath them is their purpose. Findings go to
`<build dir>/screens/p1/windows-<group>.json` next to the screenshots.

## Consequences

- Visual baselines change for the settings screens and every dialog screen
  (spacing, aligned control columns, sticky button rows); the integrator
  regenerates them.
- New dialogs get the shared rules for free; a dialog with its own final button
  row adds its part to `DIALOG_BUTTON_ROWS` (a unit test checks every listed part
  exists in a component).
- Remaining items for later: the desktop-only settings cards (Updates, Tray and
  startup) show only their titles in `nail serve`; the diagnostics dialog's
  first button row ("Show what will be included") scrolls with the content (only
  its last row sticks).
