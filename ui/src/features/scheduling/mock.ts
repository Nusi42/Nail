/**
 * The mock core's answers to the M3b invitation, scheduling and free/busy
 * requests (ADR 0065), used by `MockTransport` (`../../api/mock.ts` routes
 * exactly these requests here).
 *
 * A small in-memory scheduler on `data.features.scheduling`, close enough
 * to the core for the UI: invitation cards by message id (the demo ones
 * from `seedSchedulingDemo`), answers / counters / refreshes / forwards
 * that record the iTIP messages "sent" (`state.sent`), applying updates,
 * replies and cancellations, organizer updates and cancellations, and
 * free/busy from the user's own (mock PIM) calendars plus demo people.
 * Calendar changes go through the PIM mock's own requests (`answerPim`),
 * like the core writes through the PIM queue.
 *
 * `state.unsupported` makes every request answer `unsupported` (the core
 * before WP85).
 */
import { MAX_FREE_BUSY_ATTENDEES, MAX_FREE_BUSY_DAYS } from "../../api/generated";
import type {
  AttendeeFreeBusy,
  BusyKind,
  CalendarEvent,
  EventFields,
  FreeBusyIssue,
  FreeBusySource,
  Invitation,
  InvitationAction,
  InvitationConflict,
  InvitationTarget,
  ItipMethod,
  MessageBody,
  PimTime,
  Request,
  Response,
  SchedulingResult,
  SchedulingSettings,
  SchedulingTransport,
  SentItip,
} from "../../api/generated";
import { errorResponse, type MockCore } from "../../api/mockMail";
import { answerPim, listInstances } from "../pim/mock";
import { instantMs, localZone, pimTimeIn, sameTime, todayIn, utcString } from "../pim/time";
import { isMine, mergeBusy, statusOfResponse } from "./model";
import { plausibleEmail } from "../../state/addresses";
import { DAY_MS } from "../../state/clock";

/** The requests this feature answers. */
export const SCHEDULING_REQUESTS = [
  "GetInvitation",
  "RespondToInvitation",
  "ProposeNewTime",
  "RefreshInvitation",
  "ForwardInvitation",
  "ApplyInvitation",
  "SendInvitations",
  "CancelMeeting",
  "RespondToCounter",
  "QueryFreeBusy",
  "GetSchedulingSettings",
  "SetSchedulingSettings",
] as const satisfies readonly Request["type"][];

export type SchedulingRequest = Extract<Request, { type: (typeof SCHEDULING_REQUESTS)[number] }>;

export function isSchedulingRequest(request: Request): request is SchedulingRequest {
  return (SCHEDULING_REQUESTS as readonly string[]).includes(request.type);
}

/** Limits of `QueryFreeBusy` (`nail_api::scheduling`). */

/** A recurring busy block of a demo person, in minutes after midnight (mock zone). */
export interface DemoBusyBlock {
  /** Days after today it happens on; `null` = every day. */
  days: number[] | null;
  start: number;
  end: number;
  kind: BusyKind;
}

/** What the mock knows about another person's free/busy. */
export interface DemoPerson {
  source: FreeBusySource;
  blocks: DemoBusyBlock[];
  /** Asking fails with this error (`network` = the server did not answer). */
  error?: "network" | "auth_failed" | null;
}

/** The mock's scheduling data. */
export interface SchedulingMockState {
  /** Invitation cards by message id. */
  invitations: Record<string, Invitation>;
  /** Free/busy of other people by lower-case address. */
  people: Record<string, DemoPerson>;
  settings: SchedulingSettings;
  /** Every iTIP message "sent", oldest first (for tests and the demo). */
  sent: SentItip[];
  /** The comments sent with them, in the same order (`null` = none). */
  comments: (string | null)[];
  /** The zone and "today" the demo blocks are relative to. */
  tz: string;
  today: string | null;
  /** Answer every request `unsupported` (the core before WP85). */
  unsupported: boolean;
}

export function createSchedulingMockState(): SchedulingMockState {
  return {
    invitations: {},
    people: {},
    settings: {
      add_invitations: true,
      apply_updates: true,
      apply_replies: true,
      apply_cancellations: false,
      calendar_id: null,
      confirm_replies: false,
    },
    sent: [],
    comments: [],
    tz: localZone(),
    today: null,
    unsupported: false,
  };
}

