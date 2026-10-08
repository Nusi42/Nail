import { describe, expect, it } from "vitest";
import type { AttendeeFreeBusy, BusyPeriod } from "../../api/generated";
import {
  answerCounts,
  busyDuring,
  canRespond,
  isMine,
  mergeBusy,
  placeSpan,
  responseOfStatus,
  splitAddresses,
  statusOfResponse,
  suggestSlots,
} from "./model";
import { SCHEDULING_BASE_CSS } from "./parts";
import { plausibleEmail } from "../../state/addresses";
import { firstStart } from "./AttendeePicker";

const H = 3_600_000;
const at = (h: number) => `2026-10-02T${String(h).padStart(2, "0")}:00:00Z`;
const period = (start: number, end: number, kind: BusyPeriod["kind"] = "busy"): BusyPeriod => ({
  start: at(start),
  end: at(end),
  kind,
});

describe("scheduling model", () => {
  it("maps answers and statuses both ways", () => {
    expect(statusOfResponse("accept")).toBe("accepted");
    expect(statusOfResponse("tentative")).toBe("tentative");
    expect(statusOfResponse("decline")).toBe("declined");
    expect(responseOfStatus("accepted")).toBe("accept");
    expect(responseOfStatus("needs-action")).toBeNull();
    expect(responseOfStatus(null)).toBeNull();
    expect(canRespond({ actions: ["open-event"] })).toBe(false);
    expect(canRespond({ actions: ["decline"] })).toBe(true);
  });

  it("merges like BusyPeriod::merge", () => {
    const merged = mergeBusy([
      period(9, 10),
      period(10, 11),
      period(10, 12, "tentative"),
      period(14, 13),
      period(8, 9, "unavailable"),
    ]);
    expect(merged).toEqual([
      period(8, 9, "unavailable"),
      period(9, 11),
      period(10, 12, "tentative"),
    ]);
  });

  it("suggests aligned free slots around busy time", () => {
    const window = { start: Date.parse(at(8)), end: Date.parse(at(12)) };
    const busy = [
      { start: Date.parse(at(8)), end: Date.parse(at(9)) + 10 * 60_000 },
      { start: Date.parse(at(10)), end: Date.parse(at(11)) },
    ];
    const slots = suggestSlots(busy, window, 30 * 60_000, 30 * 60_000, 5);
    expect(slots.map((s) => new Date(s.start).toISOString().slice(11, 16))).toEqual([
      "09:30",
      "11:00",
      "11:30",
    ]);
    expect(suggestSlots(busy, window, 30 * 60_000, 30 * 60_000, 1)).toHaveLength(1);
    expect(suggestSlots([], window, 5 * H, 30 * 60_000, 5)).toEqual([]);
    expect(suggestSlots([], window, 0, 30 * 60_000, 5)).toEqual([]);
  });

  it("places spans as clipped fractions", () => {
    const window = { start: 0, end: 100 };
    expect(placeSpan({ start: 25, end: 50 }, window)).toEqual({ left: 0.25, width: 0.25 });
    expect(placeSpan({ start: -10, end: 10 }, window)).toEqual({ left: 0, width: 0.1 });
    expect(placeSpan({ start: 100, end: 120 }, window)).toBeNull();
    expect(placeSpan({ start: 1, end: 2 }, { start: 5, end: 5 })).toBeNull();
  });

  it("finds who is busy during a span", () => {
    const rows: AttendeeFreeBusy[] = [
      { email: "a@x.example", source: { type: "own" }, periods: [period(9, 10)], error: null },
      { email: "b@x.example", source: { type: "unknown" }, periods: [], error: null },
    ];
    expect(busyDuring(rows, { start: Date.parse(at(9)), end: Date.parse(at(11)) })).toEqual([
      "a@x.example",
    ]);
    expect(busyDuring(rows, { start: Date.parse(at(10)), end: Date.parse(at(11)) })).toEqual([]);
  });

  it("checks and splits addresses", () => {
    expect(plausibleEmail("ada@example.org")).toBe(true);
    expect(plausibleEmail("ada@.org")).toBe(false);
    expect(plausibleEmail("ada example.org")).toBe(false);
    expect(splitAddresses(" a@x.example, b@y.example;c@z.example ")).toEqual([
      "a@x.example",
      "b@y.example",
      "c@z.example",
    ]);
    expect(isMine("Ada@Example.org", ["ada@example.org"])).toBe(true);
    expect(isMine(null, ["ada@example.org"])).toBe(false);
  });

  it("counts answers", () => {
    expect(answerCounts(["accepted", "accepted", "needs-action"])).toMatchObject({
      accepted: 2,
      "needs-action": 1,
      declined: 0,
    });
  });

  it("base CSS is safe, parses and positions the free/busy blocks", () => {
    expect(SCHEDULING_BASE_CSS).not.toMatch(/url\s*\(|@import|expression\s*\(|javascript:/i);
    const sheet = new CSSStyleSheet();
    const rules = SCHEDULING_BASE_CSS.split("\n").filter((line) => line.trim() !== "");
    for (const rule of rules) {
      expect(() => sheet.insertRule(rule, sheet.cssRules.length), rule).not.toThrow();
    }
    expect(SCHEDULING_BASE_CSS).toMatch(/\[data-part="freebusy.track"\] \{ position: relative;/);
  });

  it("starts finding a time at the next hour that still fits, else tomorrow", () => {
    const tz = "UTC";
    const morning = Date.parse("2026-10-01T07:20:00Z");
    expect(firstStart(tz, 8, 18, 30, morning)).toEqual({
      type: "zoned",
      local: "2026-10-01T08:00:00",
      tz,
    });
    expect(firstStart(tz, 8, 18, 30, Date.parse("2026-10-01T13:05:00Z"))).toMatchObject({
      local: "2026-10-01T14:00:00",
    });
    expect(firstStart(tz, 8, 18, 60, Date.parse("2026-10-01T17:10:00Z"))).toMatchObject({
      local: "2026-10-02T08:00:00",
    });
  });
});
