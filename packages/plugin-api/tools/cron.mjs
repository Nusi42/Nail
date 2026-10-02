/**
 * Background job schedules, as Nail parses them (`CronSchedule::parse` in
 * `crates/nail-api/src/plugins/jobs.rs`): five fields — minute, hour,
 * day of month, month, day of week (0–7, 0 and 7 = Sunday) — each `*`, a
 * number, a range `a-b`, a step (`*` or a range or a number, then `/n`),
 * or a comma list of those; or `@hourly`, `@daily`, `@weekly`. Schedules
 * that would fire more often than every 5 minutes are refused.
 */

/** Shortest interval between two runs (minutes). */
export const MIN_JOB_INTERVAL_MINUTES = 5;

/**
 * @typedef {object} CronSchedule
 * @property {Set<number>} minutes
 * @property {Set<number>} hours
 * @property {Set<number>} days
 * @property {Set<number>} months
 * @property {Set<number>} weekdays 0–6, Sunday = 0
 * @property {boolean} dayRestricted
 * @property {boolean} weekdayRestricted
 */

/**
 * @param {string} text
 * @param {number} min
 * @param {number} max
 * @returns {number}
 */
function number(text, min, max) {
  if (!/^\d+$/.test(text)) throw new CronError("invalid-field");
  const n = Number(text);
  if (n < min || n > max) throw new CronError("invalid-field");
  return n;
}

/**
 * @param {string} text
 * @param {number} min
 * @param {number} max
 * @returns {Set<number>}
 */
function field(text, min, max) {
  const set = new Set();
  for (const part of text.split(",")) {
    const slash = part.indexOf("/");
    const range = slash >= 0 ? part.slice(0, slash) : part;
    let step = 1;
    if (slash >= 0) {
      const s = part.slice(slash + 1);
      if (!/^\d+$/.test(s) || Number(s) === 0) throw new CronError("invalid-field");
      step = Number(s);
    }
    let from;
    let to;
    if (range === "*") {
      from = min;
      to = max;
    } else if (range.includes("-")) {
      const [a = "", b = "", ...rest] = range.split("-");
      if (rest.length > 0) throw new CronError("invalid-field");
      from = number(a, min, max);
      to = number(b, min, max);
    } else {
      from = number(range, min, max);
      // `5/10` means 5, 15, 25, … like `5-max/10`.
      to = slash >= 0 ? max : from;
    }
    if (from > to) throw new CronError("invalid-field");
    for (let n = from; n <= to; n += step) set.add(n);
  }
  return set;
}

/** Why a schedule is refused: `field-count`, `invalid-field` or `too-frequent` (Nail's `CronIssue`). */
export class CronError extends Error {
  /** @param {"field-count" | "invalid-field" | "too-frequent"} issue */
  constructor(issue) {
    super(`invalid schedule: ${issue}`);
    this.name = "CronError";
    this.issue = issue;
  }
}

/**
 * The smallest gap between two firings within a day, wrapping into the
 * next day (minutes).
 * @param {CronSchedule} s
 */
function minGap(s) {
  const times = [];
  for (const h of [...s.hours].sort((a, b) => a - b)) {
    for (const m of [...s.minutes].sort((a, b) => a - b)) times.push(h * 60 + m);
  }
  if (times.length < 2) return Infinity;
  let gap = Infinity;
  for (let i = 1; i < times.length; i += 1)
    gap = Math.min(gap, (times[i] ?? 0) - (times[i - 1] ?? 0));
  return Math.min(gap, (times[0] ?? 0) + 24 * 60 - (times[times.length - 1] ?? 0));
}

/**
 * Parses a schedule; throws {@link CronError}.
 * @param {string} text
 * @returns {CronSchedule}
 */
export function parseCron(text) {
  const trimmed = String(text).trim();
  const named = { "@hourly": "0 * * * *", "@daily": "0 0 * * *", "@weekly": "0 0 * * 0" };
  let expanded = trimmed;
  if (trimmed.startsWith("@")) {
    const value = named[/** @type {keyof typeof named} */ (trimmed)];
    if (!value) throw new CronError("field-count");
    expanded = value;
  }
  const fields = expanded.split(/\s+/).filter(Boolean);
  if (fields.length !== 5) throw new CronError("field-count");
  const [minute = "", hour = "", day = "", month = "", weekday = ""] = fields;
  const weekdays = field(weekday, 0, 7);
  if (weekdays.has(7)) {
    weekdays.delete(7);
    weekdays.add(0);
  }
  const schedule = {
    minutes: field(minute, 0, 59),
    hours: field(hour, 0, 23),
    days: field(day, 1, 31),
    months: field(month, 1, 12),
    weekdays,
    dayRestricted: day !== "*",
    weekdayRestricted: weekday !== "*",
  };
  if (minGap(schedule) < MIN_JOB_INTERVAL_MINUTES) throw new CronError("too-frequent");
  return schedule;
}

/**
 * Whether a schedule is valid.
 * @param {unknown} text
 */
export function isCron(text) {
  if (typeof text !== "string") return false;
  try {
    parseCron(text);
    return true;
  } catch {
    return false;
  }
}

/**
 * Whether the schedule fires at this local time (as in cron: when both
 * day fields are restricted, either matches).
 * @param {CronSchedule} s
 * @param {Date} date local time
 */
export function cronMatches(s, date) {
  const day = s.days.has(date.getDate());
  const weekday = s.weekdays.has(date.getDay());
  const dateOk =
    s.dayRestricted && s.weekdayRestricted
      ? day || weekday
      : s.dayRestricted
        ? day
        : s.weekdayRestricted
          ? weekday
          : true;
  return (
    s.minutes.has(date.getMinutes()) &&
    s.hours.has(date.getHours()) &&
    s.months.has(date.getMonth() + 1) &&
    dateOk
  );
}

/**
 * The next local time after `after` the schedule fires (minute
 * resolution; searches a little over four years, enough for 29 February).
 * @param {CronSchedule} s
 * @param {Date} after
 * @returns {Date | null}
 */
export function nextCronRun(s, after) {
  const t = new Date(after.getTime());
  t.setSeconds(0, 0);
  t.setMinutes(t.getMinutes() + 1);
  const limit = after.getTime() + 4 * 366 * 24 * 3600 * 1000;
  while (t.getTime() <= limit) {
    if (!s.months.has(t.getMonth() + 1)) {
      t.setMonth(t.getMonth() + 1, 1);
      t.setHours(0, 0, 0, 0);
      continue;
    }
    const day = s.days.has(t.getDate());
    const weekday = s.weekdays.has(t.getDay());
    const dateOk =
      s.dayRestricted && s.weekdayRestricted
        ? day || weekday
        : s.dayRestricted
          ? day
          : s.weekdayRestricted
            ? weekday
            : true;
    if (!dateOk) {
      t.setDate(t.getDate() + 1);
      t.setHours(0, 0, 0, 0);
      continue;
    }
    if (!s.hours.has(t.getHours())) {
      t.setHours(t.getHours() + 1, 0, 0, 0);
      continue;
    }
    if (s.minutes.has(t.getMinutes())) return t;
    t.setMinutes(t.getMinutes() + 1);
  }
  return null;
}