// ---------------------------------------------------------------------------
// Helpers

function state(core: MockCore): SchedulingMockState {
  return core.data.features.scheduling;
}

const clone = <T>(value: T): T => structuredClone(value);

function invalid(message: string, id?: string): Response {
  return {
    type: "Error",
    code: "invalid_request",
    message,
    ...(id ? { localized: { id } } : {}),
  };
}

function notFound(what: string): Response {
  return errorResponse("not_found", `${what} not found`);
}

/** The user's addresses: every identity and account address. */
export function myAddresses(core: MockCore): string[] {
  const d = core.data;
  const out = new Set<string>();
  for (const account of d.accountsConfig.accounts) {
    out.add(account.email.toLowerCase());
    for (const identity of account.identities) out.add(identity.email.toLowerCase());
  }
  for (const account of d.accounts) if (account.email) out.add(account.email.toLowerCase());
  return [...out];
}

/** Calls the PIM mock like the core writes through the PIM queue. */
function pim<T extends Response["type"]>(
  core: MockCore,
  request: Parameters<typeof answerPim>[1],
  type: T,
): Extract<Response, { type: T }> | Extract<Response, { type: "Error" }> {
  const response = answerPim(core, request);
  if (response.type === type || response.type === "Error") {
    return response as Extract<Response, { type: T }> | Extract<Response, { type: "Error" }>;
  }
  return {
    type: "Error",
    code: "internal",
    message: `unexpected ${response.type}`,
  };
}

function getEvent(core: MockCore, id: string | null): CalendarEvent | null {
  if (id === null) return null;
  const response = pim(core, { type: "GetEvent", event_id: id }, "PimEvent");
  return response.type === "PimEvent" ? response.event : null;
}

/** The default calendar for new invitations. */
function targetCalendar(core: MockCore, requested: string | null): string | null {
  const calendars = core.data.features.pim.calendars.filter(
    (c) => !c.read_only && c.components.includes("event"),
  );
  const wanted = requested ?? state(core).settings.calendar_id;
  return calendars.find((c) => c.id === wanted)?.id ?? calendars[0]?.id ?? null;
}

/** The span of an invitation's (first) occurrence. */
function spanOf(fields: EventFields, tz: string): { start: number; end: number } {
  return {
    start: pimTimeIn(fields.start, tz).toDate().getTime(),
    end: pimTimeIn(fields.end, tz).toDate().getTime(),
  };
}

/** Events of the user's calendars overlapping an invitation (attendee side). */
export function conflictsOf(core: MockCore, invitation: Invitation): InvitationConflict[] {
  const fields = invitation.fields;
  if (fields === null || invitation.state === "cancelled" || invitation.state === "invalid") {
    return [];
  }
  if (!["request", "add", "counter"].includes(invitation.method)) return [];
  const tz = state(core).tz;
  const span =
    invitation.method === "counter" && invitation.counter
      ? spanOf({ ...fields, start: invitation.counter.start, end: invitation.counter.end }, tz)
      : spanOf(fields, tz);
  if (!(span.end > span.start)) return [];
  return listInstances(core.data.features.pim, utcString(span.start), utcString(span.end), null, tz)
    .filter(
      (i) =>
        i.event_id !== invitation.event_id &&
        i.transparency === "busy" &&
        i.status !== "cancelled" &&
        instantMs(i.start_utc) < span.end &&
        span.start < instantMs(i.end_utc),
    )
    .map((i) => ({
      event_id: i.event_id,
      calendar_id: i.calendar_id,
      recurrence_id: i.recurrence_id,
      title: i.title,
      start_utc: i.start_utc,
      end_utc: i.end_utc,
      all_day: i.all_day,
      tentative: i.status === "tentative",
    }));
}

