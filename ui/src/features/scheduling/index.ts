/**
 * Feature slot of M3b WP84 for invitations and scheduling (contract
 * ADR 0065): the invitation card in the reader (Accept / Tentative /
 * Decline / Propose new time with an optional comment, conflicts preview,
 * the organizer's view of replies and counter-proposals), the attendee
 * picker with the free/busy timeline in the event editor, the organizer's
 * update / cancel flows, scheduling settings.
 *
 * Only WP84 edits the files in this directory (plus
 * `crates/nail-config/src/widgets/scheduling.rs` and
 * `locales/{en,de}/scheduling.ftl`). The central registry
 * (`../registry.ts`) merges these exports into the app; see
 * `../README.md`.
 */
import type { Event as NailEvent } from "../../api/generated";
import type { Feature, FeatureEventHandler } from "../types";

/** Query keys of this feature. */
export const schedulingKeys = {
  all: ["scheduling"] as const,
};

/**
 * The core's events that change invitation cards or free/busy (a reply
 * was applied, the user's calendars changed, settings changed).
 */
export const SCHEDULING_EVENTS = [
  "InvitationsChanged",
  "SchedulingSettingsChanged",
  "EventsChanged",
  "CalendarsChanged",
] as const satisfies readonly NailEvent["type"][];

/** Layout widget kinds → components. */
export const widgets: Feature["widgets"] = {};

/** Action ids → handlers. */
export const actions: Feature["actions"] = {};

/** Dialog names → components. */
export const dialogs: Feature["dialogs"] = {};

/** Invitations or calendars changed: refetch what shows them. */
export const handleSchedulingEvent: FeatureEventHandler = (event, { queryClient }) => {
  if ((SCHEDULING_EVENTS as readonly string[]).includes(event.type)) {
    void queryClient.invalidateQueries({ queryKey: schedulingKeys.all });
  }
};

/** Handlers for the core's events. */
export const events: Feature["events"] = [handleSchedulingEvent];
/** Background components. */
export const effects: Feature["effects"] = [];
