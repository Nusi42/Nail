/**
 * The pure rules of the calendar's views and shell (R7, ADR 0246): how many
 * days a narrow time grid shows, whether the window's layout has the
 * calendar sidebar, what the agenda lists per day, how the month view lays
 * out spanning bars and lines, how busy a day of the year view is, the
 * free-time view's busy blocks and totals, the nearest event outside an
 * empty range, the search results by upcoming / past, the mini-month's
 * band and dots, and the quick-add preview's chips. The components only
 * render what these return.
 */
import type {
  Calendar,
  EventInstance,
  LayoutNode,
  ParsedEventText,
  ParsedPart,
  PimSearchHit,
  ResolvedLayout,
} from "../../api/generated";
import { MINUTES_PER_DAY } from "../../api/generated";
import { selectBreakpoint } from "../../layout/engine";
import { isCssColor } from "./colors";
import { colorOf } from "./common";
import { TIME_GUTTER } from "./parts";
import {
  dateString,
  daysOf,
  instancesOfDay,
  instantIn,
  instantMs,
  isAllDayLike,
  overlapsDay,
  pimTimeIn,
  placeSpanning,
  toCalendarDate,
  toZoned,
  type CalendarDate,
  type SpanningInstance,
  type ViewKind,
  type ViewRange,
} from "./time";

/** The time of day a new event starts at when nothing else says (New event, quick add). */
export const DEFAULT_NEW_EVENT_TIME = { hour: 9, minute: 0 } as const;

// ---------------------------------------------------------------------------
// Narrow windows (M7)

/** A day column of the time grid is never narrower than this (CSS pixels). */
export const MIN_DAY_COLUMN_PX = 90;

/** The day counts a time grid falls back to when its columns get too narrow. */
export const FALLBACK_DAY_COUNTS: readonly number[] = [3, 1];

/** The views drawn as a time grid (day columns). */
export const TIME_GRID_VIEWS: readonly ViewKind[] = ["day", "multi-day", "week", "work-week"];

/** The time grid's hour gutter in pixels (one per zone shown). */
export function gutterPx(zones: number): number {
  return Number.parseFloat(TIME_GUTTER) * Math.max(1, zones);
}

/**
 * How many of `count` day columns fit `width` pixels (the gutter taken
 * off) at [`MIN_DAY_COLUMN_PX`] each: `count`, else three, else one.
 * An unknown width (`<= 0`, nothing measured yet) keeps `count`.
 */
export function fittingDayCount(count: number, width: number, gutter: number): number {
  if (width <= 0) return count;
  const room = width - gutter;
  for (const n of [count, ...FALLBACK_DAY_COUNTS.filter((n) => n < count)]) {
    if (room / n >= MIN_DAY_COLUMN_PX) return n;
  }
  return 1;
}

/**
 * The view a time grid shows in `width` pixels: the chosen one while its
 * `count` columns fit, else `multi-day` with three days, else `day`;
 * `null` = no fallback (the chosen view as it is).
 */
export function narrowFallback(
  view: ViewKind,
  count: number,
  width: number,
  zones: number,
): { view: ViewKind; days: number } | null {
  if (!TIME_GRID_VIEWS.includes(view)) return null;
  const days = fittingDayCount(count, width, gutterPx(zones));
  if (days >= count) return null;
  return days === 1 ? { view: "day", days: 1 } : { view: "multi-day", days };
}

/** Widget kinds of the calendar sidebar (the drawer shows them when the layout does not). */
export const SIDEBAR_KINDS: readonly string[] = ["mini-month", "calendar-list"];

function walk(node: LayoutNode, visit: (node: LayoutNode) => boolean): boolean {
  if (node.hidden === true) return false;
  if (visit(node)) return true;
  return node.type !== "widget" && node.children.some((child) => walk(child, visit));
}

function holds(root: LayoutNode, target: LayoutNode): boolean {
  return walk(
    root,
    (n) =>
      n === target ||
      (target.id !== null &&
        n.type === "widget" &&
        target.type === "widget" &&
        n.kind === target.kind &&
        n.id === target.id),
  );
}

/**
 * Whether the window that shows `node` at `width` (its breakpoint for that
 * width) also shows the calendar sidebar (a mini-month or calendar list).
 * A node found in no window counts as having one (nothing to replace).
 */