/** What the card may offer for an invitation in its current state. */
export function actionsFor(invitation: Invitation): InvitationAction[] {
  if (invitation.state === "invalid") return [];
  const hasEvent = invitation.event_id !== null;
  const open: InvitationAction[] = hasEvent ? ["open-event"] : [];
  const { method, role, state: s } = invitation;
  if (role === "attendee") {
    switch (method) {
      case "request":
      case "add": {
        if (s === "outdated") return [...open, "refresh"];
        const out: InvitationAction[] = ["accept", "tentative", "decline", "propose-new-time"];
        if (s === "new" && !hasEvent) out.push("add-to-calendar");
        if (s === "update") out.push("update-calendar");
        return [...out, "forward", ...open];
      }
      case "cancel":
        return s === "cancelled" && hasEvent ? ["remove-from-calendar", ...open] : open;
      case "decline-counter":
        return [...open, "refresh"];
      case "publish":
        return hasEvent ? open : ["add-to-calendar"];
      default:
        return open;
    }
  }
  if (role === "organizer") {
    switch (method) {
      case "reply":
        return s === "update" ? ["apply-reply", ...open] : open;
      case "counter":
        return s === "outdated" ? open : ["accept-counter", "decline-counter", ...open];
      case "refresh":
        return ["send-current", ...open];
      default:
        return open;
    }
  }
  return hasEvent ? open : ["add-to-calendar"];
}

/** The card with conflicts and actions brought up to date. */
function refreshed(core: MockCore, invitation: Invitation): Invitation {
  const next = clone(invitation);
  if (next.event_id !== null && getEvent(core, next.event_id) === null) {
    next.event_id = null;
    next.calendar_id = null;
  }
  next.conflicts = next.role === "attendee" ? conflictsOf(core, next) : [];
  next.actions = actionsFor(next);
  return next;
}

/** Records an iTIP message as sent (iMIP mail gets an outbox id). */
function send(
  core: MockCore,
  method: ItipMethod,
  recipients: string[],
  transport: SchedulingTransport,
  comment: string | null,
): SentItip {
  const sent: SentItip = {
    method,
    recipients,
    transport,
    outbox_id: transport === "imip" ? core.nextId("outbox-itip-") : null,
  };
  state(core).sent.push(sent);
  state(core).comments.push(comment);
  return sent;
}

function scheduled(result: Partial<SchedulingResult>): Response {
  return {
    type: "Scheduled",
    result: {
      event: null,
      removed_event_id: null,
      sent: [],
      op_ids: [],
      invitation: null,
      ...result,
    },
  };
}

/** Emits `InvitationsChanged` for the cards of `ids`. */
function changed(core: MockCore, ids: string[]): void {
  if (ids.length > 0) core.emit({ type: "InvitationsChanged", message_ids: ids });
}

/** Sets the user's status among an event's attendees. */
function withMyStatus(
  fields: EventFields,
  mine: readonly string[],
  status: EventFields["attendees"][number]["status"],
): EventFields {
  return {
    ...fields,
    attendees: fields.attendees.map((a) => (isMine(a.email, mine) ? { ...a, status } : a)),
  };
}

/** Writes the invitation's event into a calendar (new or updated). */
function writeEvent(
  core: MockCore,
  invitation: Invitation,
  fields: EventFields,
  calendarRequest: string | null,
): Extract<Response, { type: "EventSaved" | "Error" }> {
  const existing = getEvent(core, invitation.event_id);
  if (existing) {
    const response = pim(
      core,
      {
        type: "UpdateEvent",
        event_id: existing.id,
        scope: invitation.recurrence_id !== null && existing.recurrence ? "this" : "all",
        recurrence_id: existing.recurrence ? invitation.recurrence_id : null,
        fields,
        recurrence: existing.recurrence,
      },
      "EventSaved",
    );
    return response;
  }
  const calendar = targetCalendar(core, calendarRequest);
  if (calendar === null)
    return { type: "Error", code: "not_found", message: "no writable calendar" };
  const response = pim(
    core,
    { type: "CreateEvent", calendar_id: calendar, fields, recurrence: invitation.recurrence },
    "EventSaved",
  );
  if (response.type !== "EventSaved") return response;
  invitation.event_id = response.event.id;
  invitation.calendar_id = response.event.calendar_id;
  return response;
}

function isResponse(value: unknown): value is Response {
  return typeof value === "object" && value !== null && "type" in value;
}

