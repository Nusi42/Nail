/**
 * The mock core's answers to the M3a calendar, contact and task requests
 * (ADR 0058), used by `MockTransport` (`../../api/mock.ts` routes exactly
 * these requests here).
 *
 * An in-memory PIM on `data.features.pim`, answering like the core will
 * once WP75 wires it: sources (local, ICS, CalDAV / CardDAV), calendars and
 * task lists, events as series with overrides and the three edit scopes
 * (`this-and-following` splits the series), expanded instances, free time,
 * address books, contacts with duplicates, merging (fold in the same book,
 * link across books) and groups, tasks with subtasks and recurring
 * completion, reminders (snooze / dismiss), search and `.ics` / `.vcf`
 * import of uploaded text. Writes answer with queued operation ids and the
 * "replay" reports `PimOpStatus` plus a change event, like the PIM queue.
 */
import type {
  AddressBook,
  Calendar,
  CalendarEvent,
  Contact,
  ContactFields,
  ContactGroup,
  ContactMatch,
  ContactSummary,
  DuplicateGroup,
  DuplicateReason,
  EventFields,
  EventInstance,
  FreeSlot,
  PimImportPreview,
  PimIssue,
  PimSearchHit,
  PimSource,
  PimSourceConfig,
  PimTime,
  Recurrence,
  Reminder,
  Request,
  Response,
  Task,
  TaskFields,
} from "../../api/generated";
import { errorResponse, type MockCore } from "../../api/mockMail";
import { endFor, expandEvent, instantOf, occurrenceStarts, sameOccurrence } from "./mockExpand";
import { demoPim, emptyContactFields, emptyEventFields, emptyTaskFields } from "./mockSeed";
import {
  dateString,
  instantMs,
  localString,
  localZone,
  makePimTime,
  parseDay,
  parseLocal,
  pimTimeIn,
  shiftPimTime,
  todayIn,
  toZoned,
  utcString,
} from "./time";
import { fromAbsolute } from "@internationalized/date";

/** The requests this feature answers. */
export const PIM_REQUESTS = [
  "ListPimSources",
  "AddPimSource",
  "UpdatePimSource",
  "RemovePimSource",
  "TestPimSource",
  "SyncPimSources",
  "ListCalendars",
  "CreateCalendar",
  "UpdateCalendar",
  "DeleteCalendar",
  "ListEventInstances",
  "GetEvent",
  "CreateEvent",
  "UpdateEvent",
  "DeleteEvent",
  "MoveEvent",
  "EventFromMessage",
  "FindFreeTime",
  "ListAddressBooks",
  "CreateAddressBook",
  "UpdateAddressBook",
  "DeleteAddressBook",
  "ListContacts",
  "GetContact",
  "CreateContact",
  "UpdateContact",
  "DeleteContacts",
  "MoveContact",
  "SearchContacts",
  "FindDuplicateContacts",
  "MergeContacts",
  "UnlinkContact",
  "GetContactPhoto",
  "ListContactGroups",
  "CreateContactGroup",
  "UpdateContactGroup",
  "DeleteContactGroup",
  "ListTasks",
  "GetTask",
  "CreateTask",
  "UpdateTask",
  "CompleteTask",
  "MoveTask",
  "DeleteTask",
  "TaskFromMessage",
  "SearchPim",
  "PreviewPimImport",
  "ImportPim",
  "ResolvePimConflict",
  "ListReminders",
  "SnoozeReminders",
  "DismissReminders",
] as const satisfies readonly Request["type"][];

export type PimRequest = Extract<Request, { type: (typeof PIM_REQUESTS)[number] }>;

export function isPimRequest(request: Request): request is PimRequest {
  return (PIM_REQUESTS as readonly string[]).includes(request.type);
}

/** The mock's PIM data. */
export interface PimMockState {
  sources: PimSource[];
  calendars: Calendar[];
  addressBooks: AddressBook[];
  events: CalendarEvent[];
  contacts: Contact[];
  groups: ContactGroup[];
  tasks: Task[];
  reminders: Reminder[];
  /** Answer every PIM request `unsupported` (the core before WP75). */
  unsupported: boolean;
  /** Make the next write's replay fail (tests). */
  failNextOp: boolean;
}

/** The demo PIM data (see `mockSeed.ts`); `empty` for a bare local source. */
export function createPimMockState(
  options: { now?: number; tz?: string; empty?: boolean } = {},
): PimMockState {
  const seed = demoPim(options.now ?? Date.now(), options.tz ?? localZone());
  if (options.empty) {
    return {
      ...seed,
      sources: seed.sources.filter((s) => s.id === "local"),
      calendars: [],
      addressBooks: [],
      events: [],
      contacts: [],
      groups: [],
      tasks: [],
      reminders: [],
      unsupported: false,
      failNextOp: false,
    };
  }
  return { ...seed, unsupported: false, failNextOp: false };
}

// ---------------------------------------------------------------------------
// Helpers

function state(core: MockCore): PimMockState {
  return core.data.features.pim;
}

const clone = <T>(value: T): T => structuredClone(value);

function notFound(what: string): Response {
  return errorResponse("not_found", `${what} not found`);
}

function invalid(issue: PimIssue): Response {
  return {
    type: "Error",
    code: "invalid_request",
    message: `invalid PIM data: ${issue}`,
    localized: { id: `pim-issue-${issue}` },
  };
}

function forbidden(what: string): Response {
  return errorResponse("forbidden", `${what} is read-only`);
}

/** Queues a PIM write: answers its op id; the "replay" reports and emits `changed`. */
function queue(
  core: MockCore,
  sourceId: string,
  changed: Parameters<MockCore["emit"]>[0][],
): string[] {
  const op = core.nextId("pimop-");
  const pim = state(core);
  const fail = pim.failNextOp;
  pim.failNextOp = false;
  core.later(() => {
    core.emit({
      type: "PimOpStatus",
      status: {
        op_id: op,
        source_id: sourceId,
        state: fail ? "failed" : "done",
        error: fail ? "network" : null,
        message: fail ? "server unreachable (mock)" : null,
      },
    });
  });
  for (const event of changed) core.emit(event);
  return [op];
}

function calendarOf(pim: PimMockState, id: string): Calendar | undefined {
  return pim.calendars.find((c) => c.id === id);
}

function bookOf(pim: PimMockState, id: string): AddressBook | undefined {
  return pim.addressBooks.find((b) => b.id === id);
}

function sourceOfCalendar(pim: PimMockState, id: string): string {
  return calendarOf(pim, id)?.source_id ?? "local";
}

function sourceOfBook(pim: PimMockState, id: string): string {
  return bookOf(pim, id)?.source_id ?? "local";
}

const now = () => utcString(Date.now());

function plausibleEmail(text: string): boolean {
  const at = text.lastIndexOf("@");
  return at > 0 && at < text.length - 1 && !/\s/.test(text) && !text.slice(at + 1).startsWith(".");
}

function timeIssue(time: PimTime): PimIssue | null {
  if (time.type === "date") return parseDay(time.date) ? null : "invalid-date";
  if (!parseLocal(time.local)) return "invalid-time";
  if (time.type === "zoned") {
    try {
      fromAbsolute(0, time.tz);
    } catch {
      return "invalid-time-zone";
    }
  }
  return null;
}

/** Like `EventFields::issues` (first issue only). */
export function eventIssue(fields: EventFields, recurrence: Recurrence | null): PimIssue | null {
  const issue = timeIssue(fields.start) ?? timeIssue(fields.end);
  if (issue) return issue;
  if ((fields.start.type === "date") !== (fields.end.type === "date")) return "mixed-all-day";
  if (instantOf(fields.end, "UTC") < instantOf(fields.start, "UTC")) return "end-before-start";
  if (
    fields.start.type === "date" &&
    fields.end.type === "date" &&
    fields.end.date <= fields.start.date
  ) {
    return "end-before-start";
  }
  if (fields.attendees.some((a) => !plausibleEmail(a.email))) return "invalid-email";
  if (fields.organizer && !plausibleEmail(fields.organizer.email)) return "invalid-email";
  if (fields.priority !== null && (fields.priority < 0 || fields.priority > 9))
    return "invalid-priority";
  if (
    recurrence?.rules.some((r) => r.interval < 1 || (r.end.type === "count" && r.end.count < 1))
  ) {
    return "invalid-recurrence";
  }
  if (
    fields.alarms.some(
      (a) => a.repeat !== null && (a.repeat.count < 1 || a.repeat.interval_secs < 1),
    )
  ) {
    return "invalid-alarm";
  }
  return null;
}

