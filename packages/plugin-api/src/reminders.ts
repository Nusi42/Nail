/**
 * The host's rule for task reminders a plug-in writes (`nail.tasks.create`
 * / `update`, R8): a list of at most `MAX_TASK_REMINDERS`, each exactly one
 * of `minutesBefore` (a whole number of minutes, 0 to 4 294 967 295, needs
 * a due time) and `at` (an RFC 3339 UTC instant ending in `Z`), no other
 * fields. The test host (`testing.ts`) applies it so a plug-in's tests
 * refuse what Nail refuses. Its single source is the Rust rule
 * (`nail_api::mcp::tools::task_reminder_alarms` behind the plug-in's input
 * parsing); `reminders.test.ts` runs this port over the vectors the Rust
 * test writes (`crates/nail-api/fixtures/ts-vectors/task-reminders.json`).
 */
import { MAX_TASK_REMINDERS } from "./generated/constants";

/** The largest `minutesBefore` (an unsigned 32-bit number of minutes, as the host reads it). */
const MAX_MINUTES_BEFORE = 0xffff_ffff;

const UTC_INSTANT = /^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2}):(\d{2})(?:\.\d{1,9})?Z$/;

/** Whether `text` is an RFC 3339 UTC instant as the host accepts it (`…Z`, a real date and time). */
export function isUtcInstant(text: string): boolean {
  const m = UTC_INSTANT.exec(text);
  if (!m) return false;
  const [year, month, day, hour, minute, second] = m.slice(1, 7).map(Number) as [
    number,
    number,
    number,
    number,
    number,
    number,
  ];
  if (year < 1 || month < 1 || month > 12 || day < 1) return false;
  const leap = (year % 4 === 0 && year % 100 !== 0) || year % 400 === 0;
  const days = [31, leap ? 29 : 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31][month - 1] ?? 0;
  return day <= days && hour < 24 && minute < 60 && second <= 60;
}

/**
 * Why the host refuses `reminders` (English), or `null` when it takes them.
 * `hasDue`: the task has a due time (a reminder before it needs one).
 */
export function taskReminderIssue(reminders: unknown, hasDue: boolean): string | null {
  if (!Array.isArray(reminders)) return "reminders must be a list";
  for (const reminder of reminders as unknown[]) {
    if (typeof reminder !== "object" || reminder === null || Array.isArray(reminder)) {
      return "a reminder is an object";
    }
    const fields = reminder as Record<string, unknown>;
    if (Object.keys(fields).some((k) => k !== "minutesBefore" && k !== "at")) {
      return "a reminder has only minutesBefore or at";
    }
    const minutes = fields.minutesBefore ?? undefined;
    const at = fields.at ?? undefined;
    if (
      minutes !== undefined &&
      (typeof minutes !== "number" ||
        !Number.isInteger(minutes) ||
        minutes < 0 ||
        minutes > MAX_MINUTES_BEFORE)
    ) {
      return "minutesBefore is a whole number of minutes";
    }
    if (at !== undefined && typeof at !== "string") return "at is a text";
  }
  if (reminders.length > MAX_TASK_REMINDERS) {
    return `at most ${String(MAX_TASK_REMINDERS)} reminders`;
  }
  for (const reminder of reminders as Record<string, unknown>[]) {
    const minutes = reminder.minutesBefore ?? undefined;
    const at = (reminder.at ?? undefined) as string | undefined;
    if ((minutes === undefined) === (at === undefined)) {
      return "a reminder has exactly one of minutesBefore and at";
    }
    if (minutes !== undefined && !hasDue) return "minutesBefore needs a due time";
    if (at !== undefined && !isUtcInstant(at)) return `at is not an RFC 3339 UTC instant: ${at}`;
  }
  return null;
}
