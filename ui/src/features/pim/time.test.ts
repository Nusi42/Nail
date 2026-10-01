import { CalendarDate, fromAbsolute } from "@internationalized/date";
import type { EventInstance, PimTime } from "../../api/generated";
import {
  busyDays,
  dateString,
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

const BERLIN = "Europe/Berlin";

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

  it("puts overlapping events side by side", () => {
    const placed = placeDay(
      [
        instance("a", "2026-10-01T09:00:00Z", "2026-10-01T10:00:00Z"),
        instance("b", "2026-10-01T09:30:00Z", "2026-10-01T10:30:00Z"),
        instance("c", "2026-10-01T11:00:00Z", "2026-10-01T11:05:00Z"),
      ],
      day,
      "UTC",
    );
    const byId = Object.fromEntries(placed.map((p) => [p.instance.event_id, p]));
    expect(byId.a).toMatchObject({ column: 0, columns: 2, startMinute: 540, endMinute: 600 });
    expect(byId.b).toMatchObject({ column: 1, columns: 2 });
    // Short events get the minimum height; separate clusters get one column.
    expect(byId.c).toMatchObject({ columns: 1, startMinute: 660, endMinute: 675 });
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
