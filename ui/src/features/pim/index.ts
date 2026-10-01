/**
 * Feature slot of M3a WP74 for calendars, contacts and tasks (contract
 * ADR 0058): calendar views (day, multi-day, week, work week, month,
 * year, agenda, free-time timeline) and the mini-month / agenda widgets,
 * the event editor (recurrence with "this / this and following / all",
 * attendees, alarms, time zones), contacts (list, card, editor, groups,
 * merge dialog), the task list and editor, reminders (snooze / dismiss),
 * sources and `.ics` / `.vcf` import.
 *
 * Only WP74 edits the files in this directory (plus
 * `crates/nail-config/src/widgets/pim.rs` and `locales/{en,de}/pim.ftl`).
 * The central registry (`../registry.ts`) merges these exports into the
 * app; see `../README.md`.
 */
import type { Event as NailEvent } from "../../api/generated";
import type { Feature, FeatureEventHandler } from "../types";

/** Query keys of this feature. */
export const pimKeys = {
  all: ["pim"] as const,
};

/** The core's PIM events (sources, collections, items, reminders, queue). */
export const PIM_EVENTS = [
  "PimSourcesChanged",
  "CalendarsChanged",
  "AddressBooksChanged",
  "EventsChanged",
  "TasksChanged",
  "ContactsChanged",
  "ReminderDue",
  "RemindersChanged",
  "PimOpStatus",
] as const satisfies readonly NailEvent["type"][];

/** Layout widget kinds → components. */
export const widgets: Feature["widgets"] = {};

/** Action ids → handlers. */
export const actions: Feature["actions"] = {};

/** Dialog names → components. */
export const dialogs: Feature["dialogs"] = {};

/** PIM data changed: refetch what shows it. */
export const handlePimEvent: FeatureEventHandler = (event, { queryClient }) => {
  if ((PIM_EVENTS as readonly string[]).includes(event.type)) {
    void queryClient.invalidateQueries({ queryKey: pimKeys.all });
  }
};

/** Handlers for the core's events. */
export const events: Feature["events"] = [handlePimEvent];
/** Background components. */
export const effects: Feature["effects"] = [];
