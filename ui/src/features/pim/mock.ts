/**
 * The mock core's answers to the M3a calendar, contact and task requests
 * (ADR 0058), used by `MockTransport` (`../../api/mock.ts` routes exactly
 * these requests here).
 *
 * Mirrors the core at the contract stage: every PIM request answers
 * `unsupported` until the integration (WP75) wires the store, iCalendar /
 * vCard, CalDAV / CardDAV and the reminder scheduler. WP74 turns this into
 * a working in-memory PIM (sources, calendars, instances, contacts,
 * groups, tasks, reminders) on `data.features.pim` for its UI tests.
 */
import type {
  AddressBook,
  Calendar,
  CalendarEvent,
  Contact,
  ContactGroup,
  PimSource,
  Reminder,
  Request,
  Response,
  Task,
} from "../../api/generated";
import { errorResponse, type MockCore } from "../../api/mockMail";

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

/** The mock's PIM data (WP74 fills and uses it). */
export interface PimMockState {
  sources: PimSource[];
  calendars: Calendar[];
  addressBooks: AddressBook[];
  events: CalendarEvent[];
  contacts: Contact[];
  groups: ContactGroup[];
  tasks: Task[];
  reminders: Reminder[];
}

export function createPimMockState(): PimMockState {
  return {
    sources: [],
    calendars: [],
    addressBooks: [],
    events: [],
    contacts: [],
    groups: [],
    tasks: [],
    reminders: [],
  };
}

/** Answers like the core at the contract stage (`unsupported`, ADR 0058). */
export function answerPim(_core: MockCore, request: PimRequest): Response {
  return errorResponse("unsupported", `${request.type} is not wired yet (M3a WP75)`);
}
