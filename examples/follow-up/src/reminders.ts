/**
 * Follow-ups: what the plug-in keeps in its storage (one key per message:
 * the task it created and whether to remind) and the date arithmetic
 * around them. No Nail API calls here, so the logic is easy to test.
 */
import type { JsonValue, PluginMessage } from "@nail/plugin-api";

/** A follow-up the plug-in created a task for. */
export interface FollowUp {
  /** The message (`nail.mail.get` id). */
  messageId: string;
  /** `nail://messages/<id>`. */
  messageUri: string;
  /** The message's subject when the follow-up was made. */
  subject: string;
  /** The sender, as shown. */
  from: string;
  /** The task Nail created. */
  taskId: string;
  /** Due date, `YYYY-MM-DD`. */
  due: string;
  /** The note (may be empty). */
  note: string;
  /** Whether to notify when it is due. */
  remind: boolean;
  /** The day the due notification was shown (`YYYY-MM-DD`), if it was. */
  notifiedOn?: string;
  /** When it was made (ISO 8601). */
  createdAt: string;
}

/** Prefix of the storage keys. */
export const KEY_PREFIX = "follow-up:";

/** The storage key of a message's follow-up. */
export function followUpKey(messageId: string): string {
  return `${KEY_PREFIX}${messageId}`;
}

const pad = (n: number) => String(n).padStart(2, "0");

/** A local date as `YYYY-MM-DD`. */
export function isoDate(date: Date): string {
  return `${String(date.getFullYear())}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`;
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
  return isoDate(new Date(now.getFullYear(), now.getMonth(), now.getDate() + d));
}

/** A readable sender. */
export function senderOf(message: PluginMessage): string {
  const from = message.from;
  if (!from) return "";
  return from.name ? `${from.name} <${from.email}>` : from.email;
}

/** What the dialog answered, checked. */
export interface FollowUpInput {
  /** `YYYY-MM-DD`. */
  due: string;
  /** The task list chosen. */
  list: string | undefined;
  /** The note, trimmed. */
  note: string;
  /** Remind when due. */
  remind: boolean;
}

/** Reads the dialog's values; `null` when the date is missing or malformed. */
export function inputFrom(values: Record<string, JsonValue>): FollowUpInput | null {
  const due = values.due;
  if (typeof due !== "string" || !/^\d{4}-\d{2}-\d{2}/.test(due)) return null;
  const list = values.list;
  const chosen = Array.isArray(list) ? list[0] : list;
  return {
    due: due.slice(0, 10),
    list: typeof chosen === "string" && chosen ? chosen : undefined,
    note: typeof values.note === "string" ? values.note.trim() : "",
    remind: values.remind !== false,
  };
}

/** Whether a stored value is a follow-up (storage is JSON; be defensive). */
export function isFollowUp(value: unknown): value is FollowUp {
  if (typeof value !== "object" || value === null) return false;
  const r = value as Record<string, unknown>;
  return (
    typeof r.messageId === "string" &&
    typeof r.taskId === "string" &&
    typeof r.due === "string" &&
    typeof r.subject === "string" &&
    typeof r.note === "string"
  );
}

/** Follow-ups sorted by due date, then subject. */
export function sortFollowUps(items: FollowUp[]): FollowUp[] {
  return [...items].sort(
    (a, b) => a.due.localeCompare(b.due) || a.subject.localeCompare(b.subject),
  );
}

/** Whether a follow-up is due before `today` (`YYYY-MM-DD`). */
export function isPast(item: FollowUp, today: string): boolean {
  return item.due < today;
}

/** The follow-ups to notify about `today`: due, reminders on, not notified yet. */
export function dueForReminder(items: FollowUp[], today: string): FollowUp[] {
  return items.filter((f) => f.remind && f.due <= today && f.notifiedOn === undefined);
}

/** As JSON for storage. */
export function toJson(item: FollowUp): JsonValue {
  const out: Record<string, JsonValue> = { ...item };
  if (item.notifiedOn === undefined) delete out.notifiedOn;
  return out;
}
