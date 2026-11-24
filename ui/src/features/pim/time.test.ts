import { CalendarDate, fromAbsolute } from "@internationalized/date";
import type { EventInstance, PimTime } from "../../api/generated";
import {
  CASCADE_EXPOSED_REM,
  CASCADE_INDENT,
  CASCADE_MIN_WIDTH,
  SIDE_BY_SIDE_MINUTES,
  nowLabelCovers,
  sideBySideMinutes,
  SHORT_EVENT_MINUTES,
  clickRange,
  dragRange,
  minuteAt,
  snapDown,
  visibleInstances,
  dayGridMove,
  likeAt,
  initialScrollTop,
  DEFAULT_VIEW_OPTIONS,
  busyDays,
  dateString,
  freeStretchesOfDay,
  workingDaysFrom,
  workingDaysOf,
  instancesOfDay,
  isAllDayLike,
  makePimTime,
  monthGrid,
  pimTimeIn,
  placeDay,
  placeSpanning,
  rangeInstants,
  sameKindAs,
  sameTime,
  shiftPimTime,
  splitPimTime,
  stepAnchor,
  utcString,
  viewRange,
  weeksOf,
} from "./time";
import { BERLIN } from "../../test/mockIds";

function instance(
  id: string,
  start: string,
  end: string,
  extra: Partial<EventInstance> = {},
): EventInstance {
  return {
    event_id: id,
    calendar_id: "c",
    recurrence_id: null,
    start: { type: "zoned", local: start.slice(0, 19), tz: "UTC" },
    end: { type: "zoned", local: end.slice(0, 19), tz: "UTC" },
    start_utc: start,
    end_utc: end,
    all_day: false,
    title: id,
    location: null,
    status: "confirmed",
    transparency: "busy",
    color: null,
    recurring: false,
    overridden: false,
    has_attendees: false,
    has_alarms: false,
    read_only: false,
    my_status: null,
    ...extra,
  };
}

describe("view ranges", () => {
  const thursday = new CalendarDate(2026, 10, 1);

  it("start weeks as the UI locale says", () => {
    expect(viewRange("week", thursday, "de").start.toString()).toBe("2026-09-28");
    expect(viewRange("week", thursday, "en-US").start.toString()).toBe("2026-09-27");
    expect(viewRange("week", thursday, "de").count).toBe(7);
  });

  it("covers day, multi-day, work week, agenda, month and year", () => {
    expect(viewRange("day", thursday, "de")).toEqual({ start: thursday, count: 1 });
    expect(
      viewRange("multi-day", thursday, "de", { days: 4, workWeekStart: "mon", workWeekDays: 5 })
        .count,
    ).toBe(4);
    const work = viewRange("work-week", thursday, "en-US");
    expect(work.start.toString()).toBe("2026-09-28");
    expect(work.count).toBe(5);
    const month = viewRange("month", thursday, "de");
    expect(month.start.toString()).toBe("2026-09-28");
    expect(month.count % 7).toBe(0);
    expect(weeksOf(month)).toHaveLength(month.count / 7);
    expect(viewRange("year", thursday, "de")).toEqual({
      start: new CalendarDate(2026, 1, 1),
      count: 365,
    });
    expect(monthGrid(new CalendarDate(2026, 2, 14), "de")[0]?.[0]?.toString()).toBe("2026-01-26");
  });

  it("steps by the view's page", () => {
    expect(stepAnchor("day", thursday, 1).toString()).toBe("2026-10-02");
    expect(stepAnchor("week", thursday, -1).toString()).toBe("2026-09-24");
    expect(stepAnchor("month", thursday, 1).toString()).toBe("2026-11-01");
    expect(stepAnchor("year", thursday, -1).toString()).toBe("2025-10-01");
    expect(
      stepAnchor("multi-day", thursday, 1, {
        days: 3,
        workWeekStart: "mon",
        workWeekDays: 5,
      }).toString(),
    ).toBe("2026-10-04");
  });

  it("turns a range into instants in the viewer's zone", () => {
    expect(rangeInstants({ start: thursday, count: 1 }, BERLIN)).toEqual({
      from: "2026-09-30T22:00:00Z",
      to: "2026-10-01T22:00:00Z",
    });
  });
});

