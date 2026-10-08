import { QueryClient } from "@tanstack/react-query";
import { describe, expect, it, vi } from "vitest";
import type {
  CalendarEvent,
  EventFields,
  Event as NailEvent,
  Recurrence,
  Request,
  Response,
} from "../../api/generated";
import { MockTransport } from "../../api/mock";
import { createUiStore } from "../../state/uiStore";
import type { FeatureEventContext } from "../types";
import { PIM_EVENTS, handlePimEvent, pimKeys, pimStoreFor } from ".";
import { PIM_REQUESTS, createPimMockState, fireReminder, isPimRequest, parseImport } from "./mock";
import { expandEvent, occurrenceStarts } from "./mockExpand";
import { emptyEventFields } from "./mockSeed";
import { plainRecurrence } from "./recurrence";
import { inTimeZone } from "../../test/env";
import { must } from "../../test/must";
import { BERLIN } from "../../test/mockIds";

inTimeZone(BERLIN);

type Of<T extends Response["type"]> = Extract<Response, { type: T }>;

async function ok<T extends Response["type"]>(
  t: MockTransport,
  request: Request,
  type: T,
): Promise<Of<T>> {
  const response = await t.send(request);
  expect(response.type, JSON.stringify(response)).toBe(type);
  return response as Of<T>;
}

function weekly(
  days: ("mon" | "tue" | "wed" | "thu" | "fri")[],
  end: Recurrence["rules"][number]["end"] = { type: "never" },
): Recurrence {
  return plainRecurrence("weekly", { end, by_day: days.map((day) => ({ day, ordinal: null })) });
}

function fields(start: string, end: string, title = "Meeting"): EventFields {
  return {
    ...emptyEventFields(
      { type: "zoned", local: start, tz: BERLIN },
      { type: "zoned", local: end, tz: BERLIN },
    ),
    title,
  };
}

function series(
  id: string,
  recurrence: Recurrence | null,
  f = fields("2026-10-05T09:00:00", "2026-10-05T10:00:00"),
): CalendarEvent {
  return {
    id,
    calendar_id: "cal-personal",
    uid: id,
    fields: f,
    recurrence,
    overrides: [],
    sequence: 0,
    created_at: null,
    updated_at: null,
    read_only: false,
    sync: "synced",
  };
}

const ms = (iso: string) => Date.parse(iso);

describe("recurrence expansion (mock)", () => {
  it("expands weekly rules on chosen days, keeping the wall time across DST", () => {
    const ev = series(
      "s",
      weekly(["mon", "wed"]),
      fields("2026-10-19T09:00:00", "2026-10-19T10:00:00"),
    );
    const instances = expandEvent(
      ev,
      ms("2026-10-19T00:00:00Z"),
      ms("2026-11-01T00:00:00Z"),
      BERLIN,
      false,
    );
    expect(instances.map((i) => i.start_utc)).toEqual([
      "2026-10-19T07:00:00Z",
      "2026-10-21T07:00:00Z",
      // Summer time ended on the 25th: 09:00 Berlin is 08:00 UTC.
      "2026-10-26T08:00:00Z",
      "2026-10-28T08:00:00Z",
    ]);
    expect(instances[0]).toMatchObject({
      recurring: true,
      recurrence_id: ev.fields.start,
      overridden: false,
    });
  });

  it("honours COUNT, UNTIL, EXDATE, RDATE and overrides", () => {
    const counted = series("c", weekly(["mon"], { type: "count", count: 2 }));
    expect(occurrenceStarts(counted, ms("2027-01-01T00:00:00Z"), BERLIN)).toHaveLength(2);
    const until = series(
      "u",
      weekly(["mon"], {
        type: "until",
        until: { type: "zoned", local: "2026-10-12T07:00:00", tz: "UTC" },
      }),
    );
    expect(occurrenceStarts(until, ms("2027-01-01T00:00:00Z"), BERLIN)).toHaveLength(2);
    const rec = weekly(["mon"]);
    rec.exceptions = [{ type: "zoned", local: "2026-10-12T09:00:00", tz: BERLIN }];
    rec.dates = [{ type: "zoned", local: "2026-10-14T09:00:00", tz: BERLIN }];
    const ev = series("x", rec);
    ev.overrides = [
      {
        recurrence_id: { type: "zoned", local: "2026-10-19T09:00:00", tz: BERLIN },
        fields: fields("2026-10-19T15:00:00", "2026-10-19T16:00:00", "Moved"),
        this_and_future: false,
      },
    ];
    const list = expandEvent(
      ev,
      ms("2026-10-05T00:00:00Z"),
      ms("2026-10-20T00:00:00Z"),
      BERLIN,
      false,
    );
    expect(list.map((i) => `${i.title}@${i.start_utc}`)).toEqual([
      "Meeting@2026-10-05T07:00:00Z",
      "Meeting@2026-10-14T07:00:00Z",
      "Moved@2026-10-19T13:00:00Z",
    ]);
    expect(list[2]?.overridden).toBe(true);
  });

  it("expands monthly n-th weekdays and yearly dates", () => {
    const monthly: Recurrence = {
      rules: [
        { ...must(weekly([]).rules[0]), freq: "monthly", by_day: [{ day: "fri", ordinal: -1 }] },
      ],
      dates: [],
      exceptions: [],
    };
    const ev = series("m", monthly, fields("2026-10-30T09:00:00", "2026-10-30T10:00:00"));
    const starts = occurrenceStarts(ev, ms("2027-01-01T00:00:00Z"), BERLIN).map((s) =>
      s.type === "date" ? s.date : s.local,
    );
    expect(starts).toEqual(["2026-10-30T09:00:00", "2026-11-27T09:00:00", "2026-12-25T09:00:00"]);
    const yearly: Recurrence = {
      rules: [{ ...must(weekly([]).rules[0]), freq: "yearly", by_day: [] }],
      dates: [],
      exceptions: [],
    };
    const birthday = series("y", yearly, {
      ...emptyEventFields(
        { type: "date", date: "2024-02-29" },
        { type: "date", date: "2024-03-01" },
      ),
      title: "Leap",
    });
    const leap = occurrenceStarts(birthday, ms("2029-01-01T00:00:00Z"), BERLIN).map((s) =>
      s.type === "date" ? s.date : "",
    );
    expect(leap).toEqual(["2024-02-29", "2028-02-29"]);
  });
});