/** The invitation a message carries, or an error response. */
function cardOf(core: MockCore, messageId: string): Invitation | Response {
  const invitation = state(core).invitations[messageId];
  if (invitation) return invitation;
  return core.data.bodies.has(messageId)
    ? invalid(`message ${messageId} carries no invitation`)
    : notFound(`message ${messageId}`);
}

function organizerOf(fields: EventFields | null): string[] {
  return fields?.organizer ? [fields.organizer.email] : [];
}

// ---------------------------------------------------------------------------
// Answers

function respond(
  core: MockCore,
  request: Extract<SchedulingRequest, { type: "RespondToInvitation" }>,
): Response {
  const mine = myAddresses(core);
  const status = statusOfResponse(request.response);
  if (request.target.type === "event") {
    const event = getEvent(core, request.target.event_id);
    if (!event) return notFound(`event ${request.target.event_id}`);
    if (!event.fields.attendees.some((a) => isMine(a.email, mine))) {
      return invalid("you are not an attendee of this event");
    }
    const recurrenceId = request.target.recurrence_id;
    const response = pim(
      core,
      {
        type: "UpdateEvent",
        event_id: event.id,
        scope: recurrenceId !== null && event.recurrence ? "this" : "all",
        recurrence_id: event.recurrence ? recurrenceId : null,
        fields: withMyStatus(event.fields, mine, status),
        recurrence: event.recurrence,
      },
      "EventSaved",
    );
    if (response.type !== "EventSaved") return response;
    const sent = request.send
      ? [send(core, "reply", organizerOf(event.fields), "imip", request.comment)]
      : [];
    // Cards about this event show the new answer.
    const ids = Object.values(state(core).invitations)
      .filter((i) => i.event_id === event.id)
      .map((i) => {
        i.my_status = status;
        return i.message_id;
      });
    changed(core, ids);
    return scheduled({ event: response.event, sent, op_ids: response.op_ids });
  }

  const card = cardOf(core, request.target.message_id);
  if (isResponse(card)) return card;
  const current = refreshed(core, card);
  if (!current.actions.includes(request.response)) {
    return invalid(`cannot ${request.response} this invitation now`);
  }
  if (current.fields === null) return invalid("the invitation has no event");
  let event: CalendarEvent | null = null;
  let op_ids: string[] = [];
  const existing = getEvent(core, current.event_id);
  if (request.response !== "decline" || existing) {
    const written = writeEvent(
      core,
      card,
      withMyStatus(current.fields, mine, status),
      request.calendar_id,
    );
    if (written.type !== "EventSaved") return written;
    event = written.event;
    op_ids = written.op_ids;
  }
  card.my_status = status;
  card.state = "current";
  const sent = request.send
    ? [send(core, "reply", organizerOf(current.fields), current.transport, request.comment)]
    : [];
  return answered(core, card, { event, sent, op_ids });
}

/**
 * Stores the card after an answer, reports it changed and answers with the
 * written event, the sent mail and the fresh card.
 */
function answered(
  core: MockCore,
  card: Invitation,
  result: { event: CalendarEvent | null; sent: SentItip[]; op_ids: string[] },
): Response {
  const after = refreshed(core, card);
  state(core).invitations[card.message_id] = after;
  changed(core, [card.message_id]);
  return scheduled({ ...result, invitation: clone(after) });
}

/** The organizer and fields an attendee-side request (counter, refresh, forward) acts on. */
function attendeeTarget(
  core: MockCore,
  target: InvitationTarget,
): { fields: EventFields; card: Invitation | null; transport: SchedulingTransport } | Response {
  if (target.type === "event") {
    const event = getEvent(core, target.event_id);
    if (!event) return notFound(`event ${target.event_id}`);
    return { fields: event.fields, card: null, transport: "imip" };
  }
  const card = cardOf(core, target.message_id);
  if (isResponse(card)) return card;
  if (card.fields === null) return invalid("the invitation has no event");
  return { fields: card.fields, card, transport: card.transport };
}