describe("times", () => {
  it("resolves dates, floating and zoned times in the viewer's zone", () => {
    expect(pimTimeIn({ type: "date", date: "2026-10-01" }, BERLIN).toAbsoluteString()).toBe(
      "2026-09-30T22:00:00.000Z",
    );
    expect(
      pimTimeIn({ type: "floating", local: "2026-10-01T09:00:00" }, "America/New_York").hour,
    ).toBe(9);
    const ny = pimTimeIn(
      { type: "zoned", local: "2026-10-01T10:00:00", tz: "America/New_York" },
      BERLIN,
    );
    expect(ny.hour).toBe(16);
  });

  it("writes times back in their own kind and zone", () => {
    const at = fromAbsolute(Date.UTC(2026, 9, 1, 14, 0), BERLIN);
    expect(sameKindAs({ type: "zoned", local: "x", tz: "America/New_York" }, at)).toEqual({
      type: "zoned",
      local: "2026-10-01T10:00:00",
      tz: "America/New_York",
    });
    expect(sameKindAs({ type: "floating", local: "x" }, at)).toEqual({
      type: "floating",
      local: "2026-10-01T16:00:00",
    });
    expect(sameKindAs({ type: "date", date: "x" }, at)).toEqual({
      type: "date",
      date: "2026-10-01",
    });
  });

  it("shifts by wall-clock time and compares moments", () => {
    const zoned: PimTime = { type: "zoned", local: "2026-10-24T09:00:00", tz: BERLIN };
    // Across the end of summer time: still 09:00 local.
    expect(shiftPimTime(zoned, 1, 0)).toEqual({ ...zoned, local: "2026-10-25T09:00:00" });
    expect(shiftPimTime({ type: "date", date: "2026-12-31" }, 1, 0)).toEqual({
      type: "date",
      date: "2027-01-01",
    });
    expect(
      sameTime(
        { type: "zoned", local: "2026-10-01T09:00:00", tz: BERLIN },
        { type: "zoned", local: "2026-10-01T07:00:00", tz: "UTC" },
      ),
    ).toBe(true);
    expect(
      sameTime(
        { type: "date", date: "2026-10-01" },
        { type: "floating", local: "2026-10-01T00:00:00" },
      ),
    ).toBe(false);
    expect(utcString(Date.UTC(2026, 0, 2, 3, 4, 5))).toBe("2026-01-02T03:04:05Z");
    expect(makePimTime(new CalendarDate(2026, 1, 2), { hour: 3, minute: 4 }, null)).toEqual({
      type: "floating",
      local: "2026-01-02T03:04:00",
    });
    expect(splitPimTime({ type: "zoned", local: "2026-01-02T03:04:00", tz: BERLIN })).toMatchObject(
      {
        time: { hour: 3, minute: 4 },
        tz: BERLIN,
      },
    );
  });
});

