# ADR 0163: P2 merge leftovers — updater announcements, a ready update stays ready, test clocks

Status: accepted · 2026-10-05 (amends ADRs 0087, 0090, 0091, 0150, 0153, 0160)

## Context

After P2 (conversations) passed its verify, the maintainer's side package `work-6`
(ADRs 0160–0162: control widths, accent in High contrast, inline images) was
merged into `work-1`. The merge brought along five leftovers from the P2 runs
and the maintainer's review screenshots:

1. `desktop-m4b.spec.ts` "the updater finds a newer version …" failed once
   under load: the status showed "ready to install", then fell back to
   "Downloading version 1.0.1… 100 % downloaded" and stayed there.
2. In `threads="expandable"` (Design B) the screenshots' conversation rows
   had no "Sender: preview" line.
3. The event editor's date buttons ("Choose…") had their calendar icon
   touching the text, and "Change which occurrences?" was framed in the
   accent colour while "When" and "Repeat" have a thin border.
4. `exchange.spec.ts` failed on the morning of 2026-10-05 because reminder
   toasts covered buttons: the mock tenants hold events dated 2026-10-05
   with 15-minute reminders, which the core catches up (up to two hours late)
   as soon as the account syncs.
5. The merge itself: `MessageView.tsx` (P2's in-card body vs. `work-6`'s
   `AttachmentList`) and the theme reference's message list / view rows.

## Decision

- **Updater: a downloaded release stays ready.** `Updater::check_finished`
  keeps `ready-to-install` when a later check (automatic or "Check now")
  fails, finds nothing or finds the same version; the package is never
  downloaded again. Only a different version replaces it (download as
  before). Unit tests: `a_ready_release_stays_ready_and_is_never_downloaded_again`,
  `a_different_version_replaces_the_ready_one`.
- **Updater: announcements are serialized.** The cause of failure 1 was not
  a second download — "100 %" never moved — but an older state announced
  last: the core's notifier (`Service::desktop_changed`) reads the stored
  update settings (a synchronous store read, slow under load) and then the
  host state, and broadcasts it. `check()` announces on the request's thread
  while the check / download task announces on its own, so a read taken at
  "downloading 100 %" could be broadcast after "ready". `Updater::changed`
  now holds `Shared::notify_order` while it calls the notifier, so the last
  broadcast always carries the latest state. Unit test
  `announcements_never_overtake_each_other` (fails without the lock with
  exactly the e2e's sequence).
- **UI: an event wins over a running status fetch.** `UpdateStatusChanged`
  cancels a `GetUpdateStatus` fetch still in flight (the refetch after
  "Check now") before it sets the status, so an older answer arriving over
  the other channel cannot undo it (Vitest in `features/system/mock.test.ts`).
- **Design B keeps the preview line.** The product was right: the default
  layout's rows are the same for every `threads` mode, so expandable rows
  show "Sender: preview". `p2.spec.ts`'s own Design B layout had left the
  third row out; it now has it, and a new test checks the preview and the
  responsive standard (incl. `undersized-control`) of the open list at
  1600, 1024 and 800 px.
- **Event editor (theme only).** `pim.datetime.pick` is an inline flex box
  with `gap "$space.xs"`; `pim.editor.scope` has the thin border of
  `pim.editor.when` / `pim.recurrence` (choices still side by side).
- **Reminder toasts vs. tests.** Toasts stay where P2 put them (bottom
  right, above windows; ADR 0153 moved them off the rail): a reminder is
  meant to be seen and is dismissed or snoozed by hand. The bug was the
  test's dependence on the wall clock: `exchange.spec.ts` sets the core's
  clock (`clock-set`) to 2026-10-01 08:00 UTC, before every dated item of
  the mock tenants, so no reminder comes due or is caught up while it runs.
  Reminder toasts are covered by `pim.spec.ts`.
- **Paper padding (found in the review screenshots).** In the dark themes'
  smart mode, designed HTML mail is drawn on a light paper (ADR 0140); its
  text touched the paper's edge, because the reader's body has no side
  padding. Any mail with a `<style>` counts as designed — the sanitizer turns
  a `<body style>` into one, and Outlook always sends one — so this was most
  HTML mail. The paper now has `body { padding: 12px 16px 24px }` (the
  mail's own body padding still wins); light mode and plain-looking mail are
  unchanged. The dark themes' visual baselines with HTML mail on paper were
  updated for it.
- **Merge.** `MessageView` renders `AttachmentList` (cards hide inline
  `cid:` images, ADR 0162) and P2's `Body` with `quotesFold`, in the reader
  and in every card of the feed; the theme reference lists P2's list parts
  and `work-6`'s attachment-list parts. The `toolbar-customizer` baselines
  that both branches changed were regenerated after the merge. P2's list
  rows, card feed and Design B children pass the responsive helper with
  `undersized-control` in Comfortable and Compact at 1600 / 1024 / 800 px.

## Consequences

- "Restart to install" no longer disappears for good after a failed or
  empty re-check; a ready update is installed on quit as before.
- Specs that sync the mock Exchange tenants should pin the core's clock the
  same way when they add dated items with reminders.