/** Like `TaskFields::issues` (first issue only). */
export function taskIssue(fields: TaskFields): PimIssue | null {
  for (const time of [fields.start, fields.due]) {
    const issue = time ? timeIssue(time) : null;
    if (issue) return issue;
  }
  if (fields.priority !== null && (fields.priority < 0 || fields.priority > 9))
    return "invalid-priority";
  if (
    fields.percent_complete !== null &&
    (fields.percent_complete < 0 || fields.percent_complete > 100)
  ) {
    return "invalid-priority";
  }
  return null;
}

/** Like `ContactFields::issues` (first issue only). */
export function contactIssue(fields: ContactFields): PimIssue | null {
  const name =
    fields.display_name.trim() ||
    [fields.name?.given, fields.name?.family].filter(Boolean).join(" ");
  if (!name && !fields.organization?.name) return "missing-name";
  if (fields.emails.some((e) => !plausibleEmail(e.value))) return "invalid-email";
  return null;
}

// ---------------------------------------------------------------------------
// Events

function eventsChanged(calendarIds: string[]): Parameters<MockCore["emit"]>[0] {
  return { type: "EventsChanged", calendar_ids: [...new Set(calendarIds)] };
}

/** The day / moment just before an occurrence, as an `UNTIL` of the series' kind. */
function untilBefore(occurrence: PimTime): PimTime {
  if (occurrence.type === "date") {
    const day = parseDay(occurrence.date);
    return { type: "date", date: day ? dateString(day.add({ days: -1 })) : occurrence.date };
  }
  if (occurrence.type === "floating") {
    const local = parseLocal(occurrence.local);
    return {
      type: "floating",
      local: local ? localString(local.add({ seconds: -1 })) : occurrence.local,
    };
  }
  const ms = instantOf(occurrence, "UTC") - 1000;
  return { type: "zoned", local: utcString(ms).slice(0, 19), tz: "UTC" };
}

/** Wall-clock difference (days, minutes) between two starts of the same kind. */
function startDelta(from: PimTime, to: PimTime): { days: number; minutes: number } {
  if (from.type === "date" && to.type === "date") {
    const a = parseDay(from.date);
    const b = parseDay(to.date);
    return { days: a && b ? b.compare(a) : 0, minutes: 0 };
  }
  if (from.type !== "date" && to.type !== "date") {
    const minutes = Math.round((instantOf(to, "UTC") - instantOf(from, "UTC")) / 60_000);
    return { days: 0, minutes };
  }
  return { days: 0, minutes: 0 };
}

function shiftOccurrences(event: CalendarEvent, oldStart: PimTime, newStart: PimTime): void {
  const { days, minutes } = startDelta(oldStart, newStart);
  if ((days === 0 && minutes === 0) || oldStart.type !== newStart.type) return;
  const shift = (time: PimTime) =>
    time.type === newStart.type ? shiftPimTime(time, days, minutes) : time;
  event.overrides = event.overrides.map((o) => ({ ...o, recurrence_id: shift(o.recurrence_id) }));
  if (event.recurrence) {
    event.recurrence = {
      ...event.recurrence,
      exceptions: event.recurrence.exceptions.map(shift),
      dates: event.recurrence.dates.map(shift),
    };
  }
}

function isFirstOccurrence(event: CalendarEvent, recurrenceId: PimTime): boolean {
  const [first] = occurrenceStarts(event, instantOf(recurrenceId, "UTC") + 1, "UTC");
  return first !== undefined && sameOccurrence(first, recurrenceId);
}

function truncateBefore(event: CalendarEvent, recurrenceId: PimTime): void {
  const until = untilBefore(recurrenceId);
  const cut = instantOf(recurrenceId, "UTC");
  if (event.recurrence) {
    event.recurrence = {
      rules: event.recurrence.rules.map((r) => ({ ...r, end: { type: "until", until } })),
      dates: event.recurrence.dates.filter((d) => instantOf(d, "UTC") < cut),
      exceptions: event.recurrence.exceptions.filter((d) => instantOf(d, "UTC") < cut),
    };
  }
  event.overrides = event.overrides.filter((o) => instantOf(o.recurrence_id, "UTC") < cut);
}

function updateEvent(
  core: MockCore,
  request: Extract<PimRequest, { type: "UpdateEvent" }>,
): Response {
  const pim = state(core);
  const event = pim.events.find((e) => e.id === request.event_id);
  if (!event) return notFound(`event ${request.event_id}`);
  if (event.read_only || calendarOf(pim, event.calendar_id)?.read_only)
    return forbidden("the calendar");
  const issue = eventIssue(request.fields, request.recurrence);
  if (issue) return invalid(issue);
  const recurring = event.recurrence !== null;
  const scope = recurring ? request.scope : "all";
  if (scope !== "all" && request.recurrence_id === null) return invalid("invalid-scope");
  let split: CalendarEvent | null = null;
  if (scope === "this" && request.recurrence_id) {
    const rid = request.recurrence_id;
    event.overrides = [
      ...event.overrides.filter((o) => !sameOccurrence(o.recurrence_id, rid)),
      { recurrence_id: rid, fields: clone(request.fields), this_and_future: false },
    ];
  } else if (
    scope === "this-and-following" &&
    request.recurrence_id &&
    !isFirstOccurrence(event, request.recurrence_id)
  ) {
    const rid = request.recurrence_id;
    const cut = instantOf(rid, "UTC");
    const following = event.overrides.filter((o) => instantOf(o.recurrence_id, "UTC") >= cut);
    const oldRecurrence = event.recurrence;
    truncateBefore(event, rid);
    const id = core.nextId("ev-");
    const newRecurrence: Recurrence | null =
      request.recurrence ??
      (oldRecurrence
        ? {
            rules: oldRecurrence.rules,
            dates: oldRecurrence.dates.filter((d) => instantOf(d, "UTC") >= cut),
            exceptions: oldRecurrence.exceptions.filter((d) => instantOf(d, "UTC") >= cut),
          }
        : null);
    split = {
      ...clone(event),
      id,
      uid: `${id}@nail.example`,
      fields: clone(request.fields),
      recurrence: newRecurrence,
      overrides: following,
      sequence: 0,
      created_at: now(),
      updated_at: now(),
      sync: "pending",
    };
    shiftOccurrences(split, rid, request.fields.start);
    pim.events.push(split);
  } else {
    const oldStart = event.fields.start;
    event.fields = clone(request.fields);
    event.recurrence = request.recurrence ? clone(request.recurrence) : null;
    if (event.recurrence) shiftOccurrences(event, oldStart, request.fields.start);
    else event.overrides = [];
  }
  event.sequence += 1;
  event.updated_at = now();
  event.sync = "pending";
  const op_ids = queue(core, sourceOfCalendar(pim, event.calendar_id), [
    eventsChanged([event.calendar_id]),
  ]);
  return { type: "EventSaved", event: clone(event), split: split ? clone(split) : null, op_ids };
}

function deleteEvent(
  core: MockCore,
  request: Extract<PimRequest, { type: "DeleteEvent" }>,
): Response {
  const pim = state(core);
  const event = pim.events.find((e) => e.id === request.event_id);
  if (!event) return notFound(`event ${request.event_id}`);
  if (event.read_only || calendarOf(pim, event.calendar_id)?.read_only)
    return forbidden("the calendar");
  const recurring = event.recurrence !== null;
  const scope = recurring ? request.scope : "all";
  if (scope !== "all" && request.recurrence_id === null) return invalid("invalid-scope");
  const rid = request.recurrence_id;
  if (scope === "this" && rid && event.recurrence) {
    event.overrides = event.overrides.filter((o) => !sameOccurrence(o.recurrence_id, rid));
    event.recurrence = { ...event.recurrence, exceptions: [...event.recurrence.exceptions, rid] };
    event.sequence += 1;
  } else if (scope === "this-and-following" && rid && !isFirstOccurrence(event, rid)) {
    truncateBefore(event, rid);
    event.sequence += 1;
  } else {
    pim.events = pim.events.filter((e) => e !== event);
    pim.reminders = pim.reminders.filter(
      (r) => !(r.target.type === "event" && r.target.event_id === event.id),
    );
  }
  const op_ids = queue(core, sourceOfCalendar(pim, event.calendar_id), [
    eventsChanged([event.calendar_id]),
  ]);
  return { type: "PimDeleted", op_ids };
}

function visibleCalendarIds(pim: PimMockState, component: "event" | "task"): string[] {
  return pim.calendars
    .filter((c) => c.settings.visible && c.components.includes(component))
    .map((c) => c.id);
}

