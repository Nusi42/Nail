/**
 * Value lists of the PIM editors (the order they are offered in). Pure, so
 * the catalog coverage test can list the message ids built from them.
 */
import type {
  AlarmAction,
  AttendeeRole,
  ContactKind,
  EditScope,
  EventStatus,
  TaskStatus,
  Transparency,
  Visibility,
} from "../../api/generated";

export const EVENT_STATUSES: readonly EventStatus[] = ["confirmed", "tentative", "cancelled"];
export const TRANSPARENCIES: readonly Transparency[] = ["busy", "free"];
export const VISIBILITIES: readonly Visibility[] = ["public", "private", "confidential"];
export const ATTENDEE_ROLES: readonly AttendeeRole[] = [
  "chair",
  "required",
  "optional",
  "non-participant",
];
export const ALARM_ACTIONS: readonly AlarmAction[] = ["display", "audio", "email"];
export const ALARM_UNITS = ["minutes", "hours", "days", "weeks"] as const;
export type AlarmUnit = (typeof ALARM_UNITS)[number];
export const EDIT_SCOPES: readonly EditScope[] = ["this", "this-and-following", "all"];
export const CONTACT_KINDS: readonly ContactKind[] = ["individual", "organization", "location"];
export const TASK_STATUSES: readonly TaskStatus[] = [
  "needs-action",
  "in-process",
  "completed",
  "cancelled",
];
