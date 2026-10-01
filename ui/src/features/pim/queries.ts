/**
 * Queries and writes of the PIM feature. Every read goes through
 * `NailClient`; every write is a core request that the core queues (the
 * PIM operation queue) and answers with the saved item — the UI refetches
 * what shows it (and the core's change events do the same for writes made
 * elsewhere).
 */
import { keepPreviousData, useQuery, type QueryClient } from "@tanstack/react-query";
import type {
  AddressBookId,
  CalendarEvent,
  CalendarId,
  ContactGroupId,
  ContactId,
  EditScope,
  EventFields,
  EventInstance,
  PimTime,
  Recurrence,
  ReminderId,
  Request,
  Response,
  TaskId,
} from "../../api/generated";
import type { NailClient } from "../../api/client";
import { useClient } from "../../state/queries";

export const pimKeys = {
  all: ["pim"] as const,
  sources: ["pim", "sources"] as const,
  calendars: ["pim", "calendars"] as const,
  instances: (from: string, to: string, ids: readonly string[] | null, tz: string) =>
    ["pim", "instances", from, to, ids, tz] as const,
  freeTime: (from: string, to: string, minutes: number, tz: string) =>
    ["pim", "free-time", from, to, minutes, tz] as const,
  event: (id: string | null) => ["pim", "event", id] as const,
  addressBooks: ["pim", "address-books"] as const,
  contacts: (books: readonly string[] | null, group: string | null, limit: number) =>
    ["pim", "contacts", books, group, limit] as const,
  contactSearch: (query: string, books: readonly string[] | null) =>
    ["pim", "contact-search", query, books] as const,
  contact: (id: string | null) => ["pim", "contact", id] as const,
  photo: (id: string | null) => ["pim", "photo", id] as const,
  groups: ["pim", "groups"] as const,
  duplicates: ["pim", "duplicates"] as const,
  tasks: (lists: readonly string[] | null, completed: boolean) =>
    ["pim", "tasks", lists, completed] as const,
  task: (id: string | null) => ["pim", "task", id] as const,
  reminders: ["pim", "reminders"] as const,
  search: (query: string) => ["pim", "search", query] as const,
};

/** Everything a PIM write needs. */
export interface PimWriteContext {
  client: NailClient;
  queryClient: QueryClient;
}

/** Refetches all PIM data (after a write; the core's events do the same). */
export function invalidatePim(queryClient: QueryClient): void {
  void queryClient.invalidateQueries({ queryKey: pimKeys.all });
}

type ResponseOf<T extends Response["type"]> = Extract<Response, { type: T }>;

/** Sends a PIM write and refetches PIM data. */
export async function pimWrite<T extends Response["type"]>(
  ctx: PimWriteContext,
  request: Request,
  expected: T,
): Promise<ResponseOf<T>> {
  const response = await ctx.client.request(request, expected);
  invalidatePim(ctx.queryClient);
  return response;
}

export function usePimSources() {
  const client = useClient();
  return useQuery({
    queryKey: pimKeys.sources,
    queryFn: async () => (await client.request({ type: "ListPimSources" }, "PimSources")).sources,
  });
}

export function useCalendars() {
  const client = useClient();
  return useQuery({
    queryKey: pimKeys.calendars,
    queryFn: async () =>
      (await client.request({ type: "ListCalendars", source_id: null }, "Calendars")).calendars,
  });
}

export function useInstances(
  range: { from: string; to: string } | null,
  calendarIds: readonly CalendarId[] | null,
  tz: string,
) {
  const client = useClient();
  return useQuery({
    queryKey: pimKeys.instances(range?.from ?? "", range?.to ?? "", calendarIds, tz),
    queryFn: async (): Promise<EventInstance[]> =>
      (
        await client.request(
          {
            type: "ListEventInstances",
            from: range?.from ?? "",
            to: range?.to ?? "",
            calendar_ids: calendarIds ? [...calendarIds] : null,
            tz,
          },
          "EventInstances",
        )
      ).instances,
    enabled: range !== null,
    placeholderData: keepPreviousData,
  });
}

export function useFreeTime(range: { from: string; to: string }, minutes: number, tz: string) {
  const client = useClient();
  return useQuery({
    queryKey: pimKeys.freeTime(range.from, range.to, minutes, tz),
    queryFn: async () =>
      (
        await client.request(
          {
            type: "FindFreeTime",
            from: range.from,
            to: range.to,
            duration_minutes: minutes,
            calendar_ids: null,
            tz,
          },
          "FreeTime",
        )
      ).slots,
  });
}

export function useEvent(id: string | null) {
  const client = useClient();
  return useQuery({
    queryKey: pimKeys.event(id),
    queryFn: async () =>
      (await client.request({ type: "GetEvent", event_id: id ?? "" }, "PimEvent")).event,
    enabled: id !== null,
  });
}

export function useAddressBooks() {
  const client = useClient();
  return useQuery({
    queryKey: pimKeys.addressBooks,
    queryFn: async () =>
      (await client.request({ type: "ListAddressBooks", source_id: null }, "AddressBooks"))
        .address_books,
  });
}

export function useContacts(
  books: readonly AddressBookId[] | null,
  group: ContactGroupId | null,
  limit: number,
) {
  const client = useClient();
  return useQuery({
    queryKey: pimKeys.contacts(books, group, limit),
    queryFn: async () =>
      client.request(
        {
          type: "ListContacts",
          address_book_ids: books ? [...books] : null,
          group_id: group,
          offset: 0,
          limit,
        },
        "Contacts",
      ),
    placeholderData: keepPreviousData,
  });
}