export function listInstances(
  pim: PimMockState,
  from: string,
  to: string,
  calendarIds: string[] | null,
  tz: string,
): EventInstance[] {
  const ids = calendarIds ?? visibleCalendarIds(pim, "event");
  const fromMs = instantMs(from);
  const toMs = instantMs(to);
  return pim.events
    .filter((e) => ids.includes(e.calendar_id))
    .flatMap((e) =>
      expandEvent(e, fromMs, toMs, tz, calendarOf(pim, e.calendar_id)?.read_only ?? false),
    )
    .sort(
      (a, b) => instantMs(a.start_utc) - instantMs(b.start_utc) || a.title.localeCompare(b.title),
    );
}

/** Free stretches of at least `minutes` within working hours (08:00–18:00 in `tz`). */
export function freeTime(
  pim: PimMockState,
  from: string,
  to: string,
  minutes: number,
  calendarIds: string[] | null,
  tz: string,
): FreeSlot[] {
  const busy = listInstances(pim, from, to, calendarIds, tz)
    .filter((i) => i.transparency === "busy" && !i.all_day)
    .map((i) => [instantMs(i.start_utc), instantMs(i.end_utc)] as const)
    .sort((a, b) => a[0] - b[0]);
  const fromMs = instantMs(from);
  const toMs = instantMs(to);
  const slots: FreeSlot[] = [];
  let day = todayIn(tz, fromMs);
  for (let guard = 0; guard < 400; guard++) {
    const dayStart = toZoned(day, tz).toDate().getTime();
    if (dayStart >= toMs) break;
    const open = Math.max(fromMs, toZoned(day, tz).add({ hours: 8 }).toDate().getTime());
    const close = Math.min(toMs, toZoned(day, tz).add({ hours: 18 }).toDate().getTime());
    let cursor = open;
    for (const [start, end] of busy) {
      if (end <= cursor || start >= close) continue;
      if (start - cursor >= minutes * 60_000)
        slots.push({ start: utcString(cursor), end: utcString(start) });
      cursor = Math.max(cursor, end);
    }
    if (close - cursor >= minutes * 60_000)
      slots.push({ start: utcString(cursor), end: utcString(close) });
    day = day.add({ days: 1 });
  }
  return slots;
}

function suggestedStart(tz: string): { start: PimTime; end: PimTime } {
  const next = fromAbsolute(Date.now(), tz).add({ hours: 1 });
  const day = todayIn(tz, next.toDate().getTime());
  return {
    start: makePimTime(day, { hour: next.hour, minute: 0 }, tz),
    end: makePimTime(day, { hour: next.hour, minute: 30 }, tz),
  };
}

// ---------------------------------------------------------------------------
// Contacts

function sortName(fields: ContactFields): string {
  const family = fields.name?.family?.trim();
  const given = fields.name?.given?.trim();
  if (family) return `${family}, ${given ?? ""}`.trim();
  return fields.display_name || fields.organization?.name || "";
}

export function summaryOf(contact: Contact): ContactSummary {
  const f = contact.fields;
  const preferred = f.emails.find((e) => e.preferred) ?? f.emails[0];
  const phone = f.phones.find((p) => p.preferred) ?? f.phones[0];
  return {
    id: contact.id,
    address_book_id: contact.address_book_id,
    display_name: f.display_name,
    sort_name: sortName(f),
    kind: f.kind,
    email: preferred?.value ?? null,
    emails: f.emails.map((e) => e.value),
    phone: phone?.value ?? null,
    organization: f.organization?.name ?? null,
    has_photo: f.photo !== null,
    group_ids: [...contact.group_ids],
    linked_ids: [...contact.linked_ids],
    read_only: contact.read_only,
  };
}

function contactsChanged(bookIds: string[]): Parameters<MockCore["emit"]>[0] {
  return { type: "ContactsChanged", address_book_ids: [...new Set(bookIds)] };
}

const normalizePhone = (text: string) => text.replace(/[^\d+]/g, "");
const normalizeName = (text: string) =>
  text
    .toLowerCase()
    .replace(/\b[a-z]\.\s*/g, "")
    .replace(/\s+/g, " ")
    .trim();

export function duplicates(pim: PimMockState, bookIds: string[] | null): DuplicateGroup[] {
  const contacts = pim.contacts.filter((c) =>
    bookIds ? bookIds.includes(c.address_book_id) : true,
  );
  const parent = new Map<string, string>();
  const reasons = new Map<string, Set<DuplicateReason>>();
  const find = (id: string): string => {
    const p = parent.get(id) ?? id;
    if (p === id) return id;
    const root = find(p);
    parent.set(id, root);
    return root;
  };
  const union = (a: string, b: string, reason: DuplicateReason) => {
    const ra = find(a);
    const rb = find(b);
    const merged = new Set([...(reasons.get(ra) ?? []), ...(reasons.get(rb) ?? []), reason]);
    if (ra !== rb) parent.set(rb, ra);
    reasons.set(ra, merged);
  };
  const byKey = new Map<string, string>();
  for (const c of contacts) {
    // Contacts already merged (linked) are one person.
    if (contacts.some((o) => c.linked_ids.includes(o.id))) continue;
    const keys: [string, DuplicateReason][] = [
      ...c.fields.emails.map(
        (e) => [`email:${e.value.toLowerCase()}`, "email"] as [string, DuplicateReason],
      ),
      ...c.fields.phones.map(
        (p) => [`phone:${normalizePhone(p.value)}`, "phone"] as [string, DuplicateReason],
      ),
      [`name:${normalizeName(c.fields.display_name)}`, "name"],
    ];
    for (const [key, reason] of keys) {
      if (key.endsWith(":")) continue;
      const other = byKey.get(key);
      if (other) union(other, c.id, reason);
      else byKey.set(key, c.id);
    }
  }
  const groups = new Map<string, string[]>();
  for (const c of contacts) {
    if (!parent.has(c.id) && !reasons.has(c.id)) continue;
    const root = find(c.id);
    groups.set(root, [...(groups.get(root) ?? []), c.id]);
  }
  return [...groups.entries()]
    .filter(([, ids]) => ids.length > 1)
    .map(([root, ids]) => ({ contact_ids: ids, reasons: [...(reasons.get(root) ?? [])].sort() }));
}

function searchContacts(
  pim: PimMockState,
  query: string,
  bookIds: string[] | null,
  limit: number,
): ContactMatch[] {
  const q = query.trim().toLowerCase();
  if (!q) return [];
  const out: ContactMatch[] = [];
  for (const c of pim.contacts) {
    if (bookIds && !bookIds.includes(c.address_book_id)) continue;
    const f = c.fields;
    let match: ContactMatch | null = null;
    if (f.display_name.toLowerCase().includes(q) || sortName(f).toLowerCase().includes(q)) {
      match = { contact: summaryOf(c), field: "name", value: null };
    } else {
      const mail = f.emails.find((e) => e.value.toLowerCase().includes(q));
      const phone = f.phones.find(
        (p) => normalizePhone(p.value).includes(normalizePhone(q)) && normalizePhone(q),
      );
      if (mail) match = { contact: summaryOf(c), field: "email", value: mail.value };
      else if (phone) match = { contact: summaryOf(c), field: "phone", value: phone.value };
      else if (f.organization?.name.toLowerCase().includes(q)) {
        match = { contact: summaryOf(c), field: "organization", value: f.organization.name };
      } else if (f.notes?.toLowerCase().includes(q))
        match = { contact: summaryOf(c), field: "other", value: null };
    }
    if (match) out.push(match);
  }
  return out.sort((a, b) => a.contact.sort_name.localeCompare(b.contact.sort_name)).slice(0, limit);
}

// ---------------------------------------------------------------------------
// Tasks

function tasksChanged(listIds: string[]): Parameters<MockCore["emit"]>[0] {
  return { type: "TasksChanged", calendar_ids: [...new Set(listIds)] };
}

function descendants(pim: PimMockState, id: string): Task[] {
  const task = pim.tasks.find((t) => t.id === id);
  if (!task) return [];
  return task.subtask_ids.flatMap((sub) => {
    const child = pim.tasks.find((t) => t.id === sub);
    return child ? [child, ...descendants(pim, sub)] : [];
  });
}

function setParent(pim: PimMockState, task: Task, parentId: string | null): void {
  for (const other of pim.tasks) other.subtask_ids = other.subtask_ids.filter((s) => s !== task.id);
  task.fields.parent_id = parentId;
  const parent = parentId ? pim.tasks.find((t) => t.id === parentId) : undefined;
  if (parent) parent.subtask_ids.push(task.id);
}

