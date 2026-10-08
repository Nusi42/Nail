/**
 * Value lists of the PIM editors (the order they are offered in). Pure, so
 * the catalog coverage test can list the message ids built from them.
 */
import {
  ALARM_ACTION_VALUES,
  ATTENDEE_ROLE_VALUES,
  CONTACT_KIND_VALUES,
  EDIT_SCOPE_VALUES,
  EVENT_STATUS_VALUES,
  TASK_STATUS_VALUES,
  TRANSPARENCY_VALUES,
  VISIBILITY_VALUES,
} from "../../api/generated";
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

export const EVENT_STATUSES: readonly EventStatus[] = EVENT_STATUS_VALUES;
export const TRANSPARENCIES: readonly Transparency[] = TRANSPARENCY_VALUES;
export const VISIBILITIES: readonly Visibility[] = VISIBILITY_VALUES;
export const ATTENDEE_ROLES: readonly AttendeeRole[] = ATTENDEE_ROLE_VALUES;
export const ALARM_ACTIONS: readonly AlarmAction[] = ALARM_ACTION_VALUES;
export const ALARM_UNITS = ["minutes", "hours", "days", "weeks"] as const;
export type AlarmUnit = (typeof ALARM_UNITS)[number];
export const EDIT_SCOPES: readonly EditScope[] = EDIT_SCOPE_VALUES;
export const CONTACT_KINDS: readonly ContactKind[] = CONTACT_KIND_VALUES;
export const TASK_STATUSES: readonly TaskStatus[] = TASK_STATUS_VALUES;
/** When an alarm fires, relative to the event (`pim-alarm-<when>`). */
export const ALARM_WHEN = ["before-start", "after-start", "before-end", "after-end"] as const;