function proposeNewTime(
  core: MockCore,
  request: Extract<SchedulingRequest, { type: "ProposeNewTime" }>,
): Response {
  const tz = state(core).tz;
  if (
    pimTimeIn(request.end, tz).toDate().getTime() <= pimTimeIn(request.start, tz).toDate().getTime()
  ) {
    return invalid("the end is before the start", "pim-issue-end-before-start");
  }
  const target = attendeeTarget(core, request.target);
  if (isResponse(target)) return target;
  if (target.card && !refreshed(core, target.card).actions.includes("propose-new-time")) {
    return invalid("cannot propose a new time for this invitation");
  }
  const sent = [
    send(core, "counter", organizerOf(target.fields), target.transport, request.comment),
  ];
  return scheduled({ sent, invitation: target.card ? refreshed(core, target.card) : null });
}

function refreshInvitation(
  core: MockCore,
  request: Extract<SchedulingRequest, { type: "RefreshInvitation" }>,
): Response {
  const target = attendeeTarget(core, request.target);
  if (isResponse(target)) return target;
  const sent = [send(core, "refresh", organizerOf(target.fields), target.transport, null)];
  return scheduled({ sent, invitation: target.card ? refreshed(core, target.card) : null });
}

function forwardInvitation(
  core: MockCore,
  request: Extract<SchedulingRequest, { type: "ForwardInvitation" }>,
): Response {
  if (request.to.length === 0) return invalid("no recipients");
  if (request.to.some((a) => !plausibleEmail(a.email))) {
    return invalid("an address is not valid", "pim-issue-invalid-email");
  }
  const target = attendeeTarget(core, request.target);
  if (isResponse(target)) return target;
  const sent = [
    send(
      core,
      "request",
      request.to.map((a) => a.email),
      "imip",
      request.comment,
    ),
  ];
  return scheduled({ sent, invitation: target.card ? refreshed(core, target.card) : null });
}

function applyInvitation(
  core: MockCore,
  request: Extract<SchedulingRequest, { type: "ApplyInvitation" }>,
): Response {
  const card = cardOf(core, request.message_id);
  if (isResponse(card)) return card;
  const current = refreshed(core, card);
  const allowed: InvitationAction[] = [
    "add-to-calendar",
    "update-calendar",
    "remove-from-calendar",
    "apply-reply",
    "send-current",
  ];
  if (!current.actions.some((a) => allowed.includes(a))) {
    return invalid("nothing to apply for this message");
  }
  let event: CalendarEvent | null = null;
  let removed: string | null = null;
  let op_ids: string[] = [];
  let sent: SentItip[] = [];
  switch (card.method) {
    case "request":
    case "add":
    case "publish": {
      if (current.fields === null) return invalid("the invitation has no event");
      const written = writeEvent(core, card, current.fields, request.calendar_id);
      if (written.type !== "EventSaved") return written;
      event = written.event;
      op_ids = written.op_ids;
      break;
    }
    case "cancel": {
      const existing = getEvent(core, card.event_id);
      if (existing) {
        const response = pim(
          core,
          {
            type: "DeleteEvent",
            event_id: existing.id,
            scope: card.recurrence_id !== null && existing.recurrence ? "this" : "all",
            recurrence_id: existing.recurrence ? card.recurrence_id : null,
          },
          "PimDeleted",
        );
        if (response.type !== "PimDeleted") return response;
        op_ids = response.op_ids;
        removed = existing.id;
        if (!(card.recurrence_id !== null && existing.recurrence)) {
          card.event_id = null;
          card.calendar_id = null;
        }
      }
      break;
    }
    case "reply": {
      const existing = getEvent(core, card.event_id);
      const from = card.from_attendee;
      if (!existing || !from) return invalid("the reply names no event of yours");
      const response = pim(
        core,
        {
          type: "UpdateEvent",
          event_id: existing.id,
          scope: "all",
          recurrence_id: null,
          fields: {
            ...existing.fields,
            attendees: existing.fields.attendees.map((a) =>
              a.email.toLowerCase() === from.email.toLowerCase()
                ? { ...a, status: from.status }
                : a,
            ),
          },
          recurrence: existing.recurrence,
        },
        "EventSaved",
      );
      if (response.type !== "EventSaved") return response;
      event = response.event;
      op_ids = response.op_ids;
      break;
    }
    case "refresh": {
      const existing = getEvent(core, card.event_id);
      const from = card.from_attendee;
      if (!existing || !from) return invalid("the request names no event of yours");
      sent = [send(core, "request", [from.email], card.transport, null)];
      event = existing;
      break;
    }
    default:
      return invalid("nothing to apply for this message");
  }
  card.state = card.method === "cancel" ? "cancelled" : "current";
  const after = refreshed(core, card);
  if (card.method === "cancel") after.actions = actionsFor({ ...after, state: "current" });
  state(core).invitations[card.message_id] = after;
  changed(core, [card.message_id]);
  return scheduled({
    event,
    removed_event_id: removed,
    sent,
    op_ids,
    invitation: clone(after),
  });
}

