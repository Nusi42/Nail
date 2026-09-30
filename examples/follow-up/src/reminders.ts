/**
 * Reminders: what the plug-in keeps in its storage (one key per message)
 * and the date arithmetic around them. No Nail API calls here, so the
 * logic is easy to test.
 */
import type { JsonValue, PluginMessage } from "@nail/plugin-api";

/** A follow-up reminder. */
export interface Reminder {
  /** The message (`nail.mail.get` id). */
  messageId: string;
  /** `nail://messages/<id>`. */
  messageUri: string;
  /** The message's subject when the reminder was set. */
  subject: string;
  /** The sender, as shown. */
  from: string;
  /** Due date, `YYYY-MM-DD`. */
  due: string;
  /** The note (may be empty). */
  note: string;
  /** When it was set (ISO 8601). */
  createdAt: string;
}

/** Prefix of the storage keys. */
export const KEY_PREFIX = "reminder:";

/** The storage key of a message's reminder. */
export function reminderKey(messageId: string): string {
  return `${KEY_PREFIX}${messageId}`;
}

const pad = (n: number) => String(n).padStart(2, "0");

/** A local date as `YYYY-MM-DD`. */
export function isoDate(date: Date): string {
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`;
}

/**
 * The date the dialog suggests, `days` ahead of `now`: the keywords
 * `today` / `tomorrow` for 0 / 1 (the UI resolves them in the user's time
 * zone), else a date.
 */
export function suggestedDue(days: number, now: Date): string {
  const d = Math.max(0, Math.floor(Number.isFinite(days) ? days : 1));
  if (d === 0) return "today";
  if (d === 1) return "tomorrow";
  const date = new Date(now.getFullYear(), now.getMonth(), now.getDate() + d);
  return isoDate(date);
}

/** A readable sender. */
export function senderOf(message: PluginMessage): string {
  const from = message.from;
  if (!from) return "";
  return from.name ? `${from.name} <${from.email}>` : from.email;
}

/** Builds a reminder from the dialog's values; `null` when the date is missing or malformed. */
export function reminderFrom(
  message: PluginMessage,
  values: Record<string, JsonValue>,
  now: Date,
): Reminder | null {
  const due = values.due;
  if (typeof due !== "string" || !/^\d{4}-\d{2}-\d{2}/.test(due)) return null;
  const note = typeof values.note === "string" ? values.note.trim() : "";
  return {
    messageId: message.id,
    messageUri: message.uri,
    subject: message.subject,
    from: senderOf(message),
    due: due.slice(0, 10),
    note,
    createdAt: now.toISOString(),
  };
}

/** Whether a stored value is a reminder (storage is JSON; be defensive). */
export function isReminder(value: unknown): value is Reminder {
  if (typeof value !== "object" || value === null) return false;
  const r = value as Record<string, unknown>;
  return (
    typeof r.messageId === "string" &&
    typeof r.due === "string" &&
    typeof r.subject === "string" &&
    typeof r.note === "string"
  );
}

/** Reminders sorted by due date, then subject. */
export function sortReminders(reminders: Reminder[]): Reminder[] {
  return [...reminders].sort(
    (a, b) => a.due.localeCompare(b.due) || a.subject.localeCompare(b.subject),
  );
}

/** Whether a reminder is due before `today` (`YYYY-MM-DD`). */
export function isPast(reminder: Reminder, today: string): boolean {
  return reminder.due < today;
}

/** As JSON for storage. */
export function toJson(reminder: Reminder): JsonValue {
  return { ...reminder };
}
