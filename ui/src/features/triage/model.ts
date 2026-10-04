/**
 * Pure helpers of the triage feature, mirroring `nail_api::triage`
 * (ADR 0043): defaults and validation of the junk and notification
 * settings, and the names the UI shows.
 */
import type {
  JunkIssue,
  JunkSettings,
  JunkSource,
  NotificationIssue,
  NotificationSettings,
  Weekday,
} from "../../api/generated";

export const JUNK_ISSUES: readonly JunkIssue[] = ["threshold", "min-training"];
export const JUNK_SOURCES: readonly JunkSource[] = ["server", "classifier", "user"];
export const NOTIFICATION_ISSUES: readonly NotificationIssue[] = [
  "quiet-hours-time",
  "empty-quiet-hours",
  "duplicate-override",
];
export const WEEKDAYS: readonly Weekday[] = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"];

/** `JunkSettings::default()`. */
export function defaultJunkSettings(): JunkSettings {
  return {
    enabled: false,
    threshold: 90,
    min_training: 10,
    honor_server_flags: true,
    move_to_junk: true,
  };
}

/** `JunkSettings::issues`. */
export function junkIssues(s: JunkSettings): JunkIssue[] {
  const out: JunkIssue[] = [];
  if (s.threshold < 50 || s.threshold > 99) out.push("threshold");
  if (s.min_training === 0 || s.min_training > 10000) out.push("min-training");
  return out;
}

/** `NotificationSettings::default()`. */
export function defaultNotificationSettings(): NotificationSettings {
  return {
    enabled: true,
    show_preview: true,
    sound: true,
    folders: "inbox",
    overrides: [],
    quiet_hours: null,
  };
}

/** `nail_api::triage::parse_hhmm`: minutes since midnight, or `null`. */
export function parseHhmm(text: string): number | null {
  const match = /^(\d{2}):(\d{2})$/.exec(text);
  if (!match) return null;
  const h = Number(match[1]);
  const m = Number(match[2]);
  return h < 24 && m < 60 ? h * 60 + m : null;
}

/** `NotificationSettings::issues`. */
export function notificationIssues(s: NotificationSettings): NotificationIssue[] {
  const out: NotificationIssue[] = [];
  if (s.quiet_hours) {
    const start = parseHhmm(s.quiet_hours.start);
    const end = parseHhmm(s.quiet_hours.end);
    if (start === null || end === null) out.push("quiet-hours-time");
    else if (start === end) out.push("empty-quiet-hours");
  }
  const scopes = s.overrides.map((o) =>
    o.scope.type === "account" ? `a:${o.scope.account_id}` : `f:${o.scope.folder_id}`,
  );
  if (new Set(scopes).size !== scopes.length) out.push("duplicate-override");
  return out;
}

// ---- Snooze ----------------------------------------------------------------

/** Quick choices of the snooze picker. */
export const SNOOZE_PRESETS = ["later-today", "tomorrow", "weekend", "next-week"] as const;
export type SnoozePreset = (typeof SNOOZE_PRESETS)[number];

/** Hour of the day snoozed mail comes back ("tomorrow morning"). */
export const MORNING_HOUR = 8;

function at(date: Date, days: number, hour: number): Date {
  const out = new Date(date);
  out.setDate(out.getDate() + days);
  out.setHours(hour, 0, 0, 0);
  return out;
}

/**
 * When a preset wakes a message, in local time; `null` when the preset
 * makes no sense now ("later today" late in the evening, "this weekend" on
 * a weekend).
 */
export function presetTime(preset: SnoozePreset, now: Date): Date | null {
  const day = now.getDay(); // 0 = Sunday
  switch (preset) {
    case "later-today": {
      const later = new Date(now);
      later.setHours(now.getHours() + 3, 0, 0, 0);
      return later.getDate() === now.getDate() && later.getHours() <= 21 ? later : null;
    }
    case "tomorrow":
      return at(now, 1, MORNING_HOUR);
    case "weekend":
      return day === 0 || day === 6 ? null : at(now, 6 - day, MORNING_HOUR);
    case "next-week":
      return at(now, (8 - day) % 7 || 7, MORNING_HOUR);
  }
}

function pad(n: number): string {
  return String(n).padStart(2, "0");
}

/** RFC 3339 with the local offset (`2026-10-01T08:00:00+02:00`). */
export function toRfc3339(date: Date): string {
  const offset = -date.getTimezoneOffset();
  const sign = offset >= 0 ? "+" : "-";
  const abs = Math.abs(offset);
  return (
    `${String(date.getFullYear())}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}` +
    `T${pad(date.getHours())}:${pad(date.getMinutes())}:${pad(date.getSeconds())}` +
    `${sign}${pad(Math.floor(abs / 60))}:${pad(abs % 60)}`
  );
}

/** The value of an `<input type="datetime-local">` for a date. */
export function toLocalInput(date: Date): string {
  return (
    `${String(date.getFullYear())}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}` +
    `T${pad(date.getHours())}:${pad(date.getMinutes())}`
  );
}

/** Parses an `<input type="datetime-local">` value; `null` when invalid. */
export function fromLocalInput(value: string): Date | null {
  const match = /^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2})$/.exec(value);
  if (!match) return null;
  const [, y, mo, d, h, mi] = match.map(Number) as [number, number, number, number, number, number];
  const date = new Date(y, mo - 1, d, h, mi, 0, 0);
  return date.getMonth() === mo - 1 && date.getDate() === d ? date : null;
}

// ---- Notifications ---------------------------------------------------------

/** How an account or folder notifies: the default, always, never. */
export type OverrideChoice = "default" | "on" | "off";

/** The override of a scope in the settings. */
export function overrideOf(
  settings: NotificationSettings,
  scope: NotificationSettings["overrides"][number]["scope"],
): OverrideChoice {
  const found = settings.overrides.find((o) =>
    o.scope.type === "account"
      ? scope.type === "account" && o.scope.account_id === scope.account_id
      : scope.type === "folder" && o.scope.folder_id === scope.folder_id,
  );
  return found === undefined ? "default" : found.notify ? "on" : "off";
}

/** The settings with the override of `scope` set (or removed for `default`). */
export function withOverride(
  settings: NotificationSettings,
  scope: NotificationSettings["overrides"][number]["scope"],
  choice: OverrideChoice,
): NotificationSettings {
  const same = (o: NotificationSettings["overrides"][number]) =>
    o.scope.type === scope.type &&
    (o.scope.type === "account"
      ? scope.type === "account" && o.scope.account_id === scope.account_id
      : scope.type === "folder" && o.scope.folder_id === scope.folder_id);
  const overrides = settings.overrides.filter((o) => !same(o));
  if (choice !== "default") overrides.push({ scope, notify: choice === "on" });
  return { ...settings, overrides };
}