/** Moves a recurring task's dates to its next occurrence. */
function advanceTask(task: Task): void {
  const rule = task.recurrence?.rules[0];
  if (!rule) return;
  const step = Math.max(1, rule.interval);
  const delta =
    rule.freq === "daily"
      ? { days: step }
      : rule.freq === "weekly"
        ? { days: 7 * step }
        : rule.freq === "monthly"
          ? { months: step }
          : rule.freq === "yearly"
            ? { years: step }
            : { days: 1 };
  const shift = (time: PimTime | null): PimTime | null => {
    if (!time) return null;
    if (time.type === "date") {
      const day = parseDay(time.date);
      return day ? { type: "date", date: dateString(day.add(delta)) } : time;
    }
    const local = parseLocal(time.local);
    if (!local) return time;
    return time.type === "floating"
      ? { type: "floating", local: localString(local.add(delta)) }
      : { type: "zoned", local: localString(local.add(delta)), tz: time.tz };
  };
  task.fields.due = shift(task.fields.due);
  task.fields.start = shift(task.fields.start);
}

// ---------------------------------------------------------------------------
// Import

interface ParsedItem {
  kind: "event" | "task" | "contact" | "group";
  props: Map<string, string>;
}

/** A minimal line-based reader of iCalendar / vCard text (unfolded lines). */
export function parseImport(content: string): {
  format: "icalendar" | "vcard" | null;
  items: ParsedItem[];
  name: string | null;
} {
  const lines = content.replace(/\r?\n[ \t]/g, "").split(/\r?\n/);
  const items: ParsedItem[] = [];
  let current: ParsedItem | null = null;
  let format: "icalendar" | "vcard" | null = null;
  let name: string | null = null;
  for (const line of lines) {
    const colon = line.indexOf(":");
    if (colon < 0) continue;
    const key = line.slice(0, colon).split(";")[0]?.toUpperCase() ?? "";
    const value = line
      .slice(colon + 1)
      .replace(/\\n/g, "\n")
      .replace(/\\([,;\\])/g, "$1");
    if (key === "BEGIN") {
      const what = value.toUpperCase();
      if (what === "VCALENDAR") format = "icalendar";
      else if (what === "VCARD") {
        format = "vcard";
        current = { kind: "contact", props: new Map() };
      } else if (what === "VEVENT") current = { kind: "event", props: new Map() };
      else if (what === "VTODO") current = { kind: "task", props: new Map() };
    } else if (key === "END") {
      const what = value.toUpperCase();
      if (current && ["VEVENT", "VTODO", "VCARD"].includes(what)) {
        if (current.kind === "contact" && current.props.get("KIND")?.toLowerCase() === "group")
          current.kind = "group";
        items.push(current);
        current = null;
      }
    } else if (current) {
      if (!current.props.has(key)) current.props.set(key, value);
    } else if (key === "X-WR-CALNAME") {
      name = value;
    }
  }
  return { format, items, name };
}

function icsTime(value: string | undefined, tz: string): PimTime | null {
  if (!value) return null;
  const m = /^(\d{4})(\d{2})(\d{2})(?:T(\d{2})(\d{2})(\d{2})(Z)?)?$/.exec(value.trim());
  if (!m) return null;
  const date = `${m[1]}-${m[2]}-${m[3]}`;
  if (!m[4]) return { type: "date", date };
  const local = `${date}T${m[4]}:${m[5]}:${m[6]}`;
  return m[7] ? { type: "zoned", local, tz: "UTC" } : { type: "zoned", local, tz };
}

function previewOf(content: string): PimImportPreview | null {
  const parsed = parseImport(content);
  if (!parsed.format) return null;
  const count = (kind: ParsedItem["kind"]) => parsed.items.filter((i) => i.kind === kind).length;
  return {
    format: parsed.format,
    name: parsed.name,
    events: count("event"),
    tasks: count("task"),
    contacts: count("contact"),
    groups: count("group"),
    sample: parsed.items
      .slice(0, 5)
      .map((i) => i.props.get("SUMMARY") ?? i.props.get("FN") ?? "")
      .filter(Boolean),
    issues: parsed.items
      .filter((i) => i.kind === "event" && !i.props.has("DTSTART"))
      .map((i) => ({
        line: null,
        item: i.props.get("SUMMARY") ?? null,
        message: "event without DTSTART",
        localized: { id: "pim-issue-invalid-date" },
      })),
  };
}

// ---------------------------------------------------------------------------
// Reminders

/** Fires a reminder now (tests and the dev mock): `ReminderDue` + state `fired`. */
export function fireReminder(core: MockCore, id: string): Reminder | null {
  const reminder = state(core).reminders.find((r) => r.id === id);
  if (!reminder) return null;
  reminder.state = "fired";
  reminder.snoozed_until = null;
  core.emit({ type: "ReminderDue", reminder: clone(reminder) });
  core.emit({ type: "RemindersChanged" });
  return clone(reminder);
}

// ---------------------------------------------------------------------------
// The answer

