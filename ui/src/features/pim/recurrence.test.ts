import { CalendarDate } from "@internationalized/date";
import type { PimTime, Recurrence, RecurrenceRule } from "../../api/generated";
import {
  choiceOf,
  defaultRule,
  describe as describeRule,
  recurrenceOf,
  ruleOf,
  simpleOf,
  untilFor,
  weekdayOf,
  weekdayOrdinal,
} from "./recurrence";

const START: PimTime = { type: "zoned", local: "2026-10-01T09:00:00", tz: "Europe/Berlin" };
const BASE: Omit<RecurrenceRule, "freq"> = {
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
};

describe("recurrence editor model", () => {
  it("knows weekdays and their ordinals", () => {
    expect(weekdayOf(new CalendarDate(2026, 10, 1))).toBe("thu");
    expect(weekdayOrdinal(new CalendarDate(2026, 10, 29))).toEqual({ ordinal: 5, last: true });
    expect(weekdayOrdinal(new CalendarDate(2026, 10, 8))).toEqual({ ordinal: 2, last: false });
  });

  it("round-trips the simple rules", () => {
    const weekly = {
      ...defaultRule("weekly", new CalendarDate(2026, 10, 1)),
      weekdays: ["mon", "thu"] as const,
    };
    const rule = ruleOf(
      { ...weekly, weekdays: [...weekly.weekdays], interval: 2, end: { type: "count", count: 5 } },
      START,
    );
    expect(rule).toMatchObject({
      freq: "weekly",
      interval: 2,
      end: { type: "count", count: 5 },
      by_day: [
        { day: "mon", ordinal: null },
        { day: "thu", ordinal: null },
      ],
    });
    expect(simpleOf(rule, START)).toEqual({
      freq: "weekly",
      interval: 2,
      weekdays: ["mon", "thu"],
      monthly: "day",
      end: { type: "count", count: 5 },
    });
    const monthly = ruleOf(
      { ...defaultRule("monthly", new CalendarDate(2026, 10, 1)), monthly: "weekday" },
      START,
    );
    expect(monthly.by_day).toEqual([{ day: "thu", ordinal: 1 }]);
    expect(simpleOf(monthly, START)?.monthly).toBe("weekday");
    const yearly = ruleOf(defaultRule("yearly", new CalendarDate(2026, 10, 1)), START);
    expect(yearly).toMatchObject({ by_month: [10], by_month_day: [1] });
    expect(simpleOf(yearly, START)?.freq).toBe("yearly");
  });

  it("keeps rules the simple editor cannot show", () => {
    const setPos: Recurrence = {
      rules: [
        { ...BASE, freq: "monthly", by_day: [{ day: "mon", ordinal: null }], by_set_pos: [-1] },
      ],
      dates: [],
      exceptions: [],
    };
    const choice = choiceOf(setPos, START);
    expect(choice.type).toBe("custom");
    expect(recurrenceOf(choice, START, setPos)).toBe(setPos);
    expect(choiceOf(null, START)).toEqual({ type: "none" });
    expect(choiceOf({ rules: [], dates: [START], exceptions: [] }, START).type).toBe("custom");
    expect(simpleOf({ ...BASE, freq: "hourly" }, START)).toBeNull();
  });

  it("keeps exceptions when the rule changes and drops everything for none", () => {
    const previous: Recurrence = {
      rules: [{ ...BASE, freq: "daily" }],
      dates: [],
      exceptions: [{ type: "zoned", local: "2026-10-03T09:00:00", tz: "Europe/Berlin" }],
    };
    const next = recurrenceOf(
      { type: "simple", rule: defaultRule("weekly", new CalendarDate(2026, 10, 1)) },
      START,
      previous,
    );
    expect(next?.exceptions).toEqual(previous.exceptions);
    expect(next?.rules[0]?.freq).toBe("weekly");
    expect(recurrenceOf({ type: "none" }, START, previous)).toBeNull();
  });

  it("writes UNTIL in the kind of the start", () => {
    const day = new CalendarDate(2026, 12, 31);
    expect(untilFor(day, { type: "date", date: "2026-10-01" })).toEqual({
      type: "date",
      date: "2026-12-31",
    });
    expect(untilFor(day, { type: "floating", local: "2026-10-01T09:00:00" })).toEqual({
      type: "floating",
      local: "2026-12-31T23:59:59",
    });
    // Zoned starts get UTC (RFC 5545): the end of the day in Berlin.
    expect(untilFor(day, START)).toEqual({
      type: "zoned",
      local: "2026-12-31T22:59:59",
      tz: "UTC",
    });
  });

  it("describes a rule with Fluent arguments", () => {
    const text = describeRule(
      {
        freq: "weekly",
        interval: 2,
        weekdays: ["thu", "mon"],
        monthly: "day",
        end: { type: "until", until: new CalendarDate(2026, 12, 31) },
      },
      START,
      (d) => d.toString(),
      (d) => d.toUpperCase(),
    );
    expect(text).toEqual({
      id: "pim-recurrence-summary",
      args: expect.objectContaining({
        freq: "weekly",
        interval: 2,
        days: "MON, THU",
        end: "until",
        until: "2026-12-31",
      }),
    });
  });
});
