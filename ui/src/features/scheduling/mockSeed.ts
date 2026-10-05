/**
 * Demo invitations for the mock core (`?mock=1`, tests): invitation
 * messages in the demo inboxes with their cards, matching events in the
 * PIM mock's calendars, and the free/busy of the demo people. Times are
 * relative to today in the mock's zone, like the PIM demo data, so the
 * new invitation really conflicts with the "Design review".
 *
 * `seedSchedulingDemo(data)` is called once on fresh `MockData` (the
 * integrator calls it from `createMockData`; tests call it themselves).
 */
import type {
  Attendee,
  AttendeeKind,
  AttendeeRole,
  CalendarEvent,
  EventFields,
  Invitation,
  MessageBody,
  MessageSummary,
  ParticipationStatus,
  PimTime,
  Response,
} from "../../api/generated";
import type { MockData } from "../../api/mockData";
import type { MockCore } from "../../api/mockMail";
import { emptyEventFields } from "../pim/defaults";
import { answerPim } from "../pim/mock";
import { dateString, localZone, makePimTime, todayIn } from "../pim/time";
import { actionsFor, conflictsOf, type DemoPerson } from "./mock";

/** Message ids of the demo invitations. */
export const DEMO_INVITATIONS = {
  /** A new invitation (work), conflicting with the "Design review". */
  newRequest: "acc-work/Inbox#invite-sync",
  /** A new weekly series already added unanswered (work). */
  series: "acc-work/Inbox#invite-one-on-one",
  /** An update of the stand-up (personal; room changed). */
  update: "acc-personal/Inbox#invite-standup-update",
  /** A cancellation of the book club (personal). */
  cancel: "acc-personal/Inbox#invite-book-club-cancel",
  /** Katherine accepted the design review (the user organizes it). */
  reply: "acc-work/Inbox#reply-review",
  /** Katherine proposes another time for the design review. */
  counter: "acc-work/Inbox#counter-review",
  /** An invitation whose sender is not its organizer (spoofing warning). */
  spoofed: "acc-work/Inbox#invite-spoofed",
} as const;

const ADA_WORK = { email: "ada@work.example", name: "Ada Lovelace" };
const ADA_HOME = { email: "ada@example.org", name: "Ada Lovelace" };
const GRACE = { email: "grace@navy.example", name: "Grace Hopper" };
const ALAN = { email: "alan@bletchley.example", name: "Alan Turing" };
const KATHERINE = { email: "katherine@nasa.example", name: "Katherine Johnson" };
const LINUS = { email: "linus@kernel.example", name: "Linus" };
const ROOM = { email: "room-4@work.example", name: "Room 4" };

function attendee(
  who: { email: string; name: string | null },
  status: ParticipationStatus,
  role: AttendeeRole = "required",
  kind: AttendeeKind = "individual",
): Attendee {
  return {
    email: who.email,
    name: who.name,
    role,
    status,
    kind,
    rsvp: status === "needs-action",
    delegated_to: [],
    delegated_from: [],
  };
}

/** A core for seeding through the PIM mock's requests (no events, own ids). */
function seedCore(data: MockData): MockCore {
  let n = 0;
  return {
    data,
    emit: () => undefined,
    later: () => undefined,
    nextId: (prefix) => `${prefix}seed-${String(++n)}`,
  };
}

function saved(response: Response): CalendarEvent {
  if (response.type !== "EventSaved") throw new Error(`demo seed: ${response.type}`);
  return response.event;
}

function invitation(
  partial: Partial<Invitation> & Pick<Invitation, "message_id" | "method" | "uid" | "role">,
): Invitation {
  return {
    recurrence_id: null,
    sequence: 0,
    dtstamp: null,
    state: "new",
    organizer: null,
    attendees: [],
    my_address: null,
    my_status: null,
    identity_id: null,
    from_attendee: null,
    fields: null,
    recurrence: null,
    counter: null,
    comment: null,
    event_id: null,
    calendar_id: null,
    conflicts: [],
    actions: [],
    transport: "imip",
    issues: [],
    ...partial,
  };
}

