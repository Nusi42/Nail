/**
 * The pure rules of the calendar's views and shell (R7, ADR 0246): narrow
 * fallbacks, the sidebar in the layout, declined invitations, the agenda's
 * rows, the month view's bars and lines, the year's heat, the free-time
 * view's blocks and totals, the nearest event, search groups, the
 * mini-month's band and dots and the quick-add chips.
 */
import { CalendarDate } from "@internationalized/date";
import type {
  CalendarEvent,
  EventFields,
  LayoutNode,
  PimSearchHit,
  PimTime,
  ResolvedLayout,
} from "../../api/generated";
import { normalizeNode } from "../../api/normalize";
import { must } from "../../test/must";
import { emptyEventFields } from "./defaults";
import { expandEvent } from "./mockExpand";
import { WORK_HOUR_STEP, prefsValid, toggleWeekday, workHourChoices } from "./CalendarPrefsForm";
import { quickFields, quickTimes } from "./QuickAdd";
import { barWidth } from "./CalendarViews";
import { yearsAround } from "./EmptyState";
import {
  MIN_DAY_COLUMN_PX,
  agendaDays,
  bandOf,
  bookedMinutes,
  busyAllDay,
  busyBlocks,
  dayDots,
  fittingDayCount,
  freeMinutes,
  groupHits,
  gutterPx,
  heatLevel,
  hourTicks,
  layoutShowsSidebar,
  monthWeek,
  narrowFallback,
  nearestOutside,
  quickChips,
} from "./viewsModel";

const TZ = "UTC";
const MONDAY = new CalendarDate(2026, 11, 23);

/** A zoned time on `day` (UTC). */
const at = (day: CalendarDate, hour: number, minute = 0): PimTime => ({
  type: "zoned",
  local: `${day.toString()}T${String(hour).padStart(2, "0")}:${String(minute).padStart(2, "0")}:00`,
  tz: TZ,
});
const allDay = (day: CalendarDate): PimTime => ({ type: "date", date: day.toString() });

/** One occurrence of a single event, as the mock core expands it. */
function occurrence(
  title: string,
  start: PimTime,
  end: PimTime,
  more: Partial<EventFields> = {},
  calendarId = "cal",
) {
  const event: CalendarEvent = {
    uid: title,
    id: `ev-${title}`,
    read_only: false,
    calendar_id: calendarId,
    sequence: 1,
    recurrence: null,
    overrides: [],
    created_at: null,
    updated_at: null,
    sync: "synced",
    fields: { ...emptyEventFields(start, end), title, ...more },
  };
  return must(expandEvent(event, 0, Number.MAX_SAFE_INTEGER, TZ, false)[0]);
}