describe("placing events", () => {
  const day = new CalendarDate(2026, 10, 1);

  const at = (hm: string) => `2026-10-01T${hm}:00Z`;
  const layout = (events: [string, string, string][]) =>
    Object.fromEntries(
      placeDay(
        events.map(([id, start, end]) => instance(id, at(start), at(end))),
        day,
        "UTC",
      ).map((p) => [p.instance.event_id, p]),
    );

  it("cascades later overlapping events, indented and on top", () => {
    // The review's Wednesday: 9:00, 9:30 and 9:45, each overlapping the one before.
    const byId = layout([
      ["sync", "09:00", "10:00"],
      ["grace", "09:30", "10:15"],
      ["critique", "09:45", "11:00"],
      ["lunch", "12:00", "12:05"],
    ]);
    expect(byId.sync).toMatchObject({ layer: 0, left: 0, width: 1, startMinute: 540 });
    expect(byId.grace).toMatchObject({ layer: 1, left: CASCADE_INDENT });
    expect(byId.critique).toMatchObject({ layer: 2 });
    expect(byId.critique?.left).toBeCloseTo(2 * CASCADE_INDENT);
    // Every title keeps at least 60 % of the column.
    for (const p of Object.values(byId)) {
      expect(p.width).toBeGreaterThanOrEqual(CASCADE_MIN_WIDTH - 1e-9);
      expect(p.left + p.width).toBeCloseTo(1);
    }
    // Short events get the minimum height; a separate cluster takes the whole column.
    expect(byId.lunch).toMatchObject({ layer: 0, left: 0, width: 1, endMinute: 735 });
  });

  it("cascades only when one title line of the earlier card stays visible", () => {
    // 48 px per hour, 16 px per rem: a title line (1.5 rem = 24 px) is 30 minutes.
    expect(CASCADE_EXPOSED_REM).toBe(1.5);
    const together = sideBySideMinutes(48 / 60, 16);
    expect(together).toBe(30);
    // A tall grid keeps the 15-minute floor; an unmeasured one falls back to it.
    expect(sideBySideMinutes(4, 16)).toBe(SIDE_BY_SIDE_MINUTES);
    expect(sideBySideMinutes(0, 16)).toBe(SIDE_BY_SIDE_MINUTES);
    const byId = Object.fromEntries(
      placeDay(
        [
          instance("sync", at("09:00"), at("10:00")),
          instance("grace", at("09:30"), at("10:15")),
          instance("critique", at("09:45"), at("11:00")),
        ],
        day,
        "UTC",
        together,
      ).map((p) => [p.instance.event_id, p]),
    );
    // 9:30 leaves 30 minutes (one line) of Team sync visible: it cascades.
    expect(byId.sync).toMatchObject({ layer: 0 });
    expect(byId.grace).toMatchObject({ layer: 1, left: CASCADE_INDENT });
    // 9:45 would leave only 15 minutes of 1:1 with Grace: it stands beside it instead.
    expect(byId.critique).toMatchObject({ layer: 1 });
    expect(byId.critique?.left).toBeGreaterThan(byId.grace?.left ?? 1);
    expect(byId.grace?.width).toBeCloseTo((1 - CASCADE_INDENT) / 2);
  });

  it("indents less when there are many levels, so the top card keeps 60 %", () => {
    const byId = layout([
      ["a", "08:00", "13:00"],
      ["b", "08:30", "13:00"],
      ["c", "09:00", "13:00"],
      ["d", "09:30", "13:00"],
      ["e", "10:00", "13:00"],
    ]);
    expect(byId.e?.layer).toBe(4);
    expect(byId.e?.left).toBeCloseTo(1 - CASCADE_MIN_WIDTH);
    expect(byId.e?.width).toBeCloseTo(CASCADE_MIN_WIDTH);
    expect(byId.b?.left).toBeCloseTo((1 - CASCADE_MIN_WIDTH) / 4);
  });

  it("puts events that start together side by side and expands into free columns", () => {
    const byId = layout([
      ["dentist", "15:00", "16:00"],
      ["kickoff", "15:00", "16:00"],
      ["call", "15:10", "15:45"],
      ["late", "16:00", "17:00"],
    ]);
    expect(SIDE_BY_SIDE_MINUTES).toBe(15);
    expect(byId.dentist).toMatchObject({ layer: 0, left: 0 });
    expect(byId.dentist?.width).toBeCloseTo(1 / 3);
    expect(byId.kickoff?.left).toBeCloseTo(1 / 3);
    expect(byId.call?.left).toBeCloseTo(2 / 3);
    // "late" starts as the others end: its own cluster, the whole column.
    expect(byId.late).toMatchObject({ left: 0, width: 1 });
    // An event beside one that ends early takes the room it leaves.
    const wide = layout([
      ["long", "09:00", "11:00"],
      ["brief", "09:10", "09:40"],
      ["after", "09:50", "10:30"],
    ]);
    expect(wide.long?.width).toBeCloseTo(0.5);
    expect(wide.brief).toMatchObject({ layer: 0 });
    expect(wide.brief?.left).toBeCloseTo(0.5);
    // "after" starts 50 minutes after "long": it cascades over it.
    expect(wide.after).toMatchObject({ layer: 1, left: CASCADE_INDENT });
  });

  it("marks events under 45 minutes as short", () => {
    const byId = layout([
      ["quarter", "08:00", "08:15"],
      ["half", "09:00", "09:30"],
      ["almost", "10:00", "10:44"],
      ["exact", "11:00", "11:45"],
    ]);
    expect(SHORT_EVENT_MINUTES).toBe(45);
    expect(byId.quarter?.short).toBe(true);
    expect(byId.half?.short).toBe(true);
    expect(byId.almost?.short).toBe(true);
    expect(byId.exact?.short).toBe(false);
  });

  it("hides declined invitations only when the preference says so", () => {
    const list = [
      instance("yes", at("09:00"), at("10:00"), { my_status: "accepted" }),
      instance("no", at("09:00"), at("10:00"), { my_status: "declined" }),
    ];
    expect(visibleInstances(list, true)).toHaveLength(2);
    expect(visibleInstances(list, false).map((i) => i.event_id)).toEqual(["yes"]);
  });

  it("clips events running over midnight and uses wall-clock minutes on DST days", () => {
    const night = placeDay(
      [instance("n", "2026-09-30T22:00:00Z", "2026-10-01T02:00:00Z")],
      day,
      "UTC",
    )[0];
    expect(night).toMatchObject({
      startMinute: 0,
      endMinute: 120,
      continuesBefore: true,
      continuesAfter: false,
    });
    const dst = new CalendarDate(2026, 10, 25);
    const placed = placeDay(
      [instance("d", "2026-10-25T09:00:00Z", "2026-10-25T10:00:00Z")],
      dst,
      BERLIN,
    )[0];
    // 09:00 UTC is 10:00 in Berlin after the change.
    expect(placed).toMatchObject({ startMinute: 600, endMinute: 660, dayMinutes: 1440 });
  });

  it("lays out all-day and long events across days", () => {
    const days = [0, 1, 2, 3].map((i) => day.add({ days: i }));
    const trip = instance("trip", "2026-10-01T22:00:00Z", "2026-10-03T22:00:00Z", {
      all_day: true,
    });
    const span = placeSpanning(
      [trip, instance("short", "2026-10-01T09:00:00Z", "2026-10-01T10:00:00Z")],
      days,
      BERLIN,
    );
    expect(span).toHaveLength(1);
    expect(span[0]).toMatchObject({ first: 1, last: 2, row: 0 });
    expect(isAllDayLike(trip)).toBe(true);
    expect(instancesOfDay([trip], day.add({ days: 1 }), BERLIN)).toHaveLength(1);
    expect([...busyDays([trip], BERLIN)]).toEqual(["2026-10-02", "2026-10-03"]);
    expect(dateString(day)).toBe("2026-10-01");
  });
});