export function layoutShowsSidebar(
  layout: Pick<ResolvedLayout, "windows"> | null,
  node: LayoutNode,
  width: number,
): boolean {
  if (layout === null) return true;
  for (const breakpoints of Object.values(layout.windows)) {
    const root = selectBreakpoint(breakpoints, width)?.root;
    if (!root || !holds(root, node)) continue;
    return walk(root, (n) => n.type === "widget" && SIDEBAR_KINDS.includes(n.kind));
  }
  return true;
}

// ---------------------------------------------------------------------------
// Agenda (S7)

/** How an agenda row shows its time. */
export type AgendaTime =
  /** All day (or a day inside a longer event). */
  | "all-day"
  /** Its start (and end) on this day. */
  | "timed"
  /** The rest of an event that began the day before: "until <end>". */
  | "until";

export interface AgendaRow {
  instance: EventInstance;
  time: AgendaTime;
  /** It ended before now. */
  past: boolean;
}

export interface AgendaDay {
  day: CalendarDate;
  /** `today` / `tomorrow` (labelled so), else `null`. */
  relative: "today" | "tomorrow" | null;
  rows: AgendaRow[];
  /** Today: the row before which the now marker goes (`rows.length` = after the last); else `null`. */
  nowAt: number | null;
}

/** The agenda's days with occurrences in `range` (see [`AgendaRow`]). */
export function agendaDays(
  instances: readonly EventInstance[],
  range: ViewRange,
  tz: string,
  today: CalendarDate,
  now: number,
): AgendaDay[] {
  return daysOf(range).flatMap((day) => {
    const dayStart = toZoned(day, tz).toDate().getTime();
    const dayEnd = toZoned(day.add({ days: 1 }), tz)
      .toDate()
      .getTime();
    const rows = instancesOfDay(instances, day, tz).map((instance): AgendaRow => {
      const start = instantMs(instance.start_utc);
      const end = instantMs(instance.end_utc);
      const time: AgendaTime =
        isAllDayLike(instance) || (start < dayStart && end > dayEnd)
          ? "all-day"
          : start < dayStart
            ? "until"
            : "timed";
      return { instance, time, past: end <= now };
    });
    if (rows.length === 0) return [];
    const isToday = day.compare(today) === 0;
    const relative = isToday
      ? "today"
      : day.compare(today.add({ days: 1 })) === 0
        ? "tomorrow"
        : null;
    let nowAt: number | null = null;
    if (isToday) {
      const next = rows.findIndex(
        (row) => row.time === "timed" && instantMs(row.instance.start_utc) > now,
      );
      nowAt = next < 0 ? rows.length : next;
    }
    return [{ day, relative, rows, nowAt }];
  });
}

// ---------------------------------------------------------------------------
// Month (S8)

/** One lane of a month cell: a bar starting here, a lane a bar passes through, or nothing. */
export type MonthLane = { kind: "bar"; span: SpanningInstance; days: number } | { kind: "gap" };

export interface MonthCell {
  day: CalendarDate;
  /** The spanning bars' lanes shown in this cell (top first). */
  lanes: MonthLane[];
  /** Timed occurrences shown as lines. */
  lines: EventInstance[];
  /** What did not fit ("+N more"). */
  more: number;
  /** Every occurrence touching the day. */
  total: number;
}

/**
 * One week row of the month view: occurrences of a day or more as bars
 * across their days (one bar per week row, packed into lanes), timed ones
 * as lines on their start day, at most `maxPerDay` entries per cell (lanes
 * a bar passes count); the rest is "+N more".
 */