describe("narrow windows", () => {
  it("keeps the days that fit at the narrowest column, else three, else one", () => {
    const gutter = gutterPx(1);
    expect(gutter).toBe(56);
    expect(fittingDayCount(7, 7 * MIN_DAY_COLUMN_PX + gutter, gutter)).toBe(7);
    expect(fittingDayCount(7, 7 * MIN_DAY_COLUMN_PX + gutter - 1, gutter)).toBe(3);
    expect(fittingDayCount(7, 420, gutter)).toBe(3);
    expect(fittingDayCount(7, 3 * MIN_DAY_COLUMN_PX + gutter - 1, gutter)).toBe(1);
    expect(fittingDayCount(5, 300, gutter)).toBe(1);
    // Not measured yet: nothing changes.
    expect(fittingDayCount(7, 0, gutter)).toBe(7);
  });

  it("falls back to three days or one day for time grids only", () => {
    expect(narrowFallback("week", 7, 420, 1)).toEqual({ view: "multi-day", days: 3 });
    expect(narrowFallback("week", 7, 300, 1)).toEqual({ view: "day", days: 1 });
    expect(narrowFallback("week", 7, 1200, 1)).toBeNull();
    // A second zone takes another gutter.
    expect(narrowFallback("week", 7, 7 * MIN_DAY_COLUMN_PX + 60, 2)).toEqual({
      view: "multi-day",
      days: 3,
    });
    expect(narrowFallback("day", 1, 200, 1)).toBeNull();
    expect(narrowFallback("month", 35, 300, 1)).toBeNull();
    expect(narrowFallback("agenda", 14, 300, 1)).toBeNull();
  });

  const widget = (kind: string, id: string): LayoutNode =>
    normalizeNode({ type: "widget", kind, id });
  const stack = (children: LayoutNode[]): LayoutNode => normalizeNode({ type: "stack", children });
  const main = widget("calendar", "calendar-main");
  const layout = {
    windows: {
      calendar: [
        { min_width: 900, max_width: null, root: stack([widget("mini-month", "m"), main]) },
        { min_width: null, max_width: 899, root: stack([main]) },
      ],
    },
  } as unknown as Pick<ResolvedLayout, "windows">;

  it("tells whether the calendar's window shows the sidebar at a width", () => {
    expect(layoutShowsSidebar(layout, main, 1200)).toBe(true);
    expect(layoutShowsSidebar(layout, main, 420)).toBe(false);
    // Found by id in a fresh copy of the tree.
    expect(layoutShowsSidebar(layout, widget("calendar", "calendar-main"), 420)).toBe(false);
    // A calendar no window holds, or no layout: nothing to replace.
    expect(layoutShowsSidebar(layout, widget("calendar", "elsewhere"), 420)).toBe(true);
    expect(layoutShowsSidebar(null, main, 420)).toBe(true);
  });
});

describe("agenda", () => {
  const tuesday = MONDAY.add({ days: 1 });
  const nowMs = Date.UTC(2026, 10, 23, 11, 0);
  const early = occurrence("Early", at(MONDAY, 8), at(MONDAY, 9));
  const late = occurrence("Late", at(MONDAY, 15), at(MONDAY, 16));
  const night = occurrence("Release night", at(MONDAY, 22), at(tuesday, 1));
  const trip = occurrence("Trip", allDay(MONDAY), allDay(MONDAY.add({ days: 3 })));

  it("labels today and tomorrow, fades the past and puts the now marker in time", () => {
    const days = agendaDays(
      [late, early, night, trip],
      { start: MONDAY, count: 3 },
      TZ,
      MONDAY,
      nowMs,
    );
    expect(days.map((d) => [d.day.toString(), d.relative])).toEqual([
      ["2026-11-23", "today"],
      ["2026-11-24", "tomorrow"],
      ["2026-11-25", null],
    ]);
    const today = must(days[0]);
    expect(today.rows.map((r) => [r.instance.title, r.time, r.past])).toEqual([
      ["Trip", "all-day", false],
      ["Early", "timed", true],
      ["Late", "timed", false],
      ["Release night", "timed", false],
    ]);
    // Between the past event and the next one.
    expect(today.nowAt).toBe(2);
    expect(must(days[1]).nowAt).toBeNull();
  });

  it("shows the second day of a midnight-crossing event as 'until'", () => {
    const days = agendaDays([night], { start: MONDAY, count: 2 }, TZ, MONDAY, nowMs);
    expect(must(days[1]).rows.map((r) => r.time)).toEqual(["until"]);
    // After the last event of today the marker goes last.
    const after = agendaDays([early], { start: MONDAY, count: 1 }, TZ, MONDAY, nowMs);
    expect(must(after[0]).nowAt).toBe(1);
  });

  it("leaves out days without events", () => {
    expect(agendaDays([], { start: MONDAY, count: 7 }, TZ, MONDAY, nowMs)).toEqual([]);
  });
});

