/**
 * How the calendar list groups calendars and what each calendar and group
 * offers (WP164): one group per source — this computer, each CalDAV /
 * Exchange account — and every ICS subscription together under
 * "Subscriptions" at the end. Pure, so the rules are tested without
 * rendering.
 */
import type { Calendar, PimSource, PimSourceId, PimSourceKind } from "../../api/generated";

/** A group of the calendar list. */
export interface CalendarGroup {
  /** Stable key (`source:<id>` or `subscriptions`). */
  key: string;
  /** What the header names: a source, the subscriptions, or all calendars. */
  kind: "source" | "subscriptions" | "all";
  /** The source of a `source` group. */
  source: PimSource | null;
  /** The source new calendars of this group go to (`null` = none can be added). */
  createIn: PimSourceId | null;
  calendars: Calendar[];
}

/** How a calendar can be removed: deleted, unsubscribed from, or not at all. */
export type Removal = "delete" | "unsubscribe" | null;

/** Sources that can hold new calendars (the core refuses the others). */
const CREATABLE: readonly PimSourceKind[] = ["local", "caldav"];

function byOrder(a: Calendar, b: Calendar): number {
  return a.settings.order - b.settings.order || a.settings.name.localeCompare(b.settings.name);
}

/**
 * The groups, in list order: sources in the order of their first calendar
 * (the local source first), subscriptions last. With `bySource` false one
 * group holds every calendar (new ones go to this computer).
 */
export function calendarGroups(
  calendars: readonly Calendar[],
  sources: readonly PimSource[],
  bySource = true,
): CalendarGroup[] {
  const sorted = [...calendars].sort(byOrder);
  if (!bySource) {
    return [{ key: "all", kind: "all", source: null, createIn: "local", calendars: sorted }];
  }
  const sourceOf = (id: PimSourceId) => sources.find((s) => s.id === id) ?? null;
  const groups = new Map<string, CalendarGroup>();
  const subscriptions: Calendar[] = [];
  for (const calendar of sorted) {
    const source = sourceOf(calendar.source_id);
    if (source?.kind === "ics") {
      subscriptions.push(calendar);
      continue;
    }
    const key = `source:${calendar.source_id}`;
    const group = groups.get(key) ?? {
      key,
      kind: "source" as const,
      source,
      createIn: source && CREATABLE.includes(source.kind) ? source.id : null,
      calendars: [],
    };
    group.calendars.push(calendar);
    groups.set(key, group);
  }
  // A computer without calendars still offers "New calendar" there.
  const local = sources.find((s) => s.kind === "local");
  if (local && !groups.has(`source:${local.id}`)) {
    groups.set(`source:${local.id}`, {
      key: `source:${local.id}`,
      kind: "source",
      source: local,
      createIn: local.id,
      calendars: [],
    });
  }
  const list = [...groups.values()].sort(
    (a, b) => Number(b.source?.kind === "local") - Number(a.source?.kind === "local"),
  );
  if (subscriptions.length > 0) {
    list.push({
      key: "subscriptions",
      kind: "subscriptions",
      source: null,
      createIn: null,
      calendars: subscriptions,
    });
  }
  return list;
}

/** How `calendar` can be removed. */
export function removalOf(calendar: Calendar, sources: readonly PimSource[]): Removal {
  const kind = sources.find((s) => s.id === calendar.source_id)?.kind;
  if (kind === "ics") return "unsubscribe";
  if ((kind === "local" || kind === "caldav") && !calendar.read_only) return "delete";
  return null;
}

/** A calendar that only holds tasks (a task list). */
export function isTaskList(calendar: Calendar): boolean {
  return calendar.components.includes("task") && !calendar.components.includes("event");
}
