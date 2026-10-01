/**
 * Demo PIM data of the mock core (`?mock=1` and the UI tests): the local
 * source with a personal calendar, a task list and address books, a
 * read-only holiday subscription, and a work CalDAV / CardDAV source; a
 * recurring stand-up, overlapping meetings, an all-day trip, a yearly
 * birthday, a meeting in New York time, tasks with subtasks, contacts with
 * duplicates across books, and groups. Times are relative to "today" in the
 * viewer's zone so the views always have something to show.
 */
import { type CalendarDate, toCalendarDate, fromAbsolute } from "@internationalized/date";
import type {
  AddressBook,
  Alarm,
  Calendar,
  CalendarEvent,
  Contact,
  ContactFields,
  ContactGroup,
  EventFields,
  PimSource,
  PimTime,
  Reminder,
  Task,
  TaskFields,
} from "../../api/generated";
import { emptyContactFields, emptyEventFields, emptyTaskFields } from "./defaults";
import { dateString, localString, localZone, makePimTime, pimTimeIn, utcString } from "./time";

/** A tiny SVG portrait (the mock's contact photo), base64. */
export const DEMO_PHOTO_SVG =
  "PHN2ZyB4bWxucz0iaHR0cDovL3d3dy53My5vcmcvMjAwMC9zdmciIHZpZXdCb3g9IjAgMCA2NCA2NCI+PHJlY3Qgd2lkdGg9IjY0IiBoZWlnaHQ9IjY0IiBmaWxsPSIjOGI1Y2Y2Ii8+PGNpcmNsZSBjeD0iMzIiIGN5PSIyNiIgcj0iMTIiIGZpbGw9IiNmZmYiLz48cmVjdCB4PSIxNCIgeT0iNDIiIHdpZHRoPSIzNiIgaGVpZ2h0PSIxOCIgcng9IjkiIGZpbGw9IiNmZmYiLz48L3N2Zz4=";

const minutesBefore = (minutes: number): Alarm => ({
  uid: null,
  action: "display",
  trigger: { type: "relative", offset_secs: -minutes * 60, related: "start" },
  description: null,
  repeat: null,
  acknowledged: null,
});

export interface PimSeed {
  sources: PimSource[];
  calendars: Calendar[];
  addressBooks: AddressBook[];
  events: CalendarEvent[];
  contacts: Contact[];
  groups: ContactGroup[];
  tasks: Task[];
  reminders: Reminder[];
}

function calendar(
  id: string,
  source: string,
  name: string,
  color: string,
  components: Calendar["components"],
  extra: Partial<Calendar> = {},
  order = 0,
): Calendar {
  return {
    id,
    source_id: source,
    account_id: null,
    settings: {
      name,
      description: null,
      color,
      visible: true,
      reminders: true,
      default_alarms: components.includes("event") ? [minutesBefore(15)] : [],
      order,
    },
    components,
    read_only: false,
    timezone: null,
    ...extra,
  };
}

function event(
  id: string,
  calendarId: string,
  fields: Partial<EventFields> & Pick<EventFields, "title" | "start" | "end">,
  extra: Partial<CalendarEvent> = {},
): CalendarEvent {
  return {
    id,
    calendar_id: calendarId,
    uid: `${id}@nail.example`,
    fields: { ...emptyEventFields(fields.start, fields.end), ...fields },
    recurrence: null,
    overrides: [],
    sequence: 0,
    created_at: null,
    updated_at: null,
    read_only: false,
    sync: "synced",
    ...extra,
  };
}

function contact(
  id: string,
  book: string,
  fields: Partial<ContactFields> & Pick<ContactFields, "display_name">,
  extra: Partial<Contact> = {},
): Contact {
  return {
    id,
    address_book_id: book,
    uid: `urn:uuid:${id}`,
    fields: { ...emptyContactFields(fields.display_name), ...fields },
    version: "4.0",
    group_ids: [],
    linked_ids: [],
    updated_at: null,
    read_only: false,
    sync: "synced",
    ...extra,
  };
}

function task(
  id: string,
  list: string,
  fields: Partial<TaskFields> & Pick<TaskFields, "title">,
  extra: Partial<Task> = {},
): Task {
  return {
    id,
    calendar_id: list,
    uid: `${id}@nail.example`,
    fields: { ...emptyTaskFields(fields.title), ...fields },
    recurrence: null,
    subtask_ids: [],
    created_at: null,
    updated_at: null,
    read_only: false,
    sync: "synced",
    ...extra,
  };
}