/** The user's event as organizer, or an error. */
function organizedEvent(core: MockCore, eventId: string): CalendarEvent | Response {
  const event = getEvent(core, eventId);
  if (!event) return notFound(`event ${eventId}`);
  if (!isMine(event.fields.organizer?.email, myAddresses(core))) {
    return invalid("only the organizer can do this");
  }
  if (event.fields.attendees.length === 0) return invalid("the event has no attendees");
  return event;
}

function others(event: CalendarEvent, mine: readonly string[]): string[] {
  return event.fields.attendees.map((a) => a.email).filter((e) => !isMine(e, mine));
}

function sendInvitations(
  core: MockCore,
  request: Extract<SchedulingRequest, { type: "SendInvitations" }>,
): Response {
  const event = organizedEvent(core, request.event_id);
  if (isResponse(event)) return event;
  const everyone = others(event, myAddresses(core));
  const unknown = request.attendees.find(
    (a) => !everyone.some((e) => e.toLowerCase() === a.toLowerCase()),
  );
  if (unknown) return invalid(`${unknown} is not an attendee`, "pim-issue-invalid-email");
  const recipients = request.attendees.length > 0 ? request.attendees : everyone;
  if (recipients.length === 0) return invalid("the event has no attendees");
  const sent = [send(core, "request", recipients, "imip", request.comment)];
  return scheduled({ event, sent });
}

function cancelMeeting(
  core: MockCore,
  request: Extract<SchedulingRequest, { type: "CancelMeeting" }>,
): Response {
  const event = organizedEvent(core, request.event_id);
  if (isResponse(event)) return event;
  const mine = myAddresses(core);
  switch (request.scope) {
    case "meeting":
    case "occurrence": {
      const occurrence = request.scope === "occurrence";
      if (occurrence && (request.recurrence_id === null || event.recurrence === null)) {
        return invalid("choose the occurrence to cancel", "pim-issue-invalid-scope");
      }
      const response = pim(
        core,
        {
          type: "DeleteEvent",
          event_id: event.id,
          scope: occurrence ? "this" : "all",
          recurrence_id: occurrence ? request.recurrence_id : null,
        },
        "PimDeleted",
      );
      if (response.type !== "PimDeleted") return response;
      const sent = [send(core, "cancel", others(event, mine), "imip", request.comment)];
      return occurrence
        ? scheduled({ event: getEvent(core, event.id), sent, op_ids: response.op_ids })
        : scheduled({ removed_event_id: event.id, sent, op_ids: response.op_ids });
    }
    case "attendees": {
      const wanted = request.attendees.map((a) => a.toLowerCase());
      if (wanted.length === 0) return invalid("choose the attendees to remove");
      const remaining = event.fields.attendees.filter(
        (a) => !wanted.includes(a.email.toLowerCase()),
      );
      if (remaining.length === event.fields.attendees.length) {
        return invalid("those people are not attendees", "pim-issue-invalid-email");
      }
      const response = pim(
        core,
        {
          type: "UpdateEvent",
          event_id: event.id,
          scope: "all",
          recurrence_id: null,
          fields: { ...event.fields, attendees: remaining },
          recurrence: event.recurrence,
        },
        "EventSaved",
      );
      if (response.type !== "EventSaved") return response;
      const sent = [send(core, "cancel", request.attendees, "imip", request.comment)];
      return scheduled({ event: response.event, sent, op_ids: response.op_ids });
    }
  }
}

