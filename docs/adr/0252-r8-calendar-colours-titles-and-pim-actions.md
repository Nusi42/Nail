# 0252 — R8 calendar and PIM UI: colours that stay apart, titles that fit, PIM actions

Status: accepted (2026-11-24)

## Context

The R8 contract (ADR 0251) gives the calendar and PIM UI package two R7 defects and the PIM backlog rows
"Answering one occurrence from the quick look", "Drawing events on touch screens", "Icons for place and call",
"Right-click menus for PIM items", "Conflict choice for events", "Recent mail on the contact card", "Undo for
invitation answers", "Quick create elsewhere" (the palette command) and "Free time" (the UI side).

- **Colours.** The mock CalDAV server seeds its `personal` calendar with `calendar-color` `#3366ccff`; the core
  gives the other calendars palette colours in order, so the demo showed that blue next to the palette's blue
  (`#2563eb`, OKLab distance ≈ 0.05). The demo-multi holiday subscription's `#d33` sat as close to the palette's
  red (≈ 0.02). Real servers do the same: a colour set elsewhere can come close to the one Nail handed out.
- **Titles.** In a taller event card (45 minutes and more) the icons (repeat, people, reminder, unanswered) sat
  absolutely positioned over the top right corner of the card, with the card's background. The title wraps to
  the full width under them, so a title that fits the card had its last characters covered; nothing overflowed,
  so `scrollWidth` did not show it. At 1280 px an unanswered repeating meeting with a reminder ("1:1 with …",
  four icons) lost the end of its title.

## Decision

1. **Seed colours.** The mock CalDAV server's `personal` calendar is fuchsia (`SEED_PERSONAL_COLOR`,
   `#c026d3ff`, one constant the testkit exports and the tests use), the demo-multi holiday subscription ochre
   (`#a16207`). Both are at least 0.08 (OKLab) from every palette colour and from each other; `colors.test.ts`
   pins this and the event card contrast for both.
2. **Display rule (`colors.ts` `displayColors`, the one colour helper).** Distance is the Euclidean distance in
   OKLab (`colorDistance`); two colours closer than `MIN_COLOR_DISTANCE` = 0.08 are too close to tell apart. The
   threshold stays under the closest palette pair (orange / red ≈ 0.087), so palette colours never count as too
   close to each other. Over the *visible* calendars in list order: palette colours (also written `#rrggbbff`)
   are what the user picks in Nail and what the core hands out, so they are always shown as stored and placed
   first; every other colour (a server's or a subscription's) is shown as stored unless it is too close to a
   colour placed before it, then it is shown in the first palette colour that is distinct from all placed ones.
   Hidden calendars, calendars without a colour and colour keywords are left alone. It is display only: nothing
   is written back. `colorOf` (every event card, swatch and dot) and the calendar list's squares use it,
   computed once per calendar list.
   A colour typed into "Other colour" is not a palette colour, so the UI cannot tell it from a server's; such a
   colour moves only when it is too close to another calendar's. Keeping it exactly as typed needs the core to
   record that the user chose it (a follow-up, see the report).
3. **Titles first.** A taller card is a wrapping row: the title takes the whole first line(s), the time line
   follows with the icons at its end, then the place. Icons never lie over the title; on a narrow card they
   still give way (`ICONS_MIN_CARD`). Short cards keep one line with the title first. Playwright checks every
   week-view title at 1440×900 and 1280×800: not cut (`scrollWidth`), not clamped (`scrollHeight`) and its last
   character not covered (`elementFromPoint`), with an unanswered repeating invitation with a reminder added.
4. **Free time.** The free-time view sends `FindFreeTime` with `hours: working-hours`, or `custom` when the
   layout gives the view its own `work-start` / `work-end` (then on the preferences' work days); it only sorts
   the slots by day (`slotsOfDay`). `freeStretchesOfDay` is gone.
5. **Touch drawing.** A touch on an empty slot draws only after the finger is held still for `TOUCH_HOLD_MS`
   (400 ms); a finger that moves more than `TOUCH_SLOP` (8 px) first scrolls, a quick tap works like a click.
   While a finger draws, a non-passive `touchmove` listener keeps the grid from scrolling; the slots use
   `touch-action: manipulation`. Mouse and pen draw at once as before.
6. **Right-click menus.** `PimMenu` and the new `usePimContextMenu` share one popup (`PimMenuPopup`) with one
   keyboard model. The menu opens at the pointer (a portal on the page, so containers with container queries
   cannot misplace it) or, from the menu key or the layout's `menu.context` keys (Shift+F10), under the item, and
   gives the focus back to it. Events (time grids, month, agenda: `useEventMenu`): Open, Edit, Delete; contacts:
   Open, Edit, Delete…; tasks: Edit, Mark as done / not done, Delete. Read-only items have no Delete. The app's
   layout menu stays away because the item handles the event first (ADR 0184).
7. **Occurrence answers.** The quick look shows the invitation card's scope choice (`ScopeChoice`, without its
   day picker: the occurrence is the one clicked) for an occurrence of a recurring invitation; the default is
   the whole series, as on the card.
8. **Undo for invitation answers.** After an answer the core queued as iMIP mail (`SentItip.outbox_id`), a PIM
   toast says the answer goes out in a moment and offers Undo: `CancelSend`, then `DeleteDraft` of its
   automatic draft (the composer does not open). The calendar keeps the answer. The toast goes once the outbox
   no longer holds the entry as scheduled (it looks again when the send time has passed). The UI mock queues
   iMIP mail in its outbox like the core.
9. **Conflict choice for events.** The contact card's choice is one component (`SyncState`); the event editor
   shows it for an event in conflict and closes after `ResolvePimConflict`, since its form shows the losing
   version.
10. **Recent mail.** The contact card lists the newest `RECENT_MAIL_COUNT` = 5 messages from or to the primary
    address (`Search` with `from:"…" OR to:"…"`, every account); a click selects the message in its account and
    folder and shows the mail view (`view.show`).
11. **New event from text.** The PIM feature registers `calendar.new-event-from-text` with its palette
    description (`Feature.actionInfo`); it opens the window `pim-event-from-text` holding the calendar's quick-add
    field (`QuickAdd`, same reading and chips). The window stays open after Enter for the next event; Tab hands
    the text to the editor and closes it.
12. **Place and call icons.** `PLACE_ICON` (`map-pin`) marks the editor's place field and the quick look's
    place, `CALL_ICON` (`video`) the quick look's Join. `map-pin` must be bundled (`nail_api::icons::BUNDLED_ICONS`
    and `widgets/lucideIcons.ts`); until then a theme can provide it.

## Consequences

- A calendar list never shows two visible calendars in colours too close to tell apart unless the user picked
  the same palette colour twice.
- Agents, the palette and the views share one free-time rule (the core's).
- Touch users scroll the grid as before and draw with a held finger.
