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