describe("free time (M5b review)", () => {
  it("starts at the anchor's first working day and skips weekends", () => {
    // Saturday 3 October 2026, en-US: Monday 5 – Friday 9.
    const saturday = new CalendarDate(2026, 10, 3);
    expect(viewRange("free-time", saturday, "en-US", { ...DEFAULT_VIEW_OPTIONS, days: 5 })).toEqual(
      {
        start: new CalendarDate(2026, 10, 5),
        count: 5,
      },
    );
    // Wednesday: Wed, Thu, Fri, (Sat, Sun), Mon, Tue.
    const range = workingDaysFrom(new CalendarDate(2026, 10, 7), "en-US", 5);
    expect(range).toEqual({ start: new CalendarDate(2026, 10, 7), count: 7 });
    expect(workingDaysOf(range, "en-US").map(dateString)).toEqual([
      "2026-10-07",
      "2026-10-08",
      "2026-10-09",
      "2026-10-12",
      "2026-10-13",
    ]);
    // The weekend is the locale's: Friday and Saturday in Israel.
    expect(workingDaysFrom(new CalendarDate(2026, 10, 2), "he-IL", 1).start).toEqual(
      new CalendarDate(2026, 10, 4),
    );
  });

  it("cuts a free stretch spanning several days to each day's working hours", () => {
    // One free stretch from Monday noon to Thursday 10:00 (UTC), as
    // FindFreeTime returns it for a calendar free for days.
    const slots = [{ start: "2026-10-05T12:00:00Z", end: "2026-10-08T10:00:00Z" }];
    const day = (d: number) =>
      freeStretchesOfDay(slots, new CalendarDate(2026, 10, d), "UTC", 8, 18, 30);
    expect(day(5)).toEqual([{ start: "2026-10-05T12:00:00Z", end: "2026-10-05T18:00:00Z" }]);
    expect(day(6)).toEqual([{ start: "2026-10-06T08:00:00Z", end: "2026-10-06T18:00:00Z" }]);
    expect(day(7)).toEqual([{ start: "2026-10-07T08:00:00Z", end: "2026-10-07T18:00:00Z" }]);
    expect(day(8)).toEqual([{ start: "2026-10-08T08:00:00Z", end: "2026-10-08T10:00:00Z" }]);
    expect(day(9)).toEqual([]);
    // Working hours in the view's zone; pieces shorter than the duration go.
    expect(
      freeStretchesOfDay(slots, new CalendarDate(2026, 10, 8), "Europe/Berlin", 8, 18, 30),
    ).toEqual([{ start: "2026-10-08T06:00:00Z", end: "2026-10-08T10:00:00Z" }]);
    expect(freeStretchesOfDay(slots, new CalendarDate(2026, 10, 8), "UTC", 9, 18, 90)).toEqual([]);
  });
});