function respondToCounter(
  core: MockCore,
  request: Extract<SchedulingRequest, { type: "RespondToCounter" }>,
): Response {
  const card = cardOf(core, request.message_id);
  if (isResponse(card)) return card;
  const current = refreshed(core, card);
  if (card.method !== "counter" || !current.actions.includes("accept-counter")) {
    return invalid("this message proposes no new time");
  }
  const from = card.from_attendee?.email;
  let event: CalendarEvent | null = null;
  let op_ids: string[] = [];
  let sent: SentItip[];
  if (request.accept) {
    const existing = getEvent(core, card.event_id);
    if (!existing || !card.counter) return invalid("the proposal names no event of yours");
    const response = pim(
      core,
      {
        type: "UpdateEvent",
        event_id: existing.id,
        scope: "all",
        recurrence_id: null,
        fields: {
          ...existing.fields,
          start: card.counter.start,
          end: card.counter.end,
          location: card.counter.location ?? existing.fields.location,
        },
        recurrence: existing.recurrence,
      },
      "EventSaved",
    );
    if (response.type !== "EventSaved") return response;
    event = response.event;
    op_ids = response.op_ids;
    sent = [
      send(core, "request", others(response.event, myAddresses(core)), "imip", request.comment),
    ];
  } else {
    sent = [send(core, "decline-counter", from ? [from] : [], card.transport, request.comment)];
  }
  card.state = "outdated";
  return answered(core, card, { event, sent, op_ids });
}

// ---------------------------------------------------------------------------
// Free/busy

function freeBusyIssue(request: Extract<SchedulingRequest, { type: "QueryFreeBusy" }>) {
  const issue = (i: FreeBusyIssue) => invalid(`free/busy: ${i}`, `freebusy-issue-${i}`);
  if (request.attendees.length === 0 || request.attendees.length > MAX_FREE_BUSY_ATTENDEES) {
    return issue("attendee-count");
  }
  if (request.attendees.some((a) => !plausibleEmail(a))) return issue("invalid-attendee");
  const from = Date.parse(request.from);
  const to = Date.parse(request.to);
  if (!Number.isFinite(from) || !Number.isFinite(to) || to <= from) return issue("invalid-range");
  if (to - from > MAX_FREE_BUSY_DAYS * DAY_MS) return issue("range-too-long");
  return null;
}

/** The user's own busy time from the (mock PIM) calendars. */
function ownBusy(
  core: MockCore,
  from: string,
  to: string,
  ignore: string | null,
): AttendeeFreeBusy["periods"] {
  const tz = state(core).tz;
  return listInstances(core.data.features.pim, from, to, null, tz)
    .filter((i) => i.transparency === "busy" && i.status !== "cancelled" && i.event_id !== ignore)
    .map((i) => ({
      start: i.start_utc,
      end: i.end_utc,
      kind: i.status === "tentative" ? "tentative" : "busy",
    }));
}

/** A demo person's blocks inside `[from, to)`. */
function personBusy(
  s: SchedulingMockState,
  person: DemoPerson,
  from: number,
  to: number,
): AttendeeFreeBusy["periods"] {
  const base = s.today ? todayFromString(s.today) : todayIn(s.tz);
  const out: AttendeeFreeBusy["periods"] = [];
  // Every day from a day before `from` to a day after `to`.
  const firstOffset = Math.floor((from - pimDayStart(base, s.tz)) / DAY_MS) - 1;
  const lastOffset = Math.ceil((to - pimDayStart(base, s.tz)) / DAY_MS) + 1;
  for (let offset = firstOffset; offset <= lastOffset && offset - firstOffset < 120; offset++) {
    const dayStart = pimDayStart(base.add({ days: offset }), s.tz);
    for (const block of person.blocks) {
      if (block.days !== null && !block.days.includes(offset)) continue;
      const start = dayStart + block.start * 60_000;
      const end = dayStart + block.end * 60_000;
      if (end <= from || start >= to) continue;
      out.push({ start: utcString(start), end: utcString(end), kind: block.kind });
    }
  }
  return out;
}

function todayFromString(text: string) {
  const [y, m, d] = text.split("-").map(Number);
  return todayIn("UTC", Date.UTC(y ?? 1970, (m ?? 1) - 1, d ?? 1, 12));
}