/** Adds one invitation message to a demo inbox. */
function addMessage(
  data: MockData,
  id: string,
  from: { email: string; name: string | null },
  to: { email: string; name: string | null },
  subject: string,
  text: string,
  date: number,
): void {
  const accountId = id.split("/")[0] ?? "";
  const folderId = `${accountId}/Inbox`;
  const summary: MessageSummary = {
    id,
    folder_id: folderId,
    account_id: accountId,
    subject,
    from,
    to: [to],
    date: new Date(date).toISOString(),
    flags: { seen: false, flagged: false, answered: false, draft: false },
    has_attachments: true,
    preview: text.replace(/\s+/g, " ").slice(0, 120),
    thread_id: `t-${id}`,
    tags: [],
    duplicates: [],
  };
  const body: MessageBody = {
    id,
    headers: {
      subject,
      from: [from],
      to: [to],
      cc: [],
      reply_to: [],
      date: summary.date,
      message_id: `${id.replaceAll("/", ".")}@mock.example`,
    },
    text,
    sanitized_html: null,
    attachments: [
      {
        index: 0,
        filename: "invite.ics",
        mime: "text/calendar",
        size: 1_204,
        inline: false,
        shown_in_body: false,
      },
    ],
    remote_content_blocked: false,
  };
  data.messages.push(summary);
  data.bodies.set(id, body);
  const folder = data.folders.find((f) => f.id === folderId);
  if (folder) {
    folder.total++;
    folder.unread++;
  }
}

export interface SeedOptions {
  now?: number;
  tz?: string;
}