describe("pim mock", () => {
  it("routes exactly its requests", () => {
    expect(new Set(PIM_REQUESTS).size).toBe(PIM_REQUESTS.length);
    expect(PIM_REQUESTS).toHaveLength(52);
    expect(isPimRequest({ type: "ListPimSources" })).toBe(true);
    expect(isPimRequest({ type: "Ping" })).toBe(false);
  });

  it("can still answer like the core at the contract stage", async () => {
    const t = new MockTransport();
    t.data.features.pim.unsupported = true;
    expect(await t.send({ type: "ListPimSources" })).toMatchObject({
      type: "Error",
      code: "unsupported",
    });
  });

  it("starts with demo data and an empty variant", async () => {
    const t = new MockTransport();
    const sources = await ok(t, { type: "ListPimSources" }, "PimSources");
    expect(sources.sources.map((s) => s.kind)).toEqual(["local", "ics", "caldav", "carddav"]);
    const empty = createPimMockState({ empty: true });
    expect(empty.calendars).toEqual([]);
    expect(empty.sources.map((s) => s.id)).toEqual(["local"]);
  });

  it("answers every PIM request without unsupported", async () => {
    const t = new MockTransport();
    const samples: Record<(typeof PIM_REQUESTS)[number], Request> = {
      ListPimSources: { type: "ListPimSources" },
      AddPimSource: {
        type: "AddPimSource",
        config: {
          id: "feed",
          name: "Feed",
          enabled: true,
          settings: {
            kind: "ics",
            url: "https://x.example/a.ics",
            color: null,
            alarms: false,
            refresh_minutes: null,
            auth: null,
          },
        },
        secret: null,
      },
      UpdatePimSource: {
        type: "UpdatePimSource",
        config: {
          id: "holidays",
          name: "Hol",
          enabled: false,
          settings: {
            kind: "ics",
            url: "https://x.example/h.ics",
            color: null,
            alarms: false,
            refresh_minutes: null,
            auth: null,
          },
        },
        secret: null,
      },
      RemovePimSource: { type: "RemovePimSource", source_id: "nope" },
      TestPimSource: {
        type: "TestPimSource",
        config: {
          id: "t",
          name: null,
          enabled: true,
          settings: {
            kind: "caldav",
            account: null,
            url: "https://dav.example/",
            auth: null,
            refresh_minutes: null,
          },
        },
        secret: null,
      },
      SyncPimSources: { type: "SyncPimSources", source_id: null },
      ListCalendars: { type: "ListCalendars", source_id: null },
      CreateCalendar: {
        type: "CreateCalendar",
        source_id: "local",
        settings: {
          name: "New",
          description: null,
          color: null,
          visible: true,
          reminders: true,
          default_alarms: [],
          order: 9,
        },
        components: ["event"],
      },
      UpdateCalendar: {
        type: "UpdateCalendar",
        calendar_id: "cal-personal",
        settings: {
          name: "Mine",
          description: null,
          color: "#000",
          visible: true,
          reminders: true,
          default_alarms: [],
          order: 0,
        },
      },
      DeleteCalendar: { type: "DeleteCalendar", calendar_id: "nope" },
      ListEventInstances: {
        type: "ListEventInstances",
        from: "2026-01-01T00:00:00Z",
        to: "2026-01-08T00:00:00Z",
        calendar_ids: null,
        tz: BERLIN,
      },
      GetEvent: { type: "GetEvent", event_id: "ev-lunch" },
      CreateEvent: {
        type: "CreateEvent",
        calendar_id: "cal-personal",
        fields: fields("2026-10-05T09:00:00", "2026-10-05T10:00:00"),
        recurrence: null,
      },
      UpdateEvent: {
        type: "UpdateEvent",
        event_id: "ev-lunch",
        scope: "all",
        recurrence_id: null,
        fields: fields("2026-10-05T09:00:00", "2026-10-05T10:00:00"),
        recurrence: null,
      },
      DeleteEvent: {
        type: "DeleteEvent",
        event_id: "ev-planning",
        scope: "all",
        recurrence_id: null,
      },
      MoveEvent: { type: "MoveEvent", event_id: "ev-review", calendar_id: "cal-personal" },
      EventFromMessage: { type: "EventFromMessage", message_id: t.data.messages[0]?.id ?? "" },
      FindFreeTime: {
        type: "FindFreeTime",
        from: "2026-01-05T00:00:00Z",
        to: "2026-01-06T00:00:00Z",
        duration_minutes: 30,
        calendar_ids: null,
        tz: BERLIN,
      },
      ListAddressBooks: { type: "ListAddressBooks", source_id: null },
      CreateAddressBook: {
        type: "CreateAddressBook",
        source_id: "local",
        settings: { name: "Club", description: null, visible: true, autocomplete: true, order: 5 },
      },
      UpdateAddressBook: {
        type: "UpdateAddressBook",
        address_book_id: "ab-personal",
        settings: {
          name: "People",
          description: null,
          visible: true,
          autocomplete: true,
          order: 0,
        },
      },
      DeleteAddressBook: { type: "DeleteAddressBook", address_book_id: "nope" },
      ListContacts: {
        type: "ListContacts",
        address_book_ids: null,
        group_id: null,
        offset: 0,
        limit: 10,
      },
      GetContact: { type: "GetContact", contact_id: "ct-ada" },
      CreateContact: {
        type: "CreateContact",
        address_book_id: "ab-personal",
        fields: { ...must(t.data.features.pim.contacts[0]).fields, display_name: "Copy" },
      },
      UpdateContact: {
        type: "UpdateContact",
        contact_id: "ct-linus",
        fields: { ...must(t.data.features.pim.contacts[5]).fields, display_name: "Linus T." },
      },
      DeleteContacts: { type: "DeleteContacts", contact_ids: ["nope"] },
      MoveContact: { type: "MoveContact", contact_id: "ct-linus", address_book_id: "ab-personal" },
      SearchContacts: { type: "SearchContacts", query: "ada", address_book_ids: null, limit: 5 },
      FindDuplicateContacts: { type: "FindDuplicateContacts", address_book_ids: null },
      MergeContacts: {
        type: "MergeContacts",
        primary_id: "nope",
        contact_ids: [],
        fields: must(t.data.features.pim.contacts[0]).fields,
      },
      UnlinkContact: { type: "UnlinkContact", contact_id: "ct-ada" },
      GetContactPhoto: { type: "GetContactPhoto", contact_id: "ct-ada" },
      ListContactGroups: { type: "ListContactGroups", address_book_ids: null },
      CreateContactGroup: {
        type: "CreateContactGroup",
        address_book_id: "ab-personal",
        name: "Friends",
        member_ids: ["ct-ada"],
        member_emails: [],
      },
      UpdateContactGroup: {
        type: "UpdateContactGroup",
        group_id: "grp-family",
        name: "Kin",
        member_ids: [],
        member_emails: [],
      },
      DeleteContactGroup: { type: "DeleteContactGroup", group_id: "nope" },
      ListTasks: { type: "ListTasks", calendar_ids: null, include_completed: true },
      GetTask: { type: "GetTask", task_id: "tk-report" },
      CreateTask: {
        type: "CreateTask",
        calendar_id: "list-tasks",
        fields: { ...must(t.data.features.pim.tasks[3]).fields, title: "New" },
        recurrence: null,
      },
      UpdateTask: {
        type: "UpdateTask",
        task_id: "tk-groceries",
        fields: { ...must(t.data.features.pim.tasks[3]).fields, title: "Shop" },
        recurrence: null,
      },
      CompleteTask: { type: "CompleteTask", task_id: "tk-groceries", completed: true },
      MoveTask: { type: "MoveTask", task_id: "tk-groceries", calendar_id: "cal-work" },
      DeleteTask: { type: "DeleteTask", task_id: "nope", with_subtasks: false },
      TaskFromMessage: { type: "TaskFromMessage", message_id: t.data.messages[0]?.id ?? "" },
      SearchPim: { type: "SearchPim", query: "review", kinds: [], limit: 10 },
      PreviewPimImport: {
        type: "PreviewPimImport",
        source: {
          type: "data",
          file_name: "a.vcf",
          content: "BEGIN:VCARD\nVERSION:4.0\nFN:Zed\nEND:VCARD\n",
        },
      },
      ImportPim: {
        type: "ImportPim",
        source: {
          type: "data",
          file_name: "a.vcf",
          content: "BEGIN:VCARD\nVERSION:4.0\nFN:Zed\nEND:VCARD\n",
        },
        calendar_id: null,
        address_book_id: null,
      },
      ResolvePimConflict: {
        type: "ResolvePimConflict",
        item: { type: "contact", contact_id: "ct-ada" },
        keep: "server",
      },
      ListReminders: { type: "ListReminders", until: null },
      SnoozeReminders: {
        type: "SnoozeReminders",
        reminder_ids: ["rem-review"],
        until: "2030-01-01T00:00:00Z",
      },
      DismissReminders: { type: "DismissReminders", reminder_ids: ["rem-review"] },
    };
    for (const type of PIM_REQUESTS) {
      const response = await t.send(samples[type]);
      expect(response, type).not.toMatchObject({ code: "unsupported" });
      expect(response.type === "Error" ? (response as { code: string }).code : "ok", type).toMatch(
        /^(ok|not_found)$/,
      );
    }
  });

  it("queues writes and reports them like the PIM queue", async () => {
    vi.useFakeTimers();
    try {
      const t = new MockTransport();
      const events: NailEvent[] = [];
      t.onEvent((e) => events.push(e));
      t.data.features.pim.failNextOp = true;
      const saved = await ok(
        t,
        {
          type: "CreateEvent",
          calendar_id: "cal-personal",
          fields: fields("2026-10-05T09:00:00", "2026-10-05T10:00:00"),
          recurrence: null,
        },
        "EventSaved",
      );
      expect(saved.op_ids).toHaveLength(1);
      expect(events).toContainEqual({ type: "EventsChanged", calendar_ids: ["cal-personal"] });
      await vi.advanceTimersByTimeAsync(50);
      expect(events.find((e) => e.type === "PimOpStatus")).toMatchObject({
        status: { op_id: saved.op_ids[0], state: "failed", error: "network" },
      });
    } finally {
      vi.useRealTimers();
    }
  });

  it("refuses invalid data with the core's issue ids and read-only calendars", async () => {
    const t = new MockTransport();
    expect(
      await t.send({
        type: "CreateEvent",
        calendar_id: "cal-personal",
        fields: fields("2026-10-05T10:00:00", "2026-10-05T09:00:00"),
        recurrence: null,
      }),
    ).toMatchObject({
      type: "Error",
      code: "invalid_request",
      localized: { id: "pim-issue-end-before-start" },
    });
    expect(
      await t.send({
        type: "CreateEvent",
        calendar_id: "cal-holidays",
        fields: fields("2026-10-05T09:00:00", "2026-10-05T10:00:00"),
        recurrence: null,
      }),
    ).toMatchObject({ type: "Error", code: "forbidden" });
    const bad = { ...fields("2026-10-05T09:00:00", "2026-10-05T10:00:00") };
    bad.attendees = [
      {
        email: "nope",
        name: null,
        role: "required",
        status: "needs-action",
        kind: "individual",
        rsvp: false,
        delegated_to: [],
        delegated_from: [],
      },
    ];
    expect(
      await t.send({
        type: "CreateEvent",
        calendar_id: "cal-personal",
        fields: bad,
        recurrence: null,
      }),
    ).toMatchObject({
      localized: { id: "pim-issue-invalid-email" },
    });
    expect(
      await t.send({
        type: "UpdateEvent",
        event_id: "ev-standup",
        scope: "this",
        recurrence_id: null,
        fields: fields("2026-10-05T09:00:00", "2026-10-05T10:00:00"),
        recurrence: null,
      }),
    ).toMatchObject({ localized: { id: "pim-issue-invalid-scope" } });
  });
});