describe("month view", () => {
  const week = Array.from({ length: 7 }, (_, i) => MONDAY.add({ days: i }));
  const trip = occurrence("Trip", allDay(MONDAY.add({ days: 1 })), allDay(MONDAY.add({ days: 4 })));
  const offsite = occurrence(
    "Offsite",
    allDay(MONDAY.add({ days: 2 })),
    allDay(MONDAY.add({ days: 3 })),
  );
  const lunch = occurrence(
    "Lunch",
    at(MONDAY.add({ days: 2 }), 12),
    at(MONDAY.add({ days: 2 }), 13),
  );
  const night = occurrence("Night", at(MONDAY, 22), at(MONDAY.add({ days: 1 }), 1));

  it("draws a multi-day event as one bar from its first day and keeps the lanes aligned", () => {
    const cells = monthWeek(week, [trip, offsite, lunch], TZ, 4);
    const tuesday = must(cells[1]);
    const bar = must(tuesday.lanes[0]);
    expect(bar.kind === "bar" && [bar.span.row, bar.days, bar.span.instance.title]).toEqual([
      0,
      3,
      "Trip",
    ]);
    const wednesday = must(cells[2]);
    // The trip passes (a gap in lane 0), the offsite starts in lane 1, then the lunch line.
    expect(wednesday.lanes.map((l) => l.kind)).toEqual(["gap", "bar"]);
    expect(wednesday.lines.map((i) => i.title)).toEqual(["Lunch"]);
    expect(wednesday.total).toBe(3);
    // Nothing on Sunday: no lanes at all.
    expect(must(cells[6]).lanes).toEqual([]);
  });

  it("shows a timed event once, on its start day, and counts what does not fit", () => {
    const cells = monthWeek(week, [night], TZ, 4);
    expect(must(cells[0]).lines.map((i) => i.title)).toEqual(["Night"]);
    expect(must(cells[1]).lines).toEqual([]);
    const crowded = monthWeek(week, [trip, offsite, lunch], TZ, 1);
    const wednesday = must(crowded[2]);
    expect(wednesday.lanes.map((l) => l.kind)).toEqual(["gap"]);
    expect(wednesday.lines).toEqual([]);
    expect(wednesday.more).toBe(2);
  });

  it("widens a bar over the cells it spans", () => {
    expect(barWidth(1)).toBe("100%");
    expect(barWidth(3)).toBe("calc(3 * 100% + 2 * (2 * var(--nail-space-xxs) + 1px))");
  });
});

describe("year heat", () => {
  it("adds the booked minutes per day, an all-day event as a working day", () => {
    const work = { start_minute: 480, end_minute: 1080 };
    const booked = bookedMinutes(
      [
        occurrence("A", at(MONDAY, 9), at(MONDAY, 11)),
        occurrence("B", at(MONDAY, 13), at(MONDAY, 14, 30)),
        occurrence("Night", at(MONDAY, 23), at(MONDAY.add({ days: 1 }), 1)),
        occurrence("Free", at(MONDAY, 15), at(MONDAY, 16), { transparency: "free" }),
        occurrence("Course", allDay(MONDAY.add({ days: 2 })), allDay(MONDAY.add({ days: 4 }))),
      ],
      TZ,
      work,
    );
    expect(booked.get("2026-11-23")).toBe(120 + 90 + 60);
    expect(booked.get("2026-11-24")).toBe(60);
    expect(booked.get("2026-11-25")).toBe(600);
    expect(booked.get("2026-11-26")).toBe(600);
    expect(booked.has("2026-11-27")).toBe(false);
  });

  it("turns booked minutes into five levels", () => {
    expect([0, 30, 120, 121, 240, 300, 361, 900].map(heatLevel)).toEqual([0, 1, 1, 2, 2, 3, 4, 4]);
  });
});