export function monthWeek(
  week: readonly CalendarDate[],
  instances: readonly EventInstance[],
  tz: string,
  maxPerDay: number,
): MonthCell[] {
  const spans = placeSpanning(instances, week, tz);
  return week.map((day, index) => {
    const covering = spans.filter((s) => s.first <= index && s.last >= index);
    const lastLane = covering.reduce((max, s) => Math.max(max, s.row), -1);
    const shownLanes = Math.min(lastLane + 1, maxPerDay);
    const lanes = Array.from({ length: shownLanes }, (_, row): MonthLane => {
      const span = covering.find((s) => s.row === row);
      if (!span) return { kind: "gap" };
      // A bar starts in its first day of the week, or in the first day it is still shown.
      return span.first === index
        ? { kind: "bar", span, days: span.last - span.first + 1 }
        : { kind: "gap" };
    });
    const timed = instancesOfDay(instances, day, tz).filter(
      (i) =>
        !isAllDayLike(i) &&
        (dateString(pimTimeIn(i.start, tz)) === dateString(day) ||
          // Started before the week: shown on its first day here.
          (index === 0 && instantMs(i.start_utc) < toZoned(day, tz).toDate().getTime())),
    );
    const room = Math.max(0, maxPerDay - shownLanes);
    const lines = timed.slice(0, room);
    const hiddenBars = covering.filter((s) => s.row >= maxPerDay).length;
    return {
      day,
      lanes,
      lines,
      more: hiddenBars + timed.length - lines.length,
      total: covering.length + timed.length,
    };
  });
}

// ---------------------------------------------------------------------------
// Year (C2)

/** Booked hours from which a day reaches each heat level (1 … 4). */
export const HEAT_STEPS_HOURS: readonly number[] = [0, 2, 4, 6];

/**
 * Booked minutes per day (`YYYY-MM-DD` in `tz`): busy timed occurrences by
 * their minutes on each day, busy all-day ones as a whole working day.
 */
export function bookedMinutes(
  instances: readonly EventInstance[],
  tz: string,
  workday: { start_minute: number; end_minute: number },
): Map<string, number> {
  const out = new Map<string, number>();
  const add = (key: string, minutes: number) => {
    out.set(key, Math.min(MINUTES_PER_DAY, (out.get(key) ?? 0) + minutes));
  };
  for (const instance of instances) {
    if (instance.transparency !== "busy") continue;
    const start = instantMs(instance.start_utc);
    const end = Math.max(instantMs(instance.end_utc), start + 1);
    let date = toZoned(toCalendarDate(instantIn(instance.start_utc, tz)), tz);
    for (let guard = 0; guard < 400 && date.toDate().getTime() < end; guard++) {
      const dayStart = date.toDate().getTime();
      const next = date.add({ days: 1 });
      const dayEnd = next.toDate().getTime();
      add(
        dateString(date),
        instance.all_day
          ? workday.end_minute - workday.start_minute
          : Math.max(0, Math.min(end, dayEnd) - Math.max(start, dayStart)) / 60_000,
      );
      date = next;
    }
  }
  return out;
}

/** The heat level (0 = free … 4) of `minutes` booked. */
export function heatLevel(minutes: number): number {
  const hours = minutes / 60;
  return HEAT_STEPS_HOURS.filter((step) => hours > step).length;
}

// ---------------------------------------------------------------------------
// Free time (C1)

/** A busy stretch of the free-time track (fractions of the working day). */
export interface BusyBlock {
  instance: EventInstance;
  left: number;
  width: number;
}

/** Busy timed occurrences of `day` inside the working day `[startMs, endMs)`, as fractions. */
export function busyBlocks(
  instances: readonly EventInstance[],
  day: CalendarDate,
  tz: string,
  startMs: number,
  endMs: number,
): BusyBlock[] {
  const span = Math.max(1, endMs - startMs);
  return instances
    .filter((i) => !isAllDayLike(i) && i.transparency === "busy" && overlapsDay(i, day, tz))
    .flatMap((instance) => {
      const left = Math.max(0, (instantMs(instance.start_utc) - startMs) / span);
      const right = Math.min(1, (instantMs(instance.end_utc) - startMs) / span);
      return right > left ? [{ instance, left, width: right - left }] : [];
    });
}

/** Busy all-day occurrences of `day` (the reason a whole day has no free time). */
export function busyAllDay(
  instances: readonly EventInstance[],
  day: CalendarDate,
  tz: string,
): EventInstance[] {
  return instances.filter(
    (i) => isAllDayLike(i) && i.transparency === "busy" && overlapsDay(i, day, tz),
  );
}

/** Minutes of the stretches (`start` / `end` RFC 3339). */
export function freeMinutes(stretches: readonly { start: string; end: string }[]): number {
  return stretches.reduce(
    (sum, s) => sum + Math.max(0, instantMs(s.end) - instantMs(s.start)) / 60_000,
    0,
  );
}