/** Answers a PIM request like the core will (ADR 0058). */
export function answerPim(core: MockCore, request: PimRequest): Response {
  const pim = state(core);
  if (pim.unsupported) {
    return errorResponse("unsupported", `${request.type} is not wired yet (M3a WP75)`);
  }
  switch (request.type) {
    // ---- Sources ----
    case "ListPimSources":
      return { type: "PimSources", sources: clone(pim.sources) };
    case "AddPimSource":
    case "UpdatePimSource":
      return saveSource(core, request.config, request.type === "AddPimSource");
    case "RemovePimSource": {
      if (request.source_id === "local")
        return errorResponse("invalid_request", "the local source stays");
      const source = pim.sources.find((s) => s.id === request.source_id);
      if (!source) return notFound(`source ${request.source_id}`);
      const calendars = pim.calendars.filter((c) => c.source_id === source.id).map((c) => c.id);
      const books = pim.addressBooks.filter((b) => b.source_id === source.id).map((b) => b.id);
      pim.sources = pim.sources.filter((s) => s !== source);
      pim.calendars = pim.calendars.filter((c) => !calendars.includes(c.id));
      pim.events = pim.events.filter((e) => !calendars.includes(e.calendar_id));
      pim.tasks = pim.tasks.filter((t) => !calendars.includes(t.calendar_id));
      pim.addressBooks = pim.addressBooks.filter((b) => !books.includes(b.id));
      pim.contacts = pim.contacts.filter((c) => !books.includes(c.address_book_id));
      pim.groups = pim.groups.filter((g) => !books.includes(g.address_book_id));
      core.data.accountsConfig.pim_sources = core.data.accountsConfig.pim_sources.filter(
        (s) => s.id !== source.id,
      );
      core.emit({ type: "PimSourcesChanged" });
      core.emit({ type: "CalendarsChanged" });
      core.emit({ type: "AddressBooksChanged" });
      return { type: "Done" };
    }
    case "TestPimSource": {
      const url =
        request.config.settings.kind === "ics"
          ? request.config.settings.url
          : (request.config.settings.url ?? "");
      if (/fail/.test(url)) {
        return {
          type: "PimSourceTested",
          result: {
            ok: false,
            discovered_url: null,
            collections: [],
            error: "auth_failed",
            message: "401 Unauthorized (mock)",
          },
        };
      }
      const kind = request.config.settings.kind;
      return {
        type: "PimSourceTested",
        result: {
          ok: true,
          discovered_url: url || "https://dav.nail.example/",
          collections:
            kind === "carddav"
              ? [
                  {
                    url: "https://dav.nail.example/contacts/",
                    name: "Contacts",
                    components: [],
                    address_book: true,
                    read_only: false,
                    color: null,
                  },
                ]
              : [
                  {
                    url: url || "https://dav.nail.example/calendars/home/",
                    name: kind === "ics" ? "Subscription" : "Home",
                    components: kind === "ics" ? ["event"] : ["event", "task"],
                    address_book: false,
                    read_only: kind === "ics",
                    color: null,
                  },
                ],
          error: null,
          message: null,
        },
      };
    }
    case "SyncPimSources": {
      const targets = pim.sources.filter(
        (s) => request.source_id === null || s.id === request.source_id,
      );
      if (request.source_id !== null && targets.length === 0)
        return notFound(`source ${request.source_id}`);
      for (const source of targets) source.state = source.enabled ? "syncing" : "disabled";
      core.emit({ type: "PimSourcesChanged" });
      core.later(() => {
        for (const source of targets) {
          if (source.enabled) {
            source.state = "idle";
            source.last_sync_at = now();
          }
        }
        core.emit({ type: "PimSourcesChanged" });
      });
      return { type: "Done" };
    }

    // ---- Calendars ----
    case "ListCalendars":
      return {
        type: "Calendars",
        calendars: clone(
          pim.calendars
            .filter((c) => request.source_id === null || c.source_id === request.source_id)
            .sort((a, b) => a.settings.order - b.settings.order),
        ),
      };
    case "CreateCalendar": {
      const source = pim.sources.find((s) => s.id === request.source_id);
      if (!source) return notFound(`source ${request.source_id}`);
      if (source.kind === "ics" || source.kind === "carddav") return forbidden("the source");
      if (!request.settings.name.trim()) return invalid("missing-name");
      const calendar: Calendar = {
        id: core.nextId("cal-"),
        source_id: source.id,
        account_id: source.account_id,
        settings: clone(request.settings),
        components: request.components.length > 0 ? [...request.components] : ["event"],
        read_only: false,
        timezone: null,
      };
      pim.calendars.push(calendar);
      const op_ids = queue(core, source.id, [{ type: "CalendarsChanged" }]);
      return { type: "CalendarSaved", calendar: clone(calendar), op_ids };
    }
    case "UpdateCalendar": {
      const calendar = calendarOf(pim, request.calendar_id);
      if (!calendar) return notFound(`calendar ${request.calendar_id}`);
      if (!request.settings.name.trim()) return invalid("missing-name");
      calendar.settings = clone(request.settings);
      const op_ids = queue(core, calendar.source_id, [
        { type: "CalendarsChanged" },
        eventsChanged([calendar.id]),
        tasksChanged([calendar.id]),
      ]);
      return { type: "CalendarSaved", calendar: clone(calendar), op_ids };
    }
    case "DeleteCalendar": {
      const calendar = calendarOf(pim, request.calendar_id);
      if (!calendar) return notFound(`calendar ${request.calendar_id}`);
      if (calendar.read_only) return forbidden("the calendar");
      pim.calendars = pim.calendars.filter((c) => c !== calendar);
      pim.events = pim.events.filter((e) => e.calendar_id !== calendar.id);
      pim.tasks = pim.tasks.filter((t) => t.calendar_id !== calendar.id);
      const op_ids = queue(core, calendar.source_id, [
        { type: "CalendarsChanged" },
        eventsChanged([calendar.id]),
      ]);
      return { type: "PimDeleted", op_ids };
    }

    // ---- Events ----
    case "ListEventInstances":
      return {
        type: "EventInstances",
        from: request.from,
        to: request.to,
        instances: listInstances(pim, request.from, request.to, request.calendar_ids, request.tz),
      };
    case "GetEvent": {
      const event = pim.events.find((e) => e.id === request.event_id);
      return event
        ? { type: "PimEvent", event: clone(event) }
        : notFound(`event ${request.event_id}`);
    }
    case "CreateEvent": {
      const calendar = calendarOf(pim, request.calendar_id);
      if (!calendar) return notFound(`calendar ${request.calendar_id}`);
      if (calendar.read_only) return forbidden("the calendar");
      const issue = eventIssue(request.fields, request.recurrence);
      if (issue) return invalid(issue);
      const id = core.nextId("ev-");
      const event: CalendarEvent = {
        id,
        calendar_id: calendar.id,
        uid: `${id}@nail.example`,
        fields: clone(request.fields),
        recurrence: request.recurrence ? clone(request.recurrence) : null,
        overrides: [],
        sequence: 0,
        created_at: now(),
        updated_at: now(),
        read_only: false,
        sync: "pending",
      };
      pim.events.push(event);
      const op_ids = queue(core, calendar.source_id, [eventsChanged([calendar.id])]);
      return { type: "EventSaved", event: clone(event), split: null, op_ids };
    }
    case "UpdateEvent":
      return updateEvent(core, request);
    case "DeleteEvent":
      return deleteEvent(core, request);
    case "MoveEvent": {
      const event = pim.events.find((e) => e.id === request.event_id);
      const target = calendarOf(pim, request.calendar_id);
      if (!event) return notFound(`event ${request.event_id}`);
      if (!target) return notFound(`calendar ${request.calendar_id}`);
      if (target.read_only || calendarOf(pim, event.calendar_id)?.read_only)
        return forbidden("the calendar");
      const from = event.calendar_id;
      event.calendar_id = target.id;
      event.sync = "pending";
      const op_ids = queue(core, target.source_id, [eventsChanged([from, target.id])]);
      return { type: "EventSaved", event: clone(event), split: null, op_ids };
    }
    case "EventFromMessage": {
      const message = core.data.messages.find((m) => m.id === request.message_id);
      if (!message) return notFound(`message ${request.message_id}`);
      const { start, end } = suggestedStart(localZone());
      const fields = emptyEventFields(start, end);
      fields.title = message.subject;
      fields.attendees = [...(message.from ? [message.from] : []), ...message.to].map((a) => ({
        email: a.email,
        name: a.name,
        role: "required",
        status: "needs-action",
        kind: "individual",
        rsvp: true,
        delegated_to: [],
        delegated_from: [],
      }));
      fields.links = [
        { type: "message", message_id: message.id, rfc822_id: null, subject: message.subject },
      ];
      return { type: "EventSuggested", fields };
    }
    case "FindFreeTime":
      return {
        type: "FreeTime",
        slots: freeTime(
          pim,
          request.from,
          request.to,
          request.duration_minutes,
          request.calendar_ids,
          request.tz,
        ),
      };

    // ---- Address books ----
    case "ListAddressBooks":
      return {
        type: "AddressBooks",
        address_books: clone(
          pim.addressBooks
            .filter((b) => request.source_id === null || b.source_id === request.source_id)
            .sort((a, b) => a.settings.order - b.settings.order),
        ),
      };
    case "CreateAddressBook": {
      const source = pim.sources.find((s) => s.id === request.source_id);
      if (!source) return notFound(`source ${request.source_id}`);
      if (source.kind === "ics" || source.kind === "caldav") return forbidden("the source");
      if (!request.settings.name.trim()) return invalid("missing-name");
      const book: AddressBook = {
        id: core.nextId("ab-"),
        source_id: source.id,
        account_id: source.account_id,
        settings: clone(request.settings),
        read_only: false,
        role: null,
      };
      pim.addressBooks.push(book);
      const op_ids = queue(core, source.id, [{ type: "AddressBooksChanged" }]);
      return { type: "AddressBookSaved", address_book: clone(book), op_ids };
    }
    case "UpdateAddressBook": {
      const book = bookOf(pim, request.address_book_id);
      if (!book) return notFound(`address book ${request.address_book_id}`);
      if (!request.settings.name.trim()) return invalid("missing-name");
      book.settings = clone(request.settings);
      const op_ids = queue(core, book.source_id, [
        { type: "AddressBooksChanged" },
        contactsChanged([book.id]),
      ]);
      return { type: "AddressBookSaved", address_book: clone(book), op_ids };
    }
    case "DeleteAddressBook": {
      const book = bookOf(pim, request.address_book_id);
      if (!book) return notFound(`address book ${request.address_book_id}`);
      if (book.read_only || book.role === "collected") return forbidden("the address book");
      pim.addressBooks = pim.addressBooks.filter((b) => b !== book);
      pim.contacts = pim.contacts.filter((c) => c.address_book_id !== book.id);
      pim.groups = pim.groups.filter((g) => g.address_book_id !== book.id);
      const op_ids = queue(core, book.source_id, [
        { type: "AddressBooksChanged" },
        contactsChanged([book.id]),
      ]);
      return { type: "PimDeleted", op_ids };
    }

    // ---- Contacts ----
    case "ListContacts": {
      const visible = pim.addressBooks.filter((b) => b.settings.visible).map((b) => b.id);
      const books = request.address_book_ids ?? visible;
      const group = request.group_id ? pim.groups.find((g) => g.id === request.group_id) : null;
      if (request.group_id && !group) return notFound(`group ${request.group_id}`);
      const all = pim.contacts
        .filter((c) => books.includes(c.address_book_id))
        .filter((c) => !group || group.member_ids.includes(c.id))
        .map(summaryOf)
        .sort((a, b) => a.sort_name.localeCompare(b.sort_name));
      return {
        type: "Contacts",
        total: all.length,
        offset: request.offset,
        contacts: all.slice(request.offset, request.offset + request.limit),
      };
    }
    case "GetContact": {
      const contact = pim.contacts.find((c) => c.id === request.contact_id);
      return contact
        ? { type: "PimContact", contact: clone(contact) }
        : notFound(`contact ${request.contact_id}`);
    }
    case "CreateContact": {
      const book = bookOf(pim, request.address_book_id);
      if (!book) return notFound(`address book ${request.address_book_id}`);
      if (book.read_only) return forbidden("the address book");
      const issue = contactIssue(request.fields);
      if (issue) return invalid(issue);
      const id = core.nextId("ct-");
      const contact: Contact = {
        id,
        address_book_id: book.id,
        uid: `urn:uuid:${id}`,
        fields: clone(request.fields),
        version: "4.0",
        group_ids: [],
        linked_ids: [],
        updated_at: now(),
        read_only: false,
        sync: "pending",
      };
      pim.contacts.push(contact);
      const op_ids = queue(core, book.source_id, [contactsChanged([book.id])]);
      return { type: "ContactSaved", contact: clone(contact), op_ids };
    }
    case "UpdateContact": {
      const contact = pim.contacts.find((c) => c.id === request.contact_id);
      if (!contact) return notFound(`contact ${request.contact_id}`);
      if (contact.read_only) return forbidden("the contact");
      const issue = contactIssue(request.fields);
      if (issue) return invalid(issue);
      contact.fields = clone(request.fields);
      contact.updated_at = now();
      contact.sync = "pending";
      const op_ids = queue(core, sourceOfBook(pim, contact.address_book_id), [
        contactsChanged([contact.address_book_id]),
      ]);
      return { type: "ContactSaved", contact: clone(contact), op_ids };
    }
    case "DeleteContacts": {
      const gone = pim.contacts.filter((c) => request.contact_ids.includes(c.id));
      if (gone.length === 0) return notFound("contacts");
      if (gone.some((c) => c.read_only)) return forbidden("the contact");
      pim.contacts = pim.contacts.filter((c) => !request.contact_ids.includes(c.id));
      for (const c of pim.contacts)
        c.linked_ids = c.linked_ids.filter((l) => !request.contact_ids.includes(l));
      for (const g of pim.groups)
        g.member_ids = g.member_ids.filter((m) => !request.contact_ids.includes(m));
      const books = gone.map((c) => c.address_book_id);
      const op_ids = queue(core, sourceOfBook(pim, books[0] ?? ""), [contactsChanged(books)]);
      return { type: "PimDeleted", op_ids };
    }
    case "MoveContact": {
      const contact = pim.contacts.find((c) => c.id === request.contact_id);
      const target = bookOf(pim, request.address_book_id);
      if (!contact) return notFound(`contact ${request.contact_id}`);
      if (!target) return notFound(`address book ${request.address_book_id}`);
      if (target.read_only) return forbidden("the address book");
      const from = contact.address_book_id;
      contact.address_book_id = target.id;
      for (const g of pim.groups) {
        if (g.address_book_id !== target.id)
          g.member_ids = g.member_ids.filter((m) => m !== contact.id);
      }
      contact.group_ids = pim.groups
        .filter((g) => g.member_ids.includes(contact.id))
        .map((g) => g.id);
      const op_ids = queue(core, target.source_id, [contactsChanged([from, target.id])]);
      return { type: "ContactSaved", contact: clone(contact), op_ids };
    }
    case "SearchContacts":
      return {
        type: "ContactMatches",
        matches: searchContacts(pim, request.query, request.address_book_ids, request.limit),
      };
    case "FindDuplicateContacts":
      return { type: "ContactDuplicates", groups: duplicates(pim, request.address_book_ids) };
    case "MergeContacts": {
      const primary = pim.contacts.find((c) => c.id === request.primary_id);
      if (!primary) return notFound(`contact ${request.primary_id}`);
      const issue = contactIssue(request.fields);
      if (issue) return invalid(issue);
      const others = pim.contacts.filter(
        (c) => request.contact_ids.includes(c.id) && c.id !== primary.id,
      );
      const folded = others.filter((c) => c.address_book_id === primary.address_book_id);
      const linked = others.filter((c) => c.address_book_id !== primary.address_book_id);
      primary.fields = clone(request.fields);
      for (const c of folded) {
        for (const g of pim.groups) {
          if (g.member_ids.includes(c.id)) {
            g.member_ids = [...new Set(g.member_ids.map((m) => (m === c.id ? primary.id : m)))];
          }
        }
        primary.group_ids = [...new Set([...primary.group_ids, ...c.group_ids])];
      }
      const foldedIds = folded.map((c) => c.id);
      pim.contacts = pim.contacts.filter((c) => !foldedIds.includes(c.id));
      for (const c of linked) {
        if (!primary.linked_ids.includes(c.id)) primary.linked_ids.push(c.id);
        if (!c.linked_ids.includes(primary.id)) c.linked_ids.push(primary.id);
      }
      primary.updated_at = now();
      primary.sync = "pending";
      const books = [primary.address_book_id, ...others.map((c) => c.address_book_id)];
      const op_ids = queue(core, sourceOfBook(pim, primary.address_book_id), [
        contactsChanged(books),
      ]);
      return { type: "ContactSaved", contact: clone(primary), op_ids };
    }
    case "UnlinkContact": {
      const contact = pim.contacts.find((c) => c.id === request.contact_id);
      if (!contact) return notFound(`contact ${request.contact_id}`);
      for (const other of pim.contacts)
        other.linked_ids = other.linked_ids.filter((l) => l !== contact.id);
      const books = [
        contact.address_book_id,
        ...contact.linked_ids.map(
          (l) => pim.contacts.find((c) => c.id === l)?.address_book_id ?? "",
        ),
      ];
      contact.linked_ids = [];
      const op_ids = queue(core, sourceOfBook(pim, contact.address_book_id), [
        contactsChanged(books.filter(Boolean)),
      ]);
      return { type: "ContactSaved", contact: clone(contact), op_ids };
    }
    case "GetContactPhoto": {
      const contact = pim.contacts.find((c) => c.id === request.contact_id);
      const photo = contact?.fields.photo;
      if (!contact || !photo?.data) return notFound(`photo of ${request.contact_id}`);
      return {
        type: "ContactPhoto",
        contact_id: contact.id,
        mime_type: photo.mime_type ?? "image/jpeg",
        data: photo.data,
      };
    }

    // ---- Groups ----
    case "ListContactGroups":
      return {
        type: "ContactGroups",
        groups: clone(
          pim.groups
            .filter(
              (g) =>
                !request.address_book_ids || request.address_book_ids.includes(g.address_book_id),
            )
            .sort((a, b) => a.name.localeCompare(b.name)),
        ),
      };
    case "CreateContactGroup":
    case "UpdateContactGroup": {
      if (!request.name.trim()) return invalid("missing-name");
      if (request.member_emails.some((e) => !plausibleEmail(e))) return invalid("invalid-email");
      let group: ContactGroup | undefined;
      if (request.type === "CreateContactGroup") {
        const book = bookOf(pim, request.address_book_id);
        if (!book) return notFound(`address book ${request.address_book_id}`);
        if (book.read_only) return forbidden("the address book");
        const id = core.nextId("grp-");
        group = {
          id,
          address_book_id: book.id,
          uid: `${id}@nail.example`,
          name: "",
          member_ids: [],
          member_emails: [],
          read_only: false,
          sync: "pending",
        };
        pim.groups.push(group);
      } else {
        group = pim.groups.find((g) => g.id === request.group_id);
        if (!group) return notFound(`group ${request.group_id}`);
        if (group.read_only) return forbidden("the group");
      }
      const target = group;
      const members = request.member_ids.filter((m) =>
        pim.contacts.some((c) => c.id === m && c.address_book_id === target.address_book_id),
      );
      target.name = request.name.trim();
      target.member_ids = [...new Set(members)];
      target.member_emails = [...new Set(request.member_emails)];
      target.sync = "pending";
      for (const c of pim.contacts) {
        const inGroup = target.member_ids.includes(c.id);
        c.group_ids = inGroup
          ? [...new Set([...c.group_ids, target.id])]
          : c.group_ids.filter((g) => g !== target.id);
      }
      const op_ids = queue(core, sourceOfBook(pim, target.address_book_id), [
        contactsChanged([target.address_book_id]),
      ]);
      return { type: "ContactGroupSaved", group: clone(target), op_ids };
    }
    case "DeleteContactGroup": {
      const group = pim.groups.find((g) => g.id === request.group_id);
      if (!group) return notFound(`group ${request.group_id}`);
      if (group.read_only) return forbidden("the group");
      pim.groups = pim.groups.filter((g) => g !== group);
      for (const c of pim.contacts) c.group_ids = c.group_ids.filter((g) => g !== group.id);
      const op_ids = queue(core, sourceOfBook(pim, group.address_book_id), [
        contactsChanged([group.address_book_id]),
      ]);
      return { type: "PimDeleted", op_ids };
    }

    // ---- Tasks ----
    case "ListTasks": {
      const lists = request.calendar_ids ?? visibleCalendarIds(pim, "task");
      return {
        type: "Tasks",
        tasks: clone(
          pim.tasks
            .filter((t) => lists.includes(t.calendar_id))
            .filter(
              (t) =>
                request.include_completed ||
                (t.fields.status !== "completed" && t.fields.status !== "cancelled"),
            ),
        ),
      };
    }
    case "GetTask": {
      const task = pim.tasks.find((t) => t.id === request.task_id);
      return task ? { type: "PimTask", task: clone(task) } : notFound(`task ${request.task_id}`);
    }
    case "CreateTask": {
      const list = calendarOf(pim, request.calendar_id);
      if (!list) return notFound(`task list ${request.calendar_id}`);
      if (list.read_only) return forbidden("the task list");
      const issue = taskIssue(request.fields);
      if (issue) return invalid(issue);
      const parent = request.fields.parent_id
        ? pim.tasks.find((t) => t.id === request.fields.parent_id)
        : null;
      if (request.fields.parent_id && (!parent || parent.calendar_id !== list.id))
        return notFound("parent task");
      const id = core.nextId("tk-");
      const task: Task = {
        id,
        calendar_id: list.id,
        uid: `${id}@nail.example`,
        fields: clone(request.fields),
        recurrence: request.recurrence ? clone(request.recurrence) : null,
        subtask_ids: [],
        created_at: now(),
        updated_at: now(),
        read_only: false,
        sync: "pending",
      };
      pim.tasks.push(task);
      if (parent) parent.subtask_ids.push(id);
      const op_ids = queue(core, list.source_id, [tasksChanged([list.id])]);
      return { type: "TaskSaved", task: clone(task), op_ids };
    }
    case "UpdateTask": {
      const task = pim.tasks.find((t) => t.id === request.task_id);
      if (!task) return notFound(`task ${request.task_id}`);
      if (task.read_only || calendarOf(pim, task.calendar_id)?.read_only)
        return forbidden("the task list");
      const issue = taskIssue(request.fields);
      if (issue) return invalid(issue);
      const parentId = request.fields.parent_id;
      if (parentId !== null) {
        if (parentId === task.id || descendants(pim, task.id).some((d) => d.id === parentId)) {
          return invalid("task-cycle");
        }
        if (!pim.tasks.some((t) => t.id === parentId && t.calendar_id === task.calendar_id)) {
          return notFound("parent task");
        }
      }
      const wasParent = task.fields.parent_id;
      task.fields = clone(request.fields);
      if (wasParent !== parentId) setParent(pim, task, parentId);
      task.recurrence = request.recurrence ? clone(request.recurrence) : null;
      task.updated_at = now();
      task.sync = "pending";
      const op_ids = queue(core, sourceOfCalendar(pim, task.calendar_id), [
        tasksChanged([task.calendar_id]),
      ]);
      return { type: "TaskSaved", task: clone(task), op_ids };
    }
    case "CompleteTask": {
      const task = pim.tasks.find((t) => t.id === request.task_id);
      if (!task) return notFound(`task ${request.task_id}`);
      if (task.read_only || calendarOf(pim, task.calendar_id)?.read_only)
        return forbidden("the task list");
      if (request.completed && task.recurrence) {
        advanceTask(task);
        task.fields.status = "needs-action";
        task.fields.percent_complete = null;
        task.fields.completed_at = null;
      } else if (request.completed) {
        task.fields.status = "completed";
        task.fields.percent_complete = 100;
        task.fields.completed_at = now();
      } else {
        task.fields.status = "needs-action";
        task.fields.percent_complete = null;
        task.fields.completed_at = null;
      }
      task.updated_at = now();
      task.sync = "pending";
      const op_ids = queue(core, sourceOfCalendar(pim, task.calendar_id), [
        tasksChanged([task.calendar_id]),
      ]);
      return { type: "TaskSaved", task: clone(task), op_ids };
    }
    case "MoveTask": {
      const task = pim.tasks.find((t) => t.id === request.task_id);
      const target = calendarOf(pim, request.calendar_id);
      if (!task) return notFound(`task ${request.task_id}`);
      if (!target || !target.components.includes("task"))
        return notFound(`task list ${request.calendar_id}`);
      if (target.read_only) return forbidden("the task list");
      const from = task.calendar_id;
      setParent(pim, task, null);
      for (const t of [task, ...descendants(pim, task.id)]) t.calendar_id = target.id;
      const op_ids = queue(core, target.source_id, [tasksChanged([from, target.id])]);
      return { type: "TaskSaved", task: clone(task), op_ids };
    }
    case "DeleteTask": {
      const task = pim.tasks.find((t) => t.id === request.task_id);
      if (!task) return notFound(`task ${request.task_id}`);
      if (task.read_only || calendarOf(pim, task.calendar_id)?.read_only)
        return forbidden("the task list");
      const gone = request.with_subtasks ? [task, ...descendants(pim, task.id)] : [task];
      if (!request.with_subtasks) {
        for (const sub of task.subtask_ids) {
          const child = pim.tasks.find((t) => t.id === sub);
          if (child) setParent(pim, child, task.fields.parent_id);
        }
      }
      const ids = gone.map((t) => t.id);
      pim.tasks = pim.tasks.filter((t) => !ids.includes(t.id));
      for (const t of pim.tasks) t.subtask_ids = t.subtask_ids.filter((s) => !ids.includes(s));
      pim.reminders = pim.reminders.filter(
        (r) => !(r.target.type === "task" && ids.includes(r.target.task_id)),
      );
      const op_ids = queue(core, sourceOfCalendar(pim, task.calendar_id), [
        tasksChanged([task.calendar_id]),
      ]);
      return { type: "PimDeleted", op_ids };
    }
    case "TaskFromMessage": {
      const message = core.data.messages.find((m) => m.id === request.message_id);
      if (!message) return notFound(`message ${request.message_id}`);
      const fields = emptyTaskFields(message.subject);
      fields.due = { type: "date", date: dateString(todayIn(localZone()).add({ days: 1 })) };
      fields.links = [
        { type: "message", message_id: message.id, rfc822_id: null, subject: message.subject },
      ];
      return { type: "TaskSuggested", fields };
    }

    // ---- Other ----
    case "SearchPim": {
      const q = request.query.trim().toLowerCase();
      const want = (kind: string) =>
        request.kinds.length === 0 || (request.kinds as string[]).includes(kind);
      const hits: PimSearchHit[] = [];
      if (q) {
        if (want("event")) {
          for (const e of pim.events) {
            const f = e.fields;
            if (
              f.title.toLowerCase().includes(q) ||
              f.location?.toLowerCase().includes(q) ||
              f.attendees.some(
                (a) => a.email.toLowerCase().includes(q) || a.name?.toLowerCase().includes(q),
              )
            ) {
              hits.push({
                item: { type: "event", event_id: e.id },
                title: f.title,
                detail: f.location,
                calendar_id: e.calendar_id,
                address_book_id: null,
                start: f.start,
              });
            }
          }
        }
        if (want("task")) {
          for (const t of pim.tasks) {
            if (t.fields.title.toLowerCase().includes(q)) {
              hits.push({
                item: { type: "task", task_id: t.id },
                title: t.fields.title,
                detail: null,
                calendar_id: t.calendar_id,
                address_book_id: null,
                start: t.fields.due,
              });
            }
          }
        }
        if (want("contact")) {
          for (const m of searchContacts(pim, q, null, request.limit)) {
            hits.push({
              item: { type: "contact", contact_id: m.contact.id },
              title: m.contact.display_name,
              detail: m.contact.email,
              calendar_id: null,
              address_book_id: m.contact.address_book_id,
              start: null,
            });
          }
        }
        if (want("contact-group")) {
          for (const g of pim.groups) {
            if (g.name.toLowerCase().includes(q)) {
              hits.push({
                item: { type: "contact-group", group_id: g.id },
                title: g.name,
                detail: null,
                calendar_id: null,
                address_book_id: g.address_book_id,
                start: null,
              });
            }
          }
        }
      }
      return { type: "PimSearchResults", hits: hits.slice(0, request.limit) };
    }
    case "PreviewPimImport": {
      if (request.source.type === "file") return notFound(`file ${request.source.path}`);
      const preview = previewOf(request.source.content);
      return preview
        ? { type: "PimImportPreview", preview }
        : errorResponse("invalid_request", "neither iCalendar nor vCard");
    }
    case "ImportPim":
      return importPim(core, request);
    case "ResolvePimConflict": {
      const item = request.item;
      const target =
        item.type === "event"
          ? pim.events.find((e) => e.id === item.event_id)
          : item.type === "task"
            ? pim.tasks.find((t) => t.id === item.task_id)
            : item.type === "contact"
              ? pim.contacts.find((c) => c.id === item.contact_id)
              : pim.groups.find((g) => g.id === item.group_id);
      if (!target) return notFound("item");
      target.sync = request.keep === "local" ? "pending" : "synced";
      core.emit({ type: "EventsChanged", calendar_ids: [] });
      return { type: "Done" };
    }

    // ---- Reminders ----
    case "ListReminders": {
      const until = request.until ? instantMs(request.until) : Number.POSITIVE_INFINITY;
      return {
        type: "Reminders",
        reminders: clone(
          pim.reminders
            .filter((r) => r.state !== "dismissed")
            .filter(
              (r) =>
                instantMs(r.state === "snoozed" && r.snoozed_until ? r.snoozed_until : r.fire_at) <=
                until,
            )
            .sort((a, b) => instantMs(a.fire_at) - instantMs(b.fire_at)),
        ),
      };
    }
    case "SnoozeReminders": {
      const targets = pim.reminders.filter((r) => request.reminder_ids.includes(r.id));
      if (targets.length === 0) return notFound("reminders");
      for (const r of targets) {
        r.state = "snoozed";
        r.snoozed_until = request.until;
      }
      core.emit({ type: "RemindersChanged" });
      return { type: "Done" };
    }
    case "DismissReminders": {
      const targets = pim.reminders.filter((r) => request.reminder_ids.includes(r.id));
      if (targets.length === 0) return notFound("reminders");
      for (const r of targets) {
        r.state = "dismissed";
        r.snoozed_until = null;
      }
      core.emit({ type: "RemindersChanged" });
      return { type: "Done" };
    }
  }
}