const email = (value: string, kinds: string[] = [], preferred = false) => ({
  value,
  kinds,
  label: null,
  preferred,
});

const NO_DAV = { refresh_minutes: null, url: null, auth: null };

/** The demo data for "now" in the viewer's zone. */
export function demoPim(now: number = Date.now(), tz: string = localZone()): PimSeed {
  const today = toCalendarDate(fromAbsolute(now, tz));
  const day = (offset: number) => today.add({ days: offset });
  const at = (offset: number, hour: number, minute = 0, zone: string | null = tz) =>
    makePimTime(day(offset), { hour, minute }, zone);
  const date = (offset: number): PimTime => ({ type: "date", date: dateString(day(offset)) });

  const sources: PimSource[] = [
    {
      id: "local",
      kind: "local",
      name: "",
      account_id: null,
      enabled: true,
      config: null,
      state: "idle",
      last_sync_at: null,
      error: null,
      message: null,
    },
    {
      id: "holidays",
      kind: "ics",
      name: "Public holidays",
      account_id: null,
      enabled: true,
      config: {
        id: "holidays",
        name: "Public holidays",
        enabled: true,
        settings: {
          kind: "ics",
          url: "webcal://holidays.nail.example/de.ics",
          color: "#16a34a",
          alarms: false,
          refresh_minutes: 1440,
          auth: null,
        },
      },
      state: "idle",
      last_sync_at: utcString(now - 3_600_000),
      error: null,
      message: null,
    },
    {
      id: "work-calendars",
      kind: "caldav",
      name: "Work calendars",
      account_id: "acc-work",
      enabled: true,
      config: {
        id: "work-calendars",
        name: "Work calendars",
        enabled: true,
        settings: { kind: "caldav", account: "acc-work", ...NO_DAV },
      },
      state: "idle",
      last_sync_at: utcString(now - 600_000),
      error: null,
      message: null,
    },
    {
      id: "work-contacts",
      kind: "carddav",
      name: "Work contacts",
      account_id: "acc-work",
      enabled: true,
      config: {
        id: "work-contacts",
        name: "Work contacts",
        enabled: true,
        settings: { kind: "carddav", account: "acc-work", ...NO_DAV },
      },
      state: "idle",
      last_sync_at: utcString(now - 600_000),
      error: null,
      message: null,
    },
  ];

  const calendars: Calendar[] = [
    calendar("cal-personal", "local", "Personal", "#3b82f6", ["event"], {}, 0),
    calendar(
      "cal-work",
      "work-calendars",
      "Work",
      "#ef4444",
      ["event", "task"],
      { account_id: "acc-work" },
      1,
    ),
    calendar(
      "cal-holidays",
      "holidays",
      "Public holidays",
      "#16a34a",
      ["event"],
      { read_only: true },
      2,
    ),
    calendar("list-tasks", "local", "Tasks", "#a855f7", ["task"], {}, 3),
  ];

  // The stand-up: weekdays 09:30–09:45 for the last four weeks and on.
  const standupStart = day(-28);
  const events: CalendarEvent[] = [
    event(
      "ev-standup",
      "cal-work",
      {
        title: "Stand-up",
        start: makePimTime(standupStart, { hour: 9, minute: 30 }, tz),
        end: makePimTime(standupStart, { hour: 9, minute: 45 }, tz),
        location: "Room 4",
        organizer: { email: "grace@navy.example", name: "Grace Hopper" },
        attendees: [
          {
            email: "ada@example.org",
            name: "Ada Lovelace",
            role: "required",
            status: "accepted",
            kind: "individual",
            rsvp: false,
            delegated_to: [],
            delegated_from: [],
          },
          {
            email: "alan@bletchley.example",
            name: "Alan Turing",
            role: "optional",
            status: "tentative",
            kind: "individual",
            rsvp: true,
            delegated_to: [],
            delegated_from: [],
          },
        ],
        alarms: [minutesBefore(5)],
        conferences: [
          {
            uri: "https://meet.nail.example/standup",
            label: "Video call",
            features: ["video", "audio"],
          },
        ],
      },
      {
        recurrence: {
          rules: [
            {
              freq: "weekly",
              interval: 1,
              end: { type: "never" },
              by_second: [],
              by_minute: [],
              by_hour: [],
              by_day: ["mon", "tue", "wed", "thu", "fri"].map((d) => ({
                day: d as "mon",
                ordinal: null,
              })),
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
        },
      },
    ),
    event("ev-lunch", "cal-personal", {
      title: "Lunch with Grace",
      start: at(0, 12, 30, null),
      end: at(0, 13, 30, null),
      location: "Canteen",
    }),
    event("ev-planning", "cal-work", {
      title: "Sprint planning",
      start: at(0, 12, 0),
      end: at(0, 13, 0),
      alarms: [minutesBefore(10)],
    }),
    event("ev-review", "cal-work", {
      title: "Design review",
      start: at(1, 14, 0),
      end: at(1, 15, 30),
      description: "Calendar views and the event editor.",
      conferences: [{ uri: "https://meet.nail.example/review", label: null, features: ["video"] }],
      attendees: [
        {
          email: "katherine@nasa.example",
          name: "Katherine Johnson",
          role: "required",
          status: "needs-action",
          kind: "individual",
          rsvp: true,
          delegated_to: [],
          delegated_from: [],
        },
      ],
    }),
    event("ev-call-ny", "cal-work", {
      title: "Call with New York",
      start: at(2, 10, 0, "America/New_York"),
      end: at(2, 11, 0, "America/New_York"),
    }),
    event("ev-trip", "cal-personal", {
      title: "Trip to Lisbon",
      start: date(3),
      end: date(6),
      transparency: "free",
    }),
    event(
      "ev-birthday",
      "cal-personal",
      { title: "Grace's birthday", start: date(5), end: date(6), transparency: "free" },
      {
        recurrence: {
          rules: [
            {
              freq: "yearly",
              interval: 1,
              end: { type: "never" },
              by_second: [],
              by_minute: [],
              by_hour: [],
              by_day: [],
              by_month_day: [day(5).day],
              by_year_day: [],
              by_week_no: [],
              by_month: [day(5).month],
              by_set_pos: [],
              week_start: null,
            },
          ],
          dates: [],
          exceptions: [],
        },
      },
    ),
    event(
      "ev-holiday",
      "cal-holidays",
      { title: "Public holiday", start: date(10), end: date(11), transparency: "free" },
      { read_only: true },
    ),
  ];

  const addressBooks: AddressBook[] = [
    {
      id: "ab-personal",
      source_id: "local",
      account_id: null,
      settings: {
        name: "Contacts",
        description: null,
        visible: true,
        autocomplete: true,
        order: 0,
      },
      read_only: false,
      role: null,
    },
    {
      id: "ab-work",
      source_id: "work-contacts",
      account_id: "acc-work",
      settings: { name: "Work", description: null, visible: true, autocomplete: true, order: 1 },
      read_only: false,
      role: null,
    },
    {
      id: "ab-collected",
      source_id: "local",
      account_id: null,
      settings: {
        name: "Collected",
        description: null,
        visible: true,
        autocomplete: true,
        order: 2,
      },
      read_only: false,
      role: "collected",
    },
  ];

  const contacts: Contact[] = [
    contact("ct-ada", "ab-personal", {
      display_name: "Ada Lovelace",
      name: { prefix: null, given: "Ada", additional: null, family: "Lovelace", suffix: null },
      emails: [email("ada@example.org", ["home"], true)],
      phones: [{ value: "+44 20 7946 0000", kinds: ["cell"], label: null, preferred: true }],
      addresses: [
        {
          kinds: ["home"],
          label: null,
          preferred: true,
          po_box: null,
          extended: null,
          street: "12 St James's Square",
          locality: "London",
          region: null,
          postal_code: "SW1Y 4JH",
          country: "United Kingdom",
        },
      ],
      birthday: { year: 1815, month: 12, day: 10 },
      notes: "Analytical engine notes.",
      photo: { mime_type: "image/svg+xml", sha256: null, url: null, data: DEMO_PHOTO_SVG },
      custom: [{ name: "X-FAVOURITE-ENGINE", value: "Analytical" }],
    }),
    contact(
      "ct-grace",
      "ab-personal",
      {
        display_name: "Grace Hopper",
        name: { prefix: null, given: "Grace", additional: null, family: "Hopper", suffix: null },
        emails: [email("grace@navy.example", ["work"], true)],
        organization: { name: "US Navy", units: [] },
        title: "Rear Admiral",
      },
      { group_ids: ["grp-family"] },
    ),
    contact(
      "ct-grace-work",
      "ab-work",
      {
        display_name: "Grace B. Hopper",
        name: { prefix: null, given: "Grace", additional: "B.", family: "Hopper", suffix: null },
        emails: [email("grace@navy.example", ["work"])],
        phones: [{ value: "+1 555 0100", kinds: ["work"], label: null, preferred: false }],
      },
      { group_ids: ["grp-team"], version: "3.0" },
    ),
    contact(
      "ct-alan",
      "ab-work",
      {
        display_name: "Alan Turing",
        name: { prefix: null, given: "Alan", additional: null, family: "Turing", suffix: null },
        emails: [email("alan@bletchley.example", ["work"], true)],
        organization: { name: "Bletchley Park", units: ["Hut 8"] },
      },
      { group_ids: ["grp-team"], version: "3.0" },
    ),
    contact("ct-katherine", "ab-personal", {
      display_name: "Katherine Johnson",
      name: { prefix: null, given: "Katherine", additional: null, family: "Johnson", suffix: null },
      emails: [email("katherine@nasa.example", ["work"], true)],
      organization: { name: "NASA", units: [] },
    }),
    contact("ct-linus", "ab-collected", {
      display_name: "Linus",
      emails: [email("linus@kernel.example")],
    }),
  ];

  const groups: ContactGroup[] = [
    {
      id: "grp-team",
      address_book_id: "ab-work",
      uid: "grp-team@nail.example",
      name: "Team",
      member_ids: ["ct-grace-work", "ct-alan"],
      member_emails: [],
      read_only: false,
      sync: "synced",
    },
    {
      id: "grp-family",
      address_book_id: "ab-personal",
      uid: "grp-family@nail.example",
      name: "Family",
      member_ids: ["ct-grace"],
      member_emails: ["cousin@family.example"],
      read_only: false,
      sync: "synced",
    },
  ];

  const tasks: Task[] = [
    task(
      "tk-report",
      "list-tasks",
      {
        title: "Write the quarterly report",
        due: at(0, 17, 0),
        priority: 1,
        status: "in-process",
        percent_complete: 40,
        description: "Numbers from finance first.",
      },
      { subtask_ids: ["tk-report-numbers", "tk-report-draft"] },
    ),
    task("tk-report-numbers", "list-tasks", {
      title: "Collect the numbers",
      parent_id: "tk-report",
      status: "completed",
      percent_complete: 100,
      completed_at: utcString(now - 86_400_000),
    }),
    task("tk-report-draft", "list-tasks", {
      title: "Draft the text",
      parent_id: "tk-report",
      due: date(0),
    }),
    task("tk-groceries", "list-tasks", { title: "Buy groceries", due: date(1), priority: 5 }),
    task(
      "tk-weekly",
      "list-tasks",
      { title: "Weekly review", due: at(4, 16, 0) },
      {
        recurrence: {
          rules: [
            {
              freq: "weekly",
              interval: 1,
              end: { type: "never" },
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
        },
      },
    ),
    task("tk-done", "list-tasks", {
      title: "Book the flights",
      status: "completed",
      percent_complete: 100,
      completed_at: utcString(now - 2 * 86_400_000),
    }),
    task("tk-work", "cal-work", { title: "Review the pull request", due: date(2), priority: 3 }),
  ];

  const reviewStart = makePimTime(day(1), { hour: 14, minute: 0 }, tz);
  const reminders: Reminder[] = [
    {
      id: "rem-review",
      target: { type: "event", event_id: "ev-review", recurrence_id: null },
      calendar_id: "cal-work",
      title: "Design review",
      location: null,
      description: null,
      starts_at: utcString(pimTimeIn(reviewStart, tz)),
      all_day: false,
      fire_at: utcString(now + 86_400_000),
      state: "pending",
      snoozed_until: null,
    },
  ];

  return { sources, calendars, addressBooks, events, contacts, groups, tasks, reminders };
}

/** Today as a `CalendarDate` in `tz` (for tests). */
export function seedToday(now: number, tz: string): CalendarDate {
  return toCalendarDate(fromAbsolute(now, tz));
}

export { localString };

export { emptyContactFields, emptyEventFields, emptyTaskFields };