describe("free time", () => {
  const start = Date.UTC(2026, 10, 23, 8);
  const end = Date.UTC(2026, 10, 23, 18);

  it("places busy blocks on the working day and names an all-day reason", () => {
    const meeting = occurrence("Meeting", at(MONDAY, 13), at(MONDAY, 15, 30));
    const early = occurrence("Early", at(MONDAY, 6), at(MONDAY, 9));
    const free = occurrence("Free", at(MONDAY, 10), at(MONDAY, 11), { transparency: "free" });
    const offsite = occurrence("Offsite", allDay(MONDAY), allDay(MONDAY.add({ days: 1 })));
    const blocks = busyBlocks([meeting, early, free, offsite], MONDAY, TZ, start, end);
    expect(blocks.map((b) => [b.instance.title, b.left, b.width])).toEqual([
      ["Meeting", 0.5, 0.25],
      ["Early", 0, 0.1],
    ]);
    expect(busyAllDay([meeting, offsite], MONDAY, TZ).map((i) => i.title)).toEqual(["Offsite"]);
  });

  it("adds up free minutes and spreads the hour axis", () => {
    expect(
      freeMinutes([
        { start: "2026-11-23T08:00:00Z", end: "2026-11-23T09:30:00Z" },
        { start: "2026-11-23T16:00:00Z", end: "2026-11-23T16:45:00Z" },
      ]),
    ).toBe(135);
    expect(hourTicks(8, 10)).toEqual([
      { hour: 8, at: 0 },
      { hour: 9, at: 0.5 },
      { hour: 10, at: 1 },
    ]);
  });
});

describe("empty ranges", () => {
  const before = occurrence(
    "Before",
    at(MONDAY.add({ days: -9 }), 9),
    at(MONDAY.add({ days: -9 }), 10),
  );
  const later = occurrence(
    "Later",
    at(MONDAY.add({ days: 30 }), 9),
    at(MONDAY.add({ days: 30 }), 10),
  );
  const sooner = occurrence(
    "Sooner",
    at(MONDAY.add({ days: 8 }), 9),
    at(MONDAY.add({ days: 8 }), 10),
  );

  it("names the next event after the range, else the last one before it", () => {
    expect(nearestOutside([before], [later, sooner])).toEqual({ instance: sooner, side: "after" });
    expect(nearestOutside([before], [])).toEqual({ instance: before, side: "before" });
    expect(nearestOutside([], [])).toBeNull();
  });

  it("looks a year to either side", () => {
    const { before: back, after: ahead } = yearsAround({ start: MONDAY, count: 7 });
    expect(back.start.toString()).toBe("2025-11-23");
    expect(back.start.add({ days: back.count }).toString()).toBe("2026-11-23");
    expect(ahead.start.toString()).toBe("2026-11-30");
    expect(ahead.count).toBe(365);
  });
});

describe("search groups", () => {
  const hit = (title: string, start: PimTime | null): PimSearchHit => ({
    item: { type: "event", event_id: title },
    title,
    detail: null,
    calendar_id: "cal",
    address_book_id: null,
    start,
  });

  it("puts today and later under upcoming (soonest first), the rest under past (latest first)", () => {
    const today = Date.UTC(2026, 10, 23);
    const groups = groupHits(
      [
        hit("Next week", at(MONDAY.add({ days: 7 }), 9)),
        hit("Today", at(MONDAY, 7)),
        hit("Last month", at(MONDAY.add({ days: -30 }), 9)),
        hit("Yesterday", allDay(MONDAY.add({ days: -1 }))),
        hit("No start", null),
      ],
      today,
      TZ,
    );
    expect(groups.upcoming.map((h) => h.title)).toEqual(["Today", "Next week"]);
    expect(groups.past.map((h) => h.title)).toEqual(["Yesterday", "Last month"]);
  });
});