function saveSource(core: MockCore, config: PimSourceConfig, add: boolean): Response {
  const pim = state(core);
  const id = config.id.trim();
  if (!id || id === "local" || !/^[a-z0-9][a-z0-9-]*$/.test(id)) {
    return {
      type: "Error",
      code: "invalid_request",
      message: "bad source id",
      localized: { id: "pim-source-issue-invalid-id" },
    };
  }
  const url = config.settings.kind === "ics" ? config.settings.url : config.settings.url;
  const account = config.settings.kind === "ics" ? null : config.settings.account;
  if (config.settings.kind === "ics" && !/^(https|webcals?):\/\/\S+$/i.test(config.settings.url)) {
    return {
      type: "Error",
      code: "invalid_request",
      message: "bad url",
      localized: { id: "pim-source-issue-invalid-url" },
    };
  }
  if (config.settings.kind !== "ics" && !account && !url) {
    return {
      type: "Error",
      code: "invalid_request",
      message: "missing url",
      localized: { id: "pim-source-issue-missing-url" },
    };
  }
  const existing = pim.sources.find((s) => s.id === id);
  if (add && existing) return errorResponse("conflict", `source ${id} exists`);
  if (!add && !existing) return notFound(`source ${id}`);
  const source: PimSource = {
    id,
    kind: config.settings.kind,
    name: config.name ?? id,
    account_id: account,
    enabled: config.enabled,
    config: clone(config),
    state: config.enabled ? "syncing" : "disabled",
    last_sync_at: existing?.last_sync_at ?? null,
    error: null,
    message: null,
  };
  if (existing) Object.assign(existing, source);
  else pim.sources.push(source);
  const configs = core.data.accountsConfig.pim_sources.filter((s) => s.id !== id);
  core.data.accountsConfig.pim_sources = [...configs, clone(config)];
  if (add && config.settings.kind !== "carddav") {
    pim.calendars.push({
      id: core.nextId("cal-"),
      source_id: id,
      account_id: account,
      settings: {
        name: config.name ?? id,
        description: null,
        color: config.settings.kind === "ics" ? config.settings.color : null,
        visible: true,
        reminders: config.settings.kind !== "ics" || config.settings.alarms,
        default_alarms: [],
        order: pim.calendars.length,
      },
      components: config.settings.kind === "ics" ? ["event"] : ["event", "task"],
      read_only: config.settings.kind === "ics",
      timezone: null,
    });
  }
  if (add && config.settings.kind === "carddav") {
    pim.addressBooks.push({
      id: core.nextId("ab-"),
      source_id: id,
      account_id: account,
      settings: {
        name: config.name ?? id,
        description: null,
        visible: true,
        autocomplete: true,
        order: pim.addressBooks.length,
      },
      read_only: false,
      role: null,
    });
  }
  core.emit({ type: "PimSourcesChanged" });
  core.emit({ type: "CalendarsChanged" });
  core.emit({ type: "AddressBooksChanged" });
  const saved = pim.sources.find((s) => s.id === id) as PimSource;
  core.later(() => {
    if (saved.enabled) {
      saved.state = "idle";
      saved.last_sync_at = now();
    }
    core.emit({ type: "PimSourcesChanged" });
  });
  return { type: "PimSourceSaved", source: clone(saved) };
}