/** The whole hours of the free-time axis from `startHour` to `endHour` (inclusive), as fractions. */
export function hourTicks(startHour: number, endHour: number): { hour: number; at: number }[] {
  const span = Math.max(1, endHour - startHour);
  return Array.from({ length: span + 1 }, (_, i) => ({
    hour: startHour + i,
    at: i / span,
  }));
}

// ---------------------------------------------------------------------------
// Empty ranges (C4)

/**
 * The occurrence nearest to an empty range: the first one after it, else
 * the last one before it; `null` when there is none.
 */
export function nearestOutside(
  before: readonly EventInstance[],
  after: readonly EventInstance[],
): { instance: EventInstance; side: "before" | "after" } | null {
  const byStart = (a: EventInstance, b: EventInstance) =>
    instantMs(a.start_utc) - instantMs(b.start_utc);
  const next = [...after].sort(byStart)[0];
  if (next) return { instance: next, side: "after" };
  const last = [...before].sort(byStart).pop();
  return last ? { instance: last, side: "before" } : null;
}

// ---------------------------------------------------------------------------
// Search (S9)

export interface SearchHitGroups {
  /** From today on, soonest first. */
  upcoming: PimSearchHit[];
  /** Before today, latest first. */
  past: PimSearchHit[];
}

/** Event hits by upcoming / past (today counts as upcoming); hits without a start are left out. */
export function groupHits(
  hits: readonly PimSearchHit[],
  todayStartMs: number,
  tz: string,
): SearchHitGroups {
  const timed = hits.flatMap((hit) =>
    hit.start ? [{ hit, at: pimTimeIn(hit.start, tz).toDate().getTime() }] : [],
  );
  return {
    upcoming: timed
      .filter((h) => h.at >= todayStartMs)
      .sort((a, b) => a.at - b.at)
      .map((h) => h.hit),
    past: timed
      .filter((h) => h.at < todayStartMs)
      .sort((a, b) => b.at - a.at)
      .map((h) => h.hit),
  };
}

// ---------------------------------------------------------------------------
// Mini-month (S5)

/** At most this many dots under a day of the mini-month. */
export const MAX_DAY_DOTS = 3;

/** Distinct colours of the occurrences per day (at most [`MAX_DAY_DOTS`]); `""` = the theme's. */
export function dayDots(
  instances: readonly EventInstance[],
  calendars: readonly Calendar[] | undefined,
  days: readonly CalendarDate[],
  tz: string,
): Map<string, string[]> {
  const out = new Map<string, string[]>();
  for (const day of days) {
    const colors: string[] = [];
    for (const instance of instancesOfDay(instances, day, tz)) {
      const raw = colorOf(calendars, instance.calendar_id, instance.color);
      const color = isCssColor(raw) ? raw.trim().toLowerCase() : "";
      if (!colors.includes(color)) colors.push(color);
      if (colors.length >= MAX_DAY_DOTS) break;
    }
    if (colors.length > 0) out.set(dateString(day), colors);
  }
  return out;
}

/** Where a day sits in the band of the range on screen (`null` = outside it). */
export function bandOf(
  day: CalendarDate,
  range: ViewRange | null,
): "single" | "start" | "middle" | "end" | null {
  if (range === null) return null;
  const last = range.start.add({ days: range.count - 1 });
  if (day.compare(range.start) < 0 || day.compare(last) > 0) return null;
  if (range.count === 1) return "single";
  if (day.compare(range.start) === 0) return "start";
  return day.compare(last) === 0 ? "end" : "middle";
}

// ---------------------------------------------------------------------------
// Quick add (S2)

/** One chip of the quick-add preview. */
export interface QuickChip {
  kind: ParsedPart | "title";
  text: string;
}

/** The preview's chips: the title, then each recognised part as typed. */
export function quickChips(text: string, parsed: ParsedEventText): QuickChip[] {
  const parts = parsed.spans
    .filter((s) => s.start >= 0 && s.end > s.start && s.end <= text.length)
    .map((s): QuickChip => ({ kind: s.kind, text: text.slice(s.start, s.end).trim() }))
    .filter((c) => c.text !== "");
  const title = parsed.title.trim();
  return title === "" ? parts : [{ kind: "title", text: title }, ...parts];
}