describe("initial scroll of the time grid", () => {
  it("starts at the hour, or on the last whole hour the grid can scroll to", () => {
    // 24 rows of 48 px; a short pane can scroll to 8 AM.
    expect(initialScrollTop(8, 48, 24 * 48 - 400)).toBe(8 * 48);
    // A tall pane (max 330 px) stops at 6 AM, not mid-way through 7 AM's label.
    expect(initialScrollTop(8, 48, 330)).toBe(6 * 48);
    expect(initialScrollTop(8, 48, 0)).toBe(0);
    expect(initialScrollTop(-2, 48, 500)).toBe(0);
    expect(initialScrollTop(8, 0, 500)).toBe(0);
  });
});

describe("day grid keys", () => {
  const day = new CalendarDate(2026, 10, 8); // a Thursday
  it("moves by days, weeks and to the ends of the week", () => {
    expect(dayGridMove("ArrowLeft", day, "en-US")?.toString()).toBe("2026-10-07");
    expect(dayGridMove("ArrowRight", day, "en-US")?.toString()).toBe("2026-10-09");
    expect(dayGridMove("ArrowUp", day, "en-US")?.toString()).toBe("2026-10-01");
    expect(dayGridMove("ArrowDown", day, "en-US")?.toString()).toBe("2026-10-15");
    expect(dayGridMove("Home", day, "en-US")?.toString()).toBe("2026-10-04");
    expect(dayGridMove("End", day, "en-US")?.toString()).toBe("2026-10-10");
    expect(dayGridMove("Home", day, "de-DE")?.toString()).toBe("2026-10-05");
  });

  it("pages by months (Shift: years) only where asked", () => {
    expect(dayGridMove("PageUp", day, "en-US")).toBeNull();
    expect(dayGridMove("PageDown", day, "en-US", { pages: true })?.toString()).toBe("2026-11-08");
    expect(dayGridMove("PageUp", day, "en-US", { pages: true, shift: true })?.toString()).toBe(
      "2025-10-08",
    );
    expect(dayGridMove("x", day, "en-US", { pages: true })).toBeNull();
  });
});