describe("edit scopes (mock)", () => {
  function setup() {
    const t = new MockTransport();
    t.data.features.pim.events = [series("s", weekly(["mon"]))];
    return t;
  }
  const occurrence = (local: string) => ({ type: "zoned" as const, local, tz: BERLIN });
  const list = async (t: MockTransport) =>
    (
      await ok(
        t,
        {
          type: "ListEventInstances",
          from: "2026-10-05T00:00:00Z",
          to: "2026-11-03T00:00:00Z",
          calendar_ids: ["cal-personal"],
          tz: BERLIN,
        },
        "EventInstances",
      )
    ).instances.filter((i) => i.event_id === "s" || i.title !== "Lunch with Grace");

  it("this: overrides one occurrence", async () => {
    const t = setup();
    await ok(
      t,
      {
        type: "UpdateEvent",
        event_id: "s",
        scope: "this",
        recurrence_id: occurrence("2026-10-12T09:00:00"),
        fields: fields("2026-10-13T11:00:00", "2026-10-13T12:00:00", "Moved"),
        recurrence: null,
      },
      "EventSaved",
    );
    const instances = (await list(t)).filter((i) => i.event_id === "s");
    expect(instances.map((i) => i.title)).toEqual([
      "Meeting",
      "Moved",
      "Meeting",
      "Meeting",
      "Meeting",
    ]);
    expect(instances[1]).toMatchObject({ start_utc: "2026-10-13T09:00:00Z", overridden: true });
  });

  it("this and following: splits the series and returns the new one", async () => {
    const t = setup();
    const saved = await ok(
      t,
      {
        type: "UpdateEvent",
        event_id: "s",
        scope: "this-and-following",
        recurrence_id: occurrence("2026-10-19T09:00:00"),
        fields: fields("2026-10-19T14:00:00", "2026-10-19T15:00:00", "Later"),
        recurrence: weekly(["mon"]),
      },
      "EventSaved",
    );
    expect(saved.split).not.toBeNull();
    expect(saved.event.recurrence?.rules[0]?.end).toEqual({
      type: "until",
      until: { type: "zoned", local: "2026-10-19T06:59:59", tz: "UTC" },
    });
    const instances = (await list(t)).filter(
      (i) => i.event_id === "s" || i.event_id === saved.split?.id,
    );
    expect(instances.map((i) => `${i.title}@${i.start_utc.slice(5, 16)}`)).toEqual([
      "Meeting@10-05T07:00",
      "Meeting@10-12T07:00",
      "Later@10-19T12:00",
      "Later@10-26T13:00",
      "Later@11-02T13:00",
    ]);
  });

  it("all: moves the series and its overrides; deleting uses scopes too", async () => {
    const t = setup();
    await ok(
      t,
      {
        type: "UpdateEvent",
        event_id: "s",
        scope: "this",
        recurrence_id: occurrence("2026-10-12T09:00:00"),
        fields: fields("2026-10-12T09:00:00", "2026-10-12T10:00:00", "Special"),
        recurrence: null,
      },
      "EventSaved",
    );
    await ok(
      t,
      {
        type: "UpdateEvent",
        event_id: "s",
        scope: "all",
        recurrence_id: null,
        fields: fields("2026-10-06T09:00:00", "2026-10-06T10:00:00"),
        recurrence: weekly(["tue"]),
      },
      "EventSaved",
    );
    const event = (await ok(t, { type: "GetEvent", event_id: "s" }, "PimEvent")).event;
    expect(event.overrides[0]?.recurrence_id).toEqual(occurrence("2026-10-13T09:00:00"));
    await ok(
      t,
      {
        type: "DeleteEvent",
        event_id: "s",
        scope: "this",
        recurrence_id: occurrence("2026-10-20T09:00:00"),
      },
      "PimDeleted",
    );
    let titles = (await list(t))
      .filter((i) => i.event_id === "s")
      .map((i) => i.start_utc.slice(5, 10));
    // The override keeps its own times (Monday the 12th); its recurrence id moved with the series.
    expect(titles).toEqual(["10-06", "10-12", "10-27"]);
    await ok(
      t,
      {
        type: "DeleteEvent",
        event_id: "s",
        scope: "this-and-following",
        recurrence_id: occurrence("2026-10-27T09:00:00"),
      },
      "PimDeleted",
    );
    titles = (await list(t)).filter((i) => i.event_id === "s").map((i) => i.start_utc.slice(5, 10));
    expect(titles).toEqual(["10-06", "10-12"]);
    await ok(
      t,
      { type: "DeleteEvent", event_id: "s", scope: "all", recurrence_id: null },
      "PimDeleted",
    );
    expect(t.data.features.pim.events.find((e) => e.id === "s")).toBeUndefined();
  });
});