describe("mini-month", () => {
  it("marks the range on screen as a band", () => {
    const week = { start: MONDAY, count: 7 };
    expect(bandOf(MONDAY, week)).toBe("start");
    expect(bandOf(MONDAY.add({ days: 3 }), week)).toBe("middle");
    expect(bandOf(MONDAY.add({ days: 6 }), week)).toBe("end");
    expect(bandOf(MONDAY.add({ days: 7 }), week)).toBeNull();
    expect(bandOf(MONDAY, { start: MONDAY, count: 1 })).toBe("single");
    expect(bandOf(MONDAY, null)).toBeNull();
  });

  it("gives each day the distinct colours of its events, at most three", () => {
    const calendars = [
      { id: "red", settings: { color: "#dc2626" } },
      { id: "blue", settings: { color: "#2563EB" } },
      { id: "none", settings: { color: null } },
    ] as unknown as Parameters<typeof dayDots>[1];
    const day = (title: string, cal: string, color: string | null = null) =>
      occurrence(title, at(MONDAY, 9), at(MONDAY, 10), { color }, cal);
    const dots = dayDots(
      [
        day("a", "red"),
        day("b", "red"),
        day("c", "blue"),
        day("d", "none"),
        day("e", "red", "#16a34a"),
      ],
      calendars,
      [MONDAY, MONDAY.add({ days: 1 })],
      TZ,
    );
    expect(dots.get("2026-11-23")).toEqual(["#dc2626", "#2563eb", ""]);
    expect(dots.has("2026-11-24")).toBe(false);
  });
});

describe("quick add", () => {
  const text = "Lunch with Bob tomorrow 12:30 at Corner café";
  const parsed = {
    title: "Lunch with Bob",
    start: at(MONDAY.add({ days: 1 }), 12, 30),
    end: at(MONDAY.add({ days: 1 }), 13, 30),
    location: "Corner café",
    recurrence: null,
    spans: [
      { kind: "date" as const, start: 15, end: 23 },
      { kind: "time" as const, start: 24, end: 29 },
      { kind: "location" as const, start: 30, end: 44 },
    ],
  };

  it("previews the title and each recognised part as typed", () => {
    expect(quickChips(text, parsed)).toEqual([
      { kind: "title", text: "Lunch with Bob" },
      { kind: "date", text: "tomorrow" },
      { kind: "time", text: "12:30" },
      { kind: "location", text: "at Corner café" },
    ]);
    // Spans outside the text are ignored.
    expect(
      quickChips("x", { ...parsed, title: "", spans: [{ kind: "date", start: 3, end: 9 }] }),
    ).toEqual([]);
  });

  it("takes the parsed times, else the day at nine for the default length", () => {
    expect(quickTimes(parsed, MONDAY, TZ)).toEqual({ start: parsed.start, end: parsed.end });
    const plain = { ...parsed, start: null, end: null };
    expect(quickTimes(plain, MONDAY, TZ)).toEqual({ start: at(MONDAY, 9), end: at(MONDAY, 10) });
    const fields = quickFields(text, { ...plain, title: "" }, MONDAY, TZ);
    expect(fields.title).toBe(text);
    expect(fields.location).toBe("Corner café");
  });
});

describe("calendar preferences form", () => {
  it("offers the day in half hours and checks the working hours", () => {
    const choices = workHourChoices();
    expect(choices[1]).toBe(WORK_HOUR_STEP);
    expect(choices.at(-1)).toBe(1440);
    const prefs = {
      work_hours: { start_minute: 480, end_minute: 1080 },
      work_days: [],
      week_start: null,
      show_week_numbers: false,
      time_zone: null,
      second_time_zone: null,
      show_declined: true,
    };
    expect(prefsValid(prefs)).toBe(true);
    expect(prefsValid({ ...prefs, work_hours: { start_minute: 600, end_minute: 600 } })).toBe(
      false,
    );
  });

  it("keeps work days in week order", () => {
    expect(toggleWeekday(["fri", "mon"], "wed", true)).toEqual(["mon", "wed", "fri"]);
    expect(toggleWeekday(["mon", "wed"], "mon", false)).toEqual(["wed"]);
  });
});
