import { describe, expect, it } from "vitest";
import type { Request, Task } from "../../api/generated";
import { DEFAULT_CALENDAR_PREFERENCES } from "../../api/generated";
import { MockTransport } from "../../api/mock";
import { must } from "../../test/must";
import { mockOk as ok } from "./testing";
import {
  defaultPreferences,
  mockParseEventText,
  preferencesIssue,
  tasksDue,
} from "./mockCalendarView";
import { myStatusOf } from "./mockExpand";
import { emptyEventFields, emptyTaskFields } from "./mockSeed";

/** 25 November 2026, 10:40 UTC (a Wednesday). */
const NOW = Date.UTC(2026, 10, 25, 10, 40);

describe("calendar preferences (mock)", () => {
  it("starts with the core's defaults and keeps a copy", () => {
    const prefs = defaultPreferences();
    expect(prefs).toEqual(DEFAULT_CALENDAR_PREFERENCES);
    prefs.work_days.push("sat");
    expect(DEFAULT_CALENDAR_PREFERENCES.work_days).not.toContain("sat");
  });

  it("checks them like the core", () => {
    const prefs = defaultPreferences();
    expect(preferencesIssue(prefs)).toBeNull();
    expect(preferencesIssue({ ...prefs, work_hours: { start_minute: 600, end_minute: 540 } })).toBe(
      "invalid-work-hours",
    );
    expect(preferencesIssue({ ...prefs, work_days: ["mon", "mon"] })).toBe("too-many");
    expect(preferencesIssue({ ...prefs, second_time_zone: "Atlantis/Lost_City" })).toBe(
      "invalid-time-zone",
    );
  });

  it("saves them and announces the change", async () => {
    const t = new MockTransport();
    const events: string[] = [];
    t.onEvent((e) => events.push(e.type));
    const preferences = {
      ...defaultPreferences(),
      show_week_numbers: true,
      week_start: "mon" as const,
    };
    await ok(t, { type: "SetCalendarPreferences", preferences }, "CalendarPreferences");
    const got = await ok(t, { type: "GetCalendarPreferences" }, "CalendarPreferences");
    expect(got.preferences).toEqual(preferences);
    expect(events).toContain("CalendarPreferencesChanged");
    const refused = await t.send({
      type: "SetCalendarPreferences",
      preferences: { ...preferences, work_hours: { start_minute: 0, end_minute: 1441 } },
    });
    expect(refused).toMatchObject({
      code: "invalid_request",
      localized: { id: "pim-issue-invalid-work-hours" },
    });
  });
});

describe("tasks due (mock)", () => {
  const task = (
    id: string,
    due: Task["fields"]["due"],
    status: Task["fields"]["status"] = "needs-action",
  ): Task => ({
    id,
    calendar_id: "list",
    uid: id,
    fields: { ...emptyTaskFields(), title: id, due, status },
    recurrence: null,
    subtask_ids: [],
    created_at: null,
    updated_at: null,
    read_only: false,
    sync: "synced",
  });

  it("keeps the open tasks due in the range, by due, placing dates in the zone", () => {
    const tasks = [
      task("later", { type: "date", date: "2026-11-27" }),
      task("none", null),
      task("first", { type: "zoned", local: "2026-11-25T09:00:00", tz: "Europe/Berlin" }),
      task("done", { type: "date", date: "2026-11-26" }, "completed"),
      task("next-week", { type: "date", date: "2026-12-03" }),
    ];
    const week = tasksDue(
      tasks,
      "2026-11-22T23:00:00Z",
      "2026-11-29T23:00:00Z",
      "Europe/Berlin",
      false,
    );
    expect(week.map((t) => t.id)).toEqual(["first", "later"]);
    const all = tasksDue(
      tasks,
      "2026-11-22T23:00:00Z",
      "2026-11-29T23:00:00Z",
      "Europe/Berlin",
      true,
    );
    expect(all.map((t) => t.id)).toEqual(["first", "done", "later"]);
  });

  it("answers ListTasksDue from the demo tasks", async () => {
    const t = new MockTransport();
    const { tasks } = await ok(
      t,
      {
        type: "ListTasksDue",
        from: new Date(Date.now() - 86_400_000).toISOString(),
        to: new Date(Date.now() + 7 * 86_400_000).toISOString(),
        tz: "UTC",
        calendar_ids: null,
        include_completed: false,
      },
      "Tasks",
    );
    expect(tasks.length).toBeGreaterThan(0);
    expect(tasks.every((x) => x.fields.due !== null)).toBe(true);
  });
});