describe("contacts, tasks, reminders, sources, import (mock)", () => {
  it("finds duplicates, folds same-book contacts and links others", async () => {
    const t = new MockTransport();
    const dups = await ok(
      t,
      { type: "FindDuplicateContacts", address_book_ids: null },
      "ContactDuplicates",
    );
    expect(dups.groups).toContainEqual({
      contact_ids: ["ct-grace", "ct-grace-work"],
      reasons: ["email", "name"],
    });
    const copy = await ok(
      t,
      {
        type: "CreateContact",
        address_book_id: "ab-personal",
        fields: { ...must(t.data.features.pim.contacts[1]).fields, display_name: "G. Hopper" },
      },
      "ContactSaved",
    );
    const primary = must(t.data.features.pim.contacts[1]);
    const merged = await ok(
      t,
      {
        type: "MergeContacts",
        primary_id: "ct-grace",
        contact_ids: [copy.contact.id, "ct-grace-work"],
        fields: primary.fields,
      },
      "ContactSaved",
    );
    expect(merged.contact.linked_ids).toEqual(["ct-grace-work"]);
    expect(t.data.features.pim.contacts.some((c) => c.id === copy.contact.id)).toBe(false);
    expect(t.data.features.pim.contacts.find((c) => c.id === "ct-grace-work")?.linked_ids).toEqual([
      "ct-grace",
    ]);
    // Linked contacts are one person: no longer duplicates.
    const after = await ok(
      t,
      { type: "FindDuplicateContacts", address_book_ids: null },
      "ContactDuplicates",
    );
    expect(after.groups).toEqual([]);
    await ok(t, { type: "UnlinkContact", contact_id: "ct-grace-work" }, "ContactSaved");
    expect(t.data.features.pim.contacts.find((c) => c.id === "ct-grace")?.linked_ids).toEqual([]);
  });

  it("pages, searches and groups contacts", async () => {
    const t = new MockTransport();
    const page = await ok(
      t,
      { type: "ListContacts", address_book_ids: null, group_id: null, offset: 0, limit: 2 },
      "Contacts",
    );
    expect(page.total).toBe(6);
    expect(page.contacts).toHaveLength(2);
    const team = await ok(
      t,
      { type: "ListContacts", address_book_ids: null, group_id: "grp-team", offset: 0, limit: 10 },
      "Contacts",
    );
    expect(team.contacts.map((c) => c.id).sort()).toEqual(["ct-alan", "ct-grace-work"]);
    const matches = await ok(
      t,
      { type: "SearchContacts", query: "bletchley", address_book_ids: null, limit: 5 },
      "ContactMatches",
    );
    expect(matches.matches[0]).toMatchObject({ field: "email", contact: { id: "ct-alan" } });
    expect(
      await t.send({
        type: "CreateContact",
        address_book_id: "ab-personal",
        fields: {
          ...must(t.data.features.pim.contacts[0]).fields,
          display_name: "",
          name: null,
          organization: null,
        },
      }),
    ).toMatchObject({
      localized: { id: "pim-issue-missing-name" },
    });
    const group = await ok(
      t,
      {
        type: "UpdateContactGroup",
        group_id: "grp-team",
        name: "Team",
        member_ids: ["ct-alan", "ct-ada"],
        member_emails: ["x@y.example"],
      },
      "ContactGroupSaved",
    );
    // Members must be in the group's address book.
    expect(group.group.member_ids).toEqual(["ct-alan"]);
  });

  it("keeps subtasks, refuses cycles, moves recurring tasks on", async () => {
    const t = new MockTransport();
    const pim = t.data.features.pim;
    const report = must(pim.tasks.find((x) => x.id === "tk-report"));
    expect(
      await t.send({
        type: "UpdateTask",
        task_id: "tk-report",
        fields: { ...report.fields, parent_id: "tk-report-draft" },
        recurrence: null,
      }),
    ).toMatchObject({ localized: { id: "pim-issue-task-cycle" } });
    const sub = await ok(
      t,
      {
        type: "CreateTask",
        calendar_id: "list-tasks",
        fields: { ...must(pim.tasks[3]).fields, title: "Proofread", parent_id: "tk-report" },
        recurrence: null,
      },
      "TaskSaved",
    );
    expect(pim.tasks.find((x) => x.id === "tk-report")?.subtask_ids).toContain(sub.task.id);
    const weekly = must(pim.tasks.find((x) => x.id === "tk-weekly"));
    const due = weekly.fields.due;
    const done = await ok(
      t,
      { type: "CompleteTask", task_id: "tk-weekly", completed: true },
      "TaskSaved",
    );
    expect(done.task.fields.status).toBe("needs-action");
    expect(done.task.fields.due).not.toEqual(due);
    const open = await ok(
      t,
      { type: "ListTasks", calendar_ids: null, include_completed: false },
      "Tasks",
    );
    expect(open.tasks.some((x) => x.id === "tk-done")).toBe(false);
    await ok(t, { type: "DeleteTask", task_id: "tk-report", with_subtasks: true }, "PimDeleted");
    expect(pim.tasks.some((x) => x.fields.parent_id === "tk-report" || x.id === "tk-report")).toBe(
      false,
    );
  });

  it("fires, snoozes and dismisses reminders", async () => {
    const t = new MockTransport();
    const events: NailEvent[] = [];
    t.onEvent((e) => events.push(e));
    expect(fireReminder(t, "rem-review")?.state).toBe("fired");
    expect(events[0]).toMatchObject({ type: "ReminderDue", reminder: { id: "rem-review" } });
    await ok(
      t,
      { type: "SnoozeReminders", reminder_ids: ["rem-review"], until: "2030-01-01T00:00:00Z" },
      "Done",
    );
    expect(t.data.features.pim.reminders[0]).toMatchObject({
      state: "snoozed",
      snoozed_until: "2030-01-01T00:00:00Z",
    });
    const soon = await ok(t, { type: "ListReminders", until: "2029-01-01T00:00:00Z" }, "Reminders");
    expect(soon.reminders).toEqual([]);
    await ok(t, { type: "DismissReminders", reminder_ids: ["rem-review"] }, "Done");
    expect((await ok(t, { type: "ListReminders", until: null }, "Reminders")).reminders).toEqual(
      [],
    );
  });

  it("adds, tests, syncs and removes sources", async () => {
    const t = new MockTransport();
    const config = {
      id: "club",
      name: "Club",
      enabled: true,
      settings: {
        kind: "ics" as const,
        url: "webcal://club.example/c.ics",
        color: "#f00",
        alarms: false,
        refresh_minutes: 60,
        auth: null,
      },
    };
    await ok(t, { type: "AddPimSource", config, secret: null }, "PimSourceSaved");
    expect(t.data.accountsConfig.pim_sources.map((s) => s.id)).toContain("club");
    expect(t.data.features.pim.calendars.find((c) => c.source_id === "club")?.read_only).toBe(true);
    expect(await t.send({ type: "AddPimSource", config, secret: null })).toMatchObject({
      code: "conflict",
    });
    expect(
      await t.send({ type: "AddPimSource", config: { ...config, id: "Bad Id" }, secret: null }),
    ).toMatchObject({
      localized: { id: "pim-source-issue-invalid-id" },
    });
    const failed = await ok(
      t,
      {
        type: "TestPimSource",
        config: { ...config, settings: { ...config.settings, url: "https://fail.example/" } },
        secret: null,
      },
      "PimSourceTested",
    );
    expect(failed.result).toMatchObject({ ok: false, error: "auth_failed" });
    await ok(t, { type: "SyncPimSources", source_id: "club" }, "Done");
    await ok(t, { type: "RemovePimSource", source_id: "club" }, "Done");
    expect(t.data.features.pim.calendars.some((c) => c.source_id === "club")).toBe(false);
    expect(await t.send({ type: "RemovePimSource", source_id: "local" })).toMatchObject({
      type: "Error",
    });
  });

  it("previews and imports iCalendar and vCard text", async () => {
    const t = new MockTransport();
    const ics =
      "BEGIN:VCALENDAR\r\nX-WR-CALNAME:Trip\r\nBEGIN:VEVENT\r\nUID:a\r\nSUMMARY:Flight\r\nDTSTART:20261101T080000Z\r\nDTEND:20261101T100000Z\r\nEND:VEVENT\r\nBEGIN:VTODO\r\nSUMMARY:Pack\r\nEND:VTODO\r\nEND:VCALENDAR\r\n";
    expect(parseImport(ics).items).toHaveLength(2);
    const preview = await ok(
      t,
      { type: "PreviewPimImport", source: { type: "data", file_name: "trip.ics", content: ics } },
      "PimImportPreview",
    );
    expect(preview.preview).toMatchObject({
      format: "icalendar",
      name: "Trip",
      events: 1,
      tasks: 1,
      sample: ["Flight", "Pack"],
    });
    const imported = await ok(
      t,
      {
        type: "ImportPim",
        source: { type: "data", file_name: "trip.ics", content: ics },
        calendar_id: "cal-personal",
        address_book_id: null,
      },
      "PimImported",
    );
    expect(imported.result.created).toBe(2);
    expect(
      t.data.features.pim.events.some(
        (e) => e.fields.title === "Flight" && e.fields.start.type === "zoned",
      ),
    ).toBe(true);
    expect(
      await t.send({
        type: "PreviewPimImport",
        source: { type: "data", file_name: "x.txt", content: "hello" },
      }),
    ).toMatchObject({ code: "invalid_request" });
  });

  it("finds free time between busy events within working hours", async () => {
    const t = new MockTransport();
    t.data.features.pim.events = [
      series("busy", null, fields("2026-10-05T10:00:00", "2026-10-05T12:00:00")),
    ];
    const free = await ok(
      t,
      {
        type: "FindFreeTime",
        from: "2026-10-04T22:00:00Z",
        to: "2026-10-05T22:00:00Z",
        duration_minutes: 60,
        calendar_ids: null,
        tz: BERLIN,
      },
      "FreeTime",
    );
    expect(free.slots).toEqual([
      { start: "2026-10-05T06:00:00Z", end: "2026-10-05T08:00:00Z" },
      { start: "2026-10-05T10:00:00Z", end: "2026-10-05T16:00:00Z" },
    ]);
  });
});

