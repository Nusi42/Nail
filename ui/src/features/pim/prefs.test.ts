import { describe, expect, it } from "vitest";
import { DEFAULT_CALENDAR_PREFERENCES } from "../../api/generated";
import type { CalendarPreferences } from "../../api/generated";
import {
  TASK_DRAG_TYPE,
  isWorkingTime,
  isoWeekNumber,
  offHours,
  setTaskDrag,
  taskOfDrag,
  viewZonesOf,
  weekdayOf,
  workHourRange,
  freeTimeHours,
  workHoursValid,
} from "./prefs";
import { localZone, parseDay } from "./time";
import { must } from "../../test/must";
import { describeVectors } from "../../test/vectors";
import calendarVectors from "../../../../crates/nail-api/fixtures/ts-vectors/calendar.json";

const day = (text: string) => must(parseDay(text));
const prefs: CalendarPreferences = {
  ...DEFAULT_CALENDAR_PREFERENCES,
  work_days: ["mon", "tue", "wed", "thu", "fri"],
};

describe("working time", () => {
  it("knows the weekday of a date", () => {
    expect(weekdayOf(day("2026-11-23"))).toBe("mon");
    expect(weekdayOf(day("2026-11-25"))).toBe("wed");
    expect(weekdayOf(day("2026-11-29"))).toBe("sun");
  });

  it("is inside the hours on a work day", () => {
    const wednesday = day("2026-11-25");
    expect(isWorkingTime(prefs, wednesday, 8 * 60)).toBe(true);
    expect(isWorkingTime(prefs, wednesday, 18 * 60 - 1)).toBe(true);
    expect(isWorkingTime(prefs, wednesday, 18 * 60)).toBe(false);
    expect(isWorkingTime(prefs, wednesday, 7 * 60 + 59)).toBe(false);
    expect(isWorkingTime(prefs, day("2026-11-28"), 10 * 60)).toBe(false);
  });

  it("shades before and after the hours, and the whole of a day off", () => {
    expect(offHours(prefs, day("2026-11-25"))).toEqual([
      [0, 480],
      [1080, 1440],
    ]);
    expect(offHours(prefs, day("2026-11-29"))).toEqual([[0, 1440]]);
    const allDay = { ...prefs, work_hours: { start_minute: 0, end_minute: 1440 } };
    expect(offHours(allDay, day("2026-11-25"))).toEqual([]);
  });
});

describe("working hour range", () => {
  it("covers the working hours in whole hours", () => {
    expect(workHourRange(prefs)).toEqual({ start: 8, end: 18 });
    expect(
      workHourRange({ ...prefs, work_hours: { start_minute: 570, end_minute: 1035 } }),
    ).toEqual({ start: 9, end: 18 });
  });
});

describe("ISO week numbers", () => {
  it.each([
    ["2026-11-25", 48],
    ["2026-01-01", 1],
    ["2027-01-01", 53],
    ["2024-12-30", 1],
    ["2021-01-03", 53],
    ["2026-12-31", 53],
  ])("%s is in week %i", (date, week) => {
    expect(isoWeekNumber(day(date))).toBe(week);
  });
});

describe("view zones", () => {
  it("takes the layout option, then the preference, then the system's zone", () => {
    expect(viewZonesOf(prefs)).toEqual({ zone: localZone(), second: null });
    const withZones = { ...prefs, time_zone: "Europe/Berlin", second_time_zone: "Asia/Tokyo" };
    expect(viewZonesOf(withZones)).toEqual({ zone: "Europe/Berlin", second: "Asia/Tokyo" });
    expect(viewZonesOf(withZones, { zone: "UTC", second: "America/New_York" })).toEqual({
      zone: "UTC",
      second: "America/New_York",
    });
    // Unknown zones are ignored; a second zone equal to the first is dropped.
    expect(
      viewZonesOf({ ...withZones, time_zone: "Atlantis/Lost_City" }, { second: "Asia/Tokyo" })
        .second,
    ).toBe("Asia/Tokyo");
    expect(viewZonesOf(withZones, { second: "Europe/Berlin" }).second).toBeNull();
  });
});

describe("task drag", () => {
  function transfer(): DataTransfer {
    const data = new Map<string, string>();
    return {
      setData: (type: string, value: string) => data.set(type, value),
      getData: (type: string) => data.get(type) ?? "",
      effectAllowed: "none",
    } as unknown as DataTransfer;
  }

  it("carries the task's id and title", () => {
    const t = transfer();
    setTaskDrag(t, { taskId: "tk-1", title: "Plan the trip" });
    expect(t.effectAllowed).toBe("copy");
    expect(taskOfDrag(t)).toEqual({ taskId: "tk-1", title: "Plan the trip" });
  });

  it("ignores drags without a task or with a malformed one", () => {
    expect(taskOfDrag(transfer())).toBeNull();
    const t = transfer();
    t.setData(TASK_DRAG_TYPE, "{not json");
    expect(taskOfDrag(t)).toBeNull();
    t.setData(TASK_DRAG_TYPE, JSON.stringify({ taskId: 3 }));
    expect(taskOfDrag(t)).toBeNull();
  });
});

// The working-hours rule is the core's `WorkHours::is_valid` (the form and the mock use this port).
describeVectors("calendar", calendarVectors, { workHoursValid });

describe("free time hours (R8, ADR 0251)", () => {
  const prefs: CalendarPreferences = {
    ...DEFAULT_CALENDAR_PREFERENCES,
    work_days: ["mon", "tue"],
    work_hours: { start_minute: 9 * 60, end_minute: 17 * 60 },
  };

  it("asks the core for the working hours of the preferences", () => {
    expect(freeTimeHours(prefs, null)).toEqual({ kind: "working-hours" });
  });

  it("asks for the layout's own hours on the preferences' working days", () => {
    expect(freeTimeHours(prefs, { start: 7, end: 12 })).toEqual({
      kind: "custom",
      work_hours: { start_minute: 420, end_minute: 720 },
      work_days: ["mon", "tue"],
    });
    // Never empty or past midnight.
    expect(freeTimeHours(prefs, { start: 10, end: 10 })).toMatchObject({
      work_hours: { start_minute: 600, end_minute: 660 },
    });
    expect(freeTimeHours(prefs, { start: 30, end: 40 })).toMatchObject({
      work_hours: { start_minute: 23 * 60, end_minute: 24 * 60 },
    });
  });
});
