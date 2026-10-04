/**
 * Feature slot of M3b WP84 for invitations and scheduling (contract
 * ADR 0065; UI ADR 0069): the invitation card in the reader (Accept /
 * Tentative / Decline / Propose new time with an optional comment,
 * conflicts preview, series or one occurrence, updated / cancelled states,
 * the organizer's view of replies and counter-proposals), the attendee
 * picker with the free/busy timeline in the event editor, the organizer's
 * update / cancel flows, scheduling settings.
 *
 * Mount points for the integrator (components other features render):
 * - `InvitationCard` — the reader, between the headers and the body:
 *   `<InvitationCard messageId={m.id} invitation={m.invitation ?? null} />`
 *   (renders nothing without an invitation).
 * - `AttendeePicker` — the PIM event editor, instead of its attendee list:
 *   attendees, roles / kinds, address suggestions, free/busy timeline with
 *   suggested slots (`onTimeChange` moves the event).
 * - `MeetingPanel` — the PIM event editor for a saved event with attendees:
 *   organizer "send invitations / cancel", attendee answers / new time.
 *
 * Only WP84 edits the files in this directory (plus
 * `locales/{en,de}/scheduling.ftl`). The central registry (`../registry.ts`)
 * merges these exports into the app; see `../README.md`.
 */
import type { Event as NailEvent } from "../../api/generated";
import type { ActionContext } from "../../actions/registry";
import type { Feature, FeatureEventHandler } from "../types";
import { FindTimeDialog, FreeBusyWidget } from "./AttendeePicker";
import { InvitationCardWidget } from "./InvitationCard";
import { CancelMeetingDialog, SchedulingSettingsDialog } from "./MeetingPanel";
import {
  CANCEL_DIALOG,
  FIND_TIME_DIALOG,
  FREEBUSY_WIDGET,
  INVITATION_CARD_WIDGET,
  SETTINGS_DIALOG,
} from "./names";
import { scheduleWrite, schedulingKeys } from "./queries";
import { schedulingStoreFor } from "./store";
import type { InvitationResponse } from "../../api/generated";

export { schedulingKeys } from "./queries";
export { InvitationCard } from "./InvitationCard";
export { AttendeePicker, FindTime } from "./AttendeePicker";
export { FreeBusyTimeline } from "./FreeBusyTimeline";
export { MeetingPanel } from "./MeetingPanel";
export { SCHEDULING_BASE_CSS } from "./parts";
export { CANCEL_DIALOG, FIND_TIME_DIALOG, SETTINGS_DIALOG } from "./names";
export { schedulingStoreFor } from "./store";

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
export const widgets: Feature["widgets"] = {
  [INVITATION_CARD_WIDGET]: InvitationCardWidget,
  [FREEBUSY_WIDGET]: FreeBusyWidget,
};

/** Answers the invitation of the selected message (and sends the reply). */
async function answerSelected(ctx: ActionContext, response: InvitationResponse): Promise<void> {
  const messageId = ctx.store.getState().selectedMessageId;
  if (messageId === null) return;
  await scheduleWrite(ctx, {
    type: "RespondToInvitation",
    target: { type: "message", message_id: messageId },
    response,
    comment: null,
    send: true,
    calendar_id: null,
  });
}

/** Whether the selected message has a card offering `response` (as far as the cache knows). */
function offers(ctx: ActionContext, response: InvitationResponse): boolean {
  const messageId = ctx.store.getState().selectedMessageId;
  if (messageId === null) return false;
  const card = ctx.queryClient.getQueryData<{ actions: string[] } | null>(
    schedulingKeys.invitation(messageId),
  );
  return card?.actions.includes(response) ?? false;
}

/** Action ids → handlers. */
export const actions: Feature["actions"] = {
  "scheduling.accept": {
    run: (ctx) => answerSelected(ctx, "accept"),
    enabled: (ctx) => offers(ctx, "accept"),
  },
  "scheduling.tentative": {
    run: (ctx) => answerSelected(ctx, "tentative"),
    enabled: (ctx) => offers(ctx, "tentative"),
  },
  "scheduling.decline": {
    run: (ctx) => answerSelected(ctx, "decline"),
    enabled: (ctx) => offers(ctx, "decline"),
  },
  /** Opens "find a time" (optionally with the selected message's people). */
  "scheduling.find-time": {
    run: (ctx) => {
      schedulingStoreFor(ctx.store).getState().setFindTime(null);
      ctx.store.getState().openWindow(FIND_TIME_DIALOG);
    },
  },
  "scheduling.settings": {
    run: (ctx) => {
      ctx.store.getState().openWindow(SETTINGS_DIALOG);
    },
  },
};

/** Dialog names → components. */
export const dialogs: Feature["dialogs"] = {
  [CANCEL_DIALOG]: CancelMeetingDialog,
  [SETTINGS_DIALOG]: SchedulingSettingsDialog,
  [FIND_TIME_DIALOG]: FindTimeDialog,
};

/**
 * The window model of this feature's windows (M5c, ADR 0116): tool windows
 * are non-modal with their default and minimum sizes; only confirmations
 * (`confirmation: true`) are modal. The layout's `window` properties win.
 */
export const windows: NonNullable<Feature["windows"]> = {
  [CANCEL_DIALOG]: { confirmation: true, size: { width: 520, height: 460 }, resizable: true },
  [SETTINGS_DIALOG]: { size: { width: 640, height: 560 }, min_size: { width: 480, height: 400 } },
  [FIND_TIME_DIALOG]: { size: { width: 900, height: 640 }, min_size: { width: 640, height: 480 } },
};

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