describe("pim events", () => {
  it("refetches PIM queries and turns reminders and failed writes into toasts", () => {
    const queryClient = new QueryClient();
    const store = createUiStore();
    const invalidate = vi.spyOn(queryClient, "invalidateQueries");
    const ctx = { queryClient, store } as unknown as FeatureEventContext;
    handlePimEvent({ type: "EventsChanged", calendar_ids: ["c1"] }, ctx);
    expect(invalidate).toHaveBeenCalledWith({ queryKey: pimKeys.all });
    invalidate.mockClear();
    handlePimEvent({ type: "AccountsChanged" }, ctx);
    expect(invalidate).not.toHaveBeenCalled();
    expect(PIM_EVENTS).toContain("ReminderDue");
    const reminder = must(new MockTransport().data.features.pim.reminders[0]);
    handlePimEvent({ type: "ReminderDue", reminder }, ctx);
    handlePimEvent(
      {
        type: "PimOpStatus",
        status: {
          op_id: "o1",
          source_id: "local",
          state: "failed",
          error: "network",
          message: null,
        },
      },
      ctx,
    );
    handlePimEvent(
      {
        type: "PimOpStatus",
        status: { op_id: "o2", source_id: "local", state: "done", error: null, message: null },
      },
      ctx,
    );
    expect(
      pimStoreFor(store)
        .getState()
        .toasts.map((x) => x.id),
    ).toEqual(["reminder-rem-review", "op-o1"]);
  });
});