export function useContactSearch(query: string, books: readonly AddressBookId[] | null) {
  const client = useClient();
  return useQuery({
    queryKey: pimKeys.contactSearch(query, books),
    queryFn: async () =>
      (
        await client.request(
          {
            type: "SearchContacts",
            query,
            address_book_ids: books ? [...books] : null,
            limit: 200,
          },
          "ContactMatches",
        )
      ).matches,
    enabled: query.trim() !== "",
    placeholderData: keepPreviousData,
  });
}

export function useContact(id: ContactId | null) {
  const client = useClient();
  return useQuery({
    queryKey: pimKeys.contact(id),
    queryFn: async () =>
      (await client.request({ type: "GetContact", contact_id: id ?? "" }, "PimContact")).contact,
    enabled: id !== null,
  });
}

/** A contact's photo as a `data:` URL, `null` when it has none. */
export function useContactPhoto(id: ContactId | null, hasPhoto: boolean) {
  const client = useClient();
  return useQuery({
    queryKey: pimKeys.photo(id),
    queryFn: async () => {
      const photo = await client.request(
        { type: "GetContactPhoto", contact_id: id ?? "" },
        "ContactPhoto",
      );
      return `data:${photo.mime_type};base64,${photo.data}`;
    },
    enabled: id !== null && hasPhoto,
  });
}

export function useContactGroups() {
  const client = useClient();
  return useQuery({
    queryKey: pimKeys.groups,
    queryFn: async () =>
      (await client.request({ type: "ListContactGroups", address_book_ids: null }, "ContactGroups"))
        .groups,
  });
}

export function useDuplicates(enabled: boolean) {
  const client = useClient();
  return useQuery({
    queryKey: pimKeys.duplicates,
    queryFn: async () =>
      (
        await client.request(
          { type: "FindDuplicateContacts", address_book_ids: null },
          "ContactDuplicates",
        )
      ).groups,
    enabled,
  });
}

export function useTasks(lists: readonly CalendarId[] | null, includeCompleted: boolean) {
  const client = useClient();
  return useQuery({
    queryKey: pimKeys.tasks(lists, includeCompleted),
    queryFn: async () =>
      (
        await client.request(
          {
            type: "ListTasks",
            calendar_ids: lists ? [...lists] : null,
            include_completed: includeCompleted,
          },
          "Tasks",
        )
      ).tasks,
    placeholderData: keepPreviousData,
  });
}

export function useTask(id: TaskId | null) {
  const client = useClient();
  return useQuery({
    queryKey: pimKeys.task(id),
    queryFn: async () =>
      (await client.request({ type: "GetTask", task_id: id ?? "" }, "PimTask")).task,
    enabled: id !== null,
  });
}

export function useReminders() {
  const client = useClient();
  return useQuery({
    queryKey: pimKeys.reminders,
    queryFn: async () =>
      (await client.request({ type: "ListReminders", until: null }, "Reminders")).reminders,
  });
}

export function usePimSearch(query: string, contactsOnly = false) {
  const client = useClient();
  return useQuery({
    queryKey: [...pimKeys.search(query), contactsOnly],
    queryFn: async () =>
      (
        await client.request(
          { type: "SearchPim", query, kinds: contactsOnly ? ["contact"] : ["event"], limit: 20 },
          "PimSearchResults",
        )
      ).hits,
    enabled: query.trim() !== "",
  });
}

// ---------------------------------------------------------------------------
// Event writes

/** Saves an event (create, or update with a scope). */
export async function saveEvent(
  ctx: PimWriteContext,
  target:
    | { mode: "create"; calendarId: CalendarId }
    | { mode: "update"; eventId: string; scope: EditScope; recurrenceId: PimTime | null },
  fields: EventFields,
  recurrence: Recurrence | null,
) {
  if (target.mode === "create") {
    return pimWrite(
      ctx,
      { type: "CreateEvent", calendar_id: target.calendarId, fields, recurrence },
      "EventSaved",
    );
  }
  return pimWrite(
    ctx,
    {
      type: "UpdateEvent",
      event_id: target.eventId,
      scope: target.scope,
      recurrence_id: target.recurrenceId,
      fields,
      recurrence,
    },
    "EventSaved",
  );
}

export async function deleteEvent(
  ctx: PimWriteContext,
  eventId: string,
  scope: EditScope,
  recurrenceId: PimTime | null,
) {
  return pimWrite(
    ctx,
    { type: "DeleteEvent", event_id: eventId, scope, recurrence_id: recurrenceId },
    "PimDeleted",
  );
}

export async function getEvent(ctx: PimWriteContext, eventId: string): Promise<CalendarEvent> {
  return (await ctx.client.request({ type: "GetEvent", event_id: eventId }, "PimEvent")).event;
}

// ---------------------------------------------------------------------------
// Reminders

export async function snoozeReminders(ctx: PimWriteContext, ids: ReminderId[], until: string) {
  return pimWrite(ctx, { type: "SnoozeReminders", reminder_ids: ids, until }, "Done");
}

export async function dismissReminders(ctx: PimWriteContext, ids: ReminderId[]) {
  return pimWrite(ctx, { type: "DismissReminders", reminder_ids: ids }, "Done");
}