describe("quick create from text (mock)", () => {
  it("reads a day, a time and a place and marks them", () => {
    const text = "Lunch with Bob tomorrow 12:30 at Corner café";
    const parsed = mockParseEventText(text, "Europe/Berlin", NOW);
    expect(parsed.title).toBe("Lunch with Bob");
    expect(parsed.start).toEqual({
      type: "zoned",
      local: "2026-11-26T12:30:00",
      tz: "Europe/Berlin",
    });
    expect(parsed.end).toEqual({
      type: "zoned",
      local: "2026-11-26T13:30:00",
      tz: "Europe/Berlin",
    });
    expect(parsed.location).toBe("Corner café");
    expect(parsed.spans.map((s) => [s.kind, text.slice(s.start, s.end)])).toEqual([
      ["date", "tomorrow"],
      ["time", "12:30"],
      ["location", "at Corner café"],
    ]);
  });

  it("reads ranges with am / pm, all-day days and plain titles", () => {
    const range = mockParseEventText("Review 3-4pm", "UTC", NOW);
    expect(range.start).toEqual({ type: "zoned", local: "2026-11-25T15:00:00", tz: "UTC" });
    expect(range.end).toEqual({ type: "zoned", local: "2026-11-25T16:00:00", tz: "UTC" });
    const allDay = mockParseEventText("Offsite tomorrow all day", "UTC", NOW);
    expect(allDay).toMatchObject({
      title: "Offsite",
      start: { type: "date", date: "2026-11-26" },
      end: { type: "date", date: "2026-11-27" },
    });
    expect(mockParseEventText("  Call mum ", "UTC", NOW)).toEqual({
      title: "Call mum",
      start: null,
      end: null,
      location: null,
      recurrence: null,
      spans: [],
    });
  });

  it("answers ParseEventText and refuses unknown zones and long text", async () => {
    const t = new MockTransport();
    const { parsed } = await ok(
      t,
      {
        type: "ParseEventText",
        text: "Dentist today 3pm",
        tz: "UTC",
        now: "2026-11-25T10:40:00Z",
        locale: "en",
      },
      "EventTextParsed",
    );
    expect(parsed.start).toEqual({ type: "zoned", local: "2026-11-25T15:00:00", tz: "UTC" });
    for (const request of [
      { type: "ParseEventText", text: "x", tz: "Atlantis/Lost_City", now: null, locale: null },
      { type: "ParseEventText", text: "x".repeat(501), tz: "UTC", now: null, locale: null },
    ] satisfies Request[]) {
      expect(await t.send(request)).toMatchObject({ type: "Error", code: "invalid_request" });
    }
  });
});

describe("my status (mock)", () => {
  const attendee = (email: string, status: "accepted" | "needs-action") => ({
    email,
    name: null,
    role: "required" as const,
    status,
    kind: "individual" as const,
    rsvp: true,
    delegated_to: [],
    delegated_from: [],
  });

  it("is the user's PARTSTAT as an attendee, null as the organizer or a stranger", () => {
    const mine = ["ada@example.org"];
    const invited = {
      ...emptyEventFields(
        { type: "date", date: "2026-11-26" },
        { type: "date", date: "2026-11-27" },
      ),
      organizer: { email: "grace@navy.example", name: null },
      attendees: [
        attendee("Ada@Example.org", "needs-action"),
        attendee("bob@example.com", "accepted"),
      ],
    };
    expect(myStatusOf(invited, mine)).toBe("needs-action");
    expect(
      myStatusOf({ ...invited, organizer: { email: "ada@example.org", name: null } }, mine),
    ).toBeNull();
    expect(myStatusOf(invited, ["someone@example.net"])).toBeNull();
  });

  it("comes with the demo instances", async () => {
    const t = new MockTransport();
    const { instances } = await ok(
      t,
      {
        type: "ListEventInstances",
        from: new Date(Date.now() - 40 * 86_400_000).toISOString(),
        to: new Date(Date.now() + 40 * 86_400_000).toISOString(),
        calendar_ids: null,
        tz: "UTC",
      },
      "EventInstances",
    );
    const standup = must(instances.find((i) => i.event_id === "ev-standup"));
    expect(standup.my_status).toBe("accepted");
  });
});