describe("likeAt", () => {
  const wall = fromAbsolute(Date.UTC(2026, 9, 8, 7, 30), "UTC");
  it("keeps the kind and zone of the model time", () => {
    expect(likeAt({ type: "date", date: "2026-01-01" }, wall)).toEqual({
      type: "date",
      date: "2026-10-08",
    });
    expect(likeAt({ type: "floating", local: "2026-01-01T00:00:00" }, wall)).toEqual({
      type: "floating",
      local: "2026-10-08T07:30:00",
    });
    expect(
      likeAt({ type: "zoned", local: "2026-01-01T00:00:00", tz: "Europe/Berlin" }, wall),
    ).toEqual({ type: "zoned", local: "2026-10-08T07:30:00", tz: "Europe/Berlin" });
  });
});

describe("drawing on the time grid", () => {
  it("snaps a drag to 15-minute steps in either direction", () => {
    // Pressed at 10:07, dragged to 11:23: 10:00 to 11:30 (snapped to the nearest step).
    expect(dragRange(607, 683, 0, 1440)).toEqual({ start: 600, end: 690 });
    // Upwards from 10:07 to 9:20: 9:15 to 10:15 (the step pressed stays in the range).
    expect(dragRange(607, 560, 0, 1440)).toEqual({ start: 555, end: 615 });
    // Barely moved: at least one step.
    expect(dragRange(607, 609, 0, 1440)).toEqual({ start: 600, end: 615 });
    // Kept inside the hours shown.
    expect(dragRange(1430, 1500, 0, 1440)).toEqual({ start: 1425, end: 1440 });
    expect(dragRange(70, -40, 60, 1440)).toEqual({ start: 60, end: 75 });
  });

  it("snaps a click to the 15-minute slot under the pointer", () => {
    expect(snapDown(607)).toBe(600);
    expect(snapDown(614.9)).toBe(600);
    expect(snapDown(615)).toBe(615);
    expect(clickRange(637, 60, 0, 1440)).toEqual({ start: 630, end: 690 });
    // Near the end of the day the event is cut at the last hour shown.
    expect(clickRange(1435, 60, 0, 1440)).toEqual({ start: 1425, end: 1440 });
  });

  it("maps pixels to minutes of the hours shown", () => {
    expect(minuteAt(0, 480, 480, 960)).toBe(480);
    expect(minuteAt(240, 480, 480, 960)).toBe(720);
    expect(minuteAt(-10, 480, 480, 960)).toBe(480);
    expect(minuteAt(999, 480, 480, 960)).toBe(960);
    expect(minuteAt(10, 0, 480, 960)).toBe(480);
  });
});

describe("week start preference", () => {
  it("starts week and month views on the preferred weekday", () => {
    const anchor = new CalendarDate(2026, 11, 25); // a Wednesday
    expect(dateString(viewRange("week", anchor, "en-US").start)).toBe("2026-11-22");
    expect(
      dateString(
        viewRange("week", anchor, "en-US", { ...DEFAULT_VIEW_OPTIONS, weekStart: "mon" }).start,
      ),
    ).toBe("2026-11-23");
    expect(
      dateString(
        viewRange("week", anchor, "de", { ...DEFAULT_VIEW_OPTIONS, weekStart: "sat" }).start,
      ),
    ).toBe("2026-11-21");
    expect(
      dateString(
        viewRange("month", anchor, "en-US", { ...DEFAULT_VIEW_OPTIONS, weekStart: "mon" }).start,
      ),
    ).toBe("2026-10-26");
  });
});

describe("nowLabelCovers", () => {
  it("hides the label of the hour now is in, and of an hour starting or ending close by", () => {
    const at = (h: number, m: number) => h * 60 + m;
    // 8:59: the 8 AM row holds the label, 9 AM starts a minute later.
    expect([7, 8, 9, 10].filter((h) => nowLabelCovers(h, at(8, 59)))).toEqual([8, 9]);
    // 8:30: only 8 AM.
    expect([7, 8, 9].filter((h) => nowLabelCovers(h, at(8, 30)))).toEqual([8]);
    // 9:05: 9 AM, and 8 AM whose row ended five minutes ago.
    expect([7, 8, 9, 10].filter((h) => nowLabelCovers(h, at(9, 5)))).toEqual([8, 9]);
  });
});