function importPim(core: MockCore, request: Extract<PimRequest, { type: "ImportPim" }>): Response {
  const pim = state(core);
  if (request.source.type === "file") return notFound(`file ${request.source.path}`);
  const parsed = parseImport(request.source.content);
  if (!parsed.format) return errorResponse("invalid_request", "neither iCalendar nor vCard");
  const tz = localZone();
  let created = 0;
  let skipped = 0;
  const calendar = request.calendar_id
    ? calendarOf(pim, request.calendar_id)
    : pim.calendars.find((c) => c.source_id === "local" && c.components.includes("event"));
  const list =
    request.calendar_id && calendar?.components.includes("task")
      ? calendar
      : pim.calendars.find((c) => c.components.includes("task") && !c.read_only);
  const book = request.address_book_id
    ? bookOf(pim, request.address_book_id)
    : pim.addressBooks.find((b) => b.source_id === "local" && b.role === null);
  if (calendar?.read_only || book?.read_only) return forbidden("the target");
  const touched = { events: new Set<string>(), tasks: new Set<string>(), books: new Set<string>() };
  for (const item of parsed.items) {
    const props = item.props;
    if (item.kind === "event" && calendar) {
      const start = icsTime(props.get("DTSTART"), tz);
      if (!start) {
        skipped++;
        continue;
      }
      const end =
        icsTime(props.get("DTEND"), tz) ??
        (start.type === "date"
          ? {
              type: "date" as const,
              date: dateString((parseDay(start.date) ?? todayIn(tz)).add({ days: 1 })),
            }
          : endFor({ ...emptyEventFields(start, start) }, start));
      const fields = emptyEventFields(start, end);
      fields.title = props.get("SUMMARY") ?? "";
      fields.location = props.get("LOCATION") ?? null;
      fields.description = props.get("DESCRIPTION") ?? null;
      const id = core.nextId("ev-");
      pim.events.push({
        id,
        calendar_id: calendar.id,
        uid: props.get("UID") ?? `${id}@nail.example`,
        fields,
        recurrence: null,
        overrides: [],
        sequence: 0,
        created_at: now(),
        updated_at: now(),
        read_only: false,
        sync: "pending",
      });
      touched.events.add(calendar.id);
      created++;
    } else if (item.kind === "task" && list) {
      const fields = emptyTaskFields(props.get("SUMMARY") ?? "");
      fields.due = icsTime(props.get("DUE"), tz);
      const id = core.nextId("tk-");
      pim.tasks.push({
        id,
        calendar_id: list.id,
        uid: props.get("UID") ?? `${id}@nail.example`,
        fields,
        recurrence: null,
        subtask_ids: [],
        created_at: now(),
        updated_at: now(),
        read_only: false,
        sync: "pending",
      });
      touched.tasks.add(list.id);
      created++;
    } else if (item.kind === "contact" && book) {
      const fields = emptyContactFields(props.get("FN") ?? "");
      const mail = props.get("EMAIL");
      if (mail) fields.emails = [{ value: mail, kinds: [], label: null, preferred: true }];
      const tel = props.get("TEL");
      if (tel) fields.phones = [{ value: tel, kinds: [], label: null, preferred: true }];
      if (contactIssue(fields)) {
        skipped++;
        continue;
      }
      const id = core.nextId("ct-");
      pim.contacts.push({
        id,
        address_book_id: book.id,
        uid: props.get("UID") ?? `urn:uuid:${id}`,
        fields,
        version: props.get("VERSION") === "3.0" ? "3.0" : "4.0",
        group_ids: [],
        linked_ids: [],
        updated_at: now(),
        read_only: false,
        sync: "pending",
      });
      touched.books.add(book.id);
      created++;
    } else {
      skipped++;
    }
  }
  const changed: Parameters<MockCore["emit"]>[0][] = [];
  if (touched.events.size > 0) changed.push(eventsChanged([...touched.events]));
  if (touched.tasks.size > 0) changed.push(tasksChanged([...touched.tasks]));
  if (touched.books.size > 0) changed.push(contactsChanged([...touched.books]));
  const op_ids =
    created > 0 ? queue(core, calendar?.source_id ?? book?.source_id ?? "local", changed) : [];
  return { type: "PimImported", result: { created, updated: 0, skipped, issues: [], op_ids } };
}

/** The instant of a `PimTime` in a zone (exported for tests). */
export function instantOfTime(time: PimTime, tz: string): number {
  return pimTimeIn(time, tz).toDate().getTime();
}