function pimDayStart(day: ReturnType<typeof todayIn>, tz: string): number {
  const time: PimTime = {
    type: "zoned",
    local: `${String(day.year).padStart(4, "0")}-${String(day.month).padStart(2, "0")}-${String(day.day).padStart(2, "0")}T00:00:00`,
    tz,
  };
  return pimTimeIn(time, tz).toDate().getTime();
}

function queryFreeBusy(
  core: MockCore,
  request: Extract<SchedulingRequest, { type: "QueryFreeBusy" }>,
): Response {
  const problem = freeBusyIssue(request);
  if (problem) return problem;
  const s = state(core);
  const mine = myAddresses(core);
  const from = Date.parse(request.from);
  const to = Date.parse(request.to);
  const attendees: AttendeeFreeBusy[] = request.attendees.map((email) => {
    if (isMine(email, mine)) {
      return {
        email,
        source: { type: "own" },
        periods: mergeBusy(ownBusy(core, request.from, request.to, request.ignore_event_id)),
        error: null,
      };
    }
    const person = s.people[email.toLowerCase()];
    if (!person) return { email, source: { type: "unknown" }, periods: [], error: null };
    if (person.error) {
      return {
        email,
        source: person.source,
        periods: [],
        error: person.error,
        localized: { id: `error-${person.error.replaceAll("_", "-")}` },
      };
    }
    return {
      email,
      source: person.source,
      periods: mergeBusy(personBusy(s, person, from, to)),
      error: null,
    };
  });
  return { type: "FreeBusy", result: { from: request.from, to: request.to, attendees } };
}

// ---------------------------------------------------------------------------
// Dispatch

export function answerScheduling(core: MockCore, request: SchedulingRequest): Response {
  const s = state(core);
  if (s.unsupported) {
    return errorResponse("unsupported", `${request.type} is not wired yet (M3b WP85)`);
  }
  switch (request.type) {
    case "GetInvitation": {
      const invitation = s.invitations[request.message_id];
      if (!invitation) {
        return core.data.bodies.has(request.message_id)
          ? { type: "Invitation", invitation: null }
          : notFound(`message ${request.message_id}`);
      }
      const current = refreshed(core, invitation);
      s.invitations[request.message_id] = current;
      return { type: "Invitation", invitation: clone(current) };
    }
    case "RespondToInvitation":
      return respond(core, request);
    case "ProposeNewTime":
      return proposeNewTime(core, request);
    case "RefreshInvitation":
      return refreshInvitation(core, request);
    case "ForwardInvitation":
      return forwardInvitation(core, request);
    case "ApplyInvitation":
      return applyInvitation(core, request);
    case "SendInvitations":
      return sendInvitations(core, request);
    case "CancelMeeting":
      return cancelMeeting(core, request);
    case "RespondToCounter":
      return respondToCounter(core, request);
    case "QueryFreeBusy":
      return queryFreeBusy(core, request);
    case "GetSchedulingSettings":
      return { type: "SchedulingSettings", settings: clone(s.settings) };
    case "SetSchedulingSettings": {
      const calendar = request.settings.calendar_id;
      if (
        calendar !== null &&
        !core.data.features.pim.calendars.some(
          (c) => c.id === calendar && !c.read_only && c.components.includes("event"),
        )
      ) {
        return notFound(`calendar ${calendar}`);
      }
      s.settings = clone(request.settings);
      core.emit({ type: "SchedulingSettingsChanged" });
      return { type: "SchedulingSettings", settings: clone(s.settings) };
    }
  }
}

/** Whether two `PimTime`s are the same occurrence (exported for tests). */
export function sameOccurrence(a: PimTime | null, b: PimTime | null): boolean {
  if (a === null || b === null) return a === b;
  return sameTime(a, b);
}

/**
 * Adds the card to a message body like the core's `GetMessage`
 * (`MessageBody.invitation`); `../../api/mock.ts` can call it next to the
 * other decorators.
 */
export function decorateInvitation(core: MockCore, body: MessageBody): MessageBody {
  const s = state(core);
  const invitation = s.unsupported ? undefined : s.invitations[body.id];
  if (!invitation) return body;
  const current = refreshed(core, invitation);
  s.invitations[body.id] = current;
  return { ...body, invitation: clone(current) };
}