/** Fills the scheduling demo into fresh mock data (see the module comment). */
export function seedSchedulingDemo(data: MockData, options: SeedOptions = {}): void {
  const now = options.now ?? Date.now();
  const tz = options.tz ?? localZone();
  const today = todayIn(tz, now);
  const s = data.features.scheduling;
  s.tz = tz;
  s.today = dateString(today);
  const core = seedCore(data);
  const at = (days: number, hour: number, minute = 0): PimTime =>
    makePimTime(today.add({ days }), { hour, minute }, tz);
  const fields = (
    partial: Partial<EventFields> & Pick<EventFields, "title" | "start" | "end">,
  ): EventFields => ({ ...emptyEventFields(partial.start, partial.end), ...partial });
  const hoursAgo = (h: number) => now - h * 3_600_000;

  // 1. A new invitation that overlaps the design review tomorrow.
  const syncFields = fields({
    title: "Architecture sync",
    start: at(1, 14, 30),
    end: at(1, 15, 30),
    location: "Room 4",
    description: "Module boundaries for the scheduling work.",
    organizer: GRACE,
    attendees: [
      attendee(GRACE, "accepted", "chair"),
      attendee(ADA_WORK, "needs-action"),
      attendee(ALAN, "tentative", "optional"),
      attendee(ROOM, "accepted", "non-participant", "room"),
    ],
  });
  addMessage(
    data,
    DEMO_INVITATIONS.newRequest,
    GRACE,
    ADA_WORK,
    "Invitation: Architecture sync",
    "Grace Hopper invites you to Architecture sync.",
    hoursAgo(1),
  );
  s.invitations[DEMO_INVITATIONS.newRequest] = invitation({
    message_id: DEMO_INVITATIONS.newRequest,
    method: "request",
    uid: "arch-sync@navy.example",
    role: "attendee",
    sequence: 0,
    dtstamp: new Date(hoursAgo(1)).toISOString(),
    organizer: GRACE,
    attendees: syncFields.attendees,
    my_address: ADA_WORK.email,
    my_status: "needs-action",
    identity_id: ADA_WORK.email,
    fields: syncFields,
    comment: "Let's settle the module boundaries before the sprint.",
  });

  // 2. A weekly series, already in the calendar unanswered (add_invitations).
  const seriesFields = fields({
    title: "Weekly 1:1",
    start: at(2, 11),
    end: at(2, 11, 30),
    organizer: GRACE,
    attendees: [attendee(GRACE, "accepted", "chair"), attendee(ADA_WORK, "needs-action")],
  });
  const seriesRecurrence: CalendarEvent["recurrence"] = {
    rules: [
      {
        freq: "weekly",
        interval: 1,
        end: { type: "count", count: 10 },
        by_second: [],
        by_minute: [],
        by_hour: [],
        by_day: [],
        by_month_day: [],
        by_year_day: [],
        by_week_no: [],
        by_month: [],
        by_set_pos: [],
        week_start: null,
      },
    ],
    dates: [],
    exceptions: [],
  };
  const series = saved(
    answerPim(core, {
      type: "CreateEvent",
      calendar_id: "cal-work",
      fields: { ...seriesFields, status: "tentative" },
      recurrence: seriesRecurrence,
    }),
  );
  addMessage(
    data,
    DEMO_INVITATIONS.series,
    GRACE,
    ADA_WORK,
    "Invitation: Weekly 1:1",
    "Grace Hopper invites you to a weekly 1:1.",
    hoursAgo(2),
  );
  s.invitations[DEMO_INVITATIONS.series] = invitation({
    message_id: DEMO_INVITATIONS.series,
    method: "request",
    uid: series.uid,
    role: "attendee",
    organizer: GRACE,
    attendees: seriesFields.attendees,
    my_address: ADA_WORK.email,
    my_status: "needs-action",
    identity_id: ADA_WORK.email,
    fields: seriesFields,
    recurrence: seriesRecurrence,
    event_id: series.id,
    calendar_id: series.calendar_id,
  });

  // 3. An update of the stand-up: the room changed.
  const standup = data.features.pim.events.find((e) => e.id === "ev-standup");
  if (standup) {
    const updated = { ...structuredClone(standup.fields), location: "Room 5" };
    addMessage(
      data,
      DEMO_INVITATIONS.update,
      GRACE,
      ADA_HOME,
      "Updated invitation: Stand-up",
      "The stand-up moves to Room 5.",
      hoursAgo(3),
    );
    s.invitations[DEMO_INVITATIONS.update] = invitation({
      message_id: DEMO_INVITATIONS.update,
      method: "request",
      uid: standup.uid,
      role: "attendee",
      state: "update",
      sequence: standup.sequence + 1,
      organizer: standup.fields.organizer,
      attendees: updated.attendees,
      my_address: ADA_HOME.email,
      my_status: "accepted",
      identity_id: ADA_HOME.email,
      fields: updated,
      recurrence: structuredClone(standup.recurrence),
      event_id: standup.id,
      calendar_id: standup.calendar_id,
    });
  }

  // 4. A cancellation of the book club.
  const bookClub = saved(
    answerPim(core, {
      type: "CreateEvent",
      calendar_id: "cal-personal",
      fields: fields({
        title: "Book club",
        start: at(3, 19),
        end: at(3, 21),
        location: "Library",
        organizer: LINUS,
        attendees: [attendee(LINUS, "accepted", "chair"), attendee(ADA_HOME, "accepted")],
      }),
      recurrence: null,
    }),
  );
  addMessage(
    data,
    DEMO_INVITATIONS.cancel,
    LINUS,
    ADA_HOME,
    "Cancelled: Book club",
    "The book club is cancelled this time.",
    hoursAgo(4),
  );
  s.invitations[DEMO_INVITATIONS.cancel] = invitation({
    message_id: DEMO_INVITATIONS.cancel,
    method: "cancel",
    uid: bookClub.uid,
    role: "attendee",
    state: "cancelled",
    sequence: 1,
    organizer: LINUS,
    attendees: bookClub.fields.attendees,
    my_address: ADA_HOME.email,
    my_status: "accepted",
    identity_id: ADA_HOME.email,
    fields: { ...bookClub.fields, status: "cancelled" },
    comment: "Half of us are travelling.",
    event_id: bookClub.id,
    calendar_id: bookClub.calendar_id,
  });

  // 5./6. The user organizes the design review: a reply and a counter-proposal.
  const review = data.features.pim.events.find((e) => e.id === "ev-review");
  if (review) {
    review.fields.organizer = ADA_WORK;
    const replied = attendee(KATHERINE, "accepted");
    addMessage(
      data,
      DEMO_INVITATIONS.reply,
      KATHERINE,
      ADA_WORK,
      "Accepted: Design review",
      "Katherine Johnson accepted.",
      hoursAgo(5),
    );
    s.invitations[DEMO_INVITATIONS.reply] = invitation({
      message_id: DEMO_INVITATIONS.reply,
      method: "reply",
      uid: review.uid,
      role: "organizer",
      state: "update",
      organizer: ADA_WORK,
      attendees: [replied],
      my_address: ADA_WORK.email,
      identity_id: ADA_WORK.email,
      from_attendee: replied,
      fields: { ...review.fields, attendees: [replied] },
      comment: "Looking forward to it.",
      event_id: review.id,
      calendar_id: review.calendar_id,
    });
    const proposer = attendee(KATHERINE, "tentative");
    addMessage(
      data,
      DEMO_INVITATIONS.counter,
      KATHERINE,
      ADA_WORK,
      "New time proposed: Design review",
      "Katherine Johnson proposes another time.",
      hoursAgo(6),
    );
    s.invitations[DEMO_INVITATIONS.counter] = invitation({
      message_id: DEMO_INVITATIONS.counter,
      method: "counter",
      uid: review.uid,
      role: "organizer",
      state: "current",
      organizer: ADA_WORK,
      attendees: [proposer],
      my_address: ADA_WORK.email,
      identity_id: ADA_WORK.email,
      from_attendee: proposer,
      fields: structuredClone(review.fields),
      counter: {
        start: at(2, 10),
        end: at(2, 11, 30),
        location: null,
        comment: "I am at a conference that afternoon.",
      },
      event_id: review.id,
      calendar_id: review.calendar_id,
    });
  }

  // 7. A spoofed invitation: the sender is not the organizer.
  const spoofFields = fields({
    title: "Urgent budget approval",
    start: at(1, 9),
    end: at(1, 9, 30),
    organizer: { email: "ceo@work.example", name: "The CEO" },
    attendees: [attendee(ADA_WORK, "needs-action")],
  });
  addMessage(
    data,
    DEMO_INVITATIONS.spoofed,
    { email: "billing@evil.example", name: "The CEO" },
    ADA_WORK,
    "Invitation: Urgent budget approval",
    "Please approve the transfer.",
    hoursAgo(7),
  );
  s.invitations[DEMO_INVITATIONS.spoofed] = invitation({
    message_id: DEMO_INVITATIONS.spoofed,
    method: "request",
    uid: "budget@evil.example",
    role: "attendee",
    organizer: spoofFields.organizer,
    attendees: spoofFields.attendees,
    my_address: ADA_WORK.email,
    my_status: "needs-action",
    identity_id: ADA_WORK.email,
    fields: spoofFields,
    issues: ["sender-not-organizer"],
  });

  for (const card of Object.values(s.invitations)) {
    card.conflicts = card.role === "attendee" ? conflictsOf(core, card) : [];
    card.actions = actionsFor(card);
  }

  // Other people's free/busy (minutes after midnight, days after today).
  const people: Record<string, DemoPerson> = {
    [GRACE.email]: {
      source: { type: "caldav", source_id: "work-calendars" },
      blocks: [
        { days: null, start: 9 * 60, end: 10 * 60, kind: "busy" },
        { days: [1], start: 11 * 60, end: 12 * 60 + 30, kind: "busy" },
        { days: [2], start: 13 * 60, end: 17 * 60, kind: "unavailable" },
      ],
    },
    [ALAN.email]: {
      source: { type: "caldav", source_id: "work-calendars" },
      blocks: [
        { days: null, start: 12 * 60, end: 13 * 60, kind: "busy" },
        { days: null, start: 15 * 60, end: 16 * 60, kind: "tentative" },
      ],
    },
    [KATHERINE.email]: {
      source: { type: "caldav", source_id: "work-calendars" },
      blocks: [
        { days: null, start: 8 * 60, end: 9 * 60 + 30, kind: "busy" },
        { days: null, start: 14 * 60, end: 15 * 60, kind: "busy" },
      ],
    },
    [ROOM.email]: {
      source: { type: "caldav", source_id: "work-calendars" },
      blocks: [{ days: null, start: 10 * 60, end: 11 * 60, kind: "busy" }],
    },
    [LINUS.email]: {
      source: { type: "caldav", source_id: "work-calendars" },
      blocks: [],
      error: "network",
    },
  };
  s.people = people;
}
