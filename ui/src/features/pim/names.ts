/**
 * Names of the PIM feature's dialogs (`window.open "<name>"`; their titles
 * are the Fluent messages `window-<name>`), actions and widget kinds.
 */

/** The event editor. */
export const EVENT_DIALOG = "pim-event-editor";
/** "This / this and following / all" for a change of a recurring event. */
export const SCOPE_DIALOG = "pim-edit-scope";
/** The contact editor. */
export const CONTACT_DIALOG = "pim-contact-editor";
/** Contact groups. */
export const GROUPS_DIALOG = "pim-contact-groups";
/** Merging duplicate contacts. */
export const MERGE_DIALOG = "pim-merge-contacts";
/** The task editor. */
export const TASK_DIALOG = "pim-task-editor";
/** Importing `.ics` / `.vcf` files. */
export const IMPORT_DIALOG = "pim-import";
/** Calendar subscriptions and CalDAV / CardDAV servers. */
export const SOURCES_DIALOG = "pim-sources";

/** A calendar's settings: name, colour, reminders (M5b, from its "…" menu). */
export const CALENDAR_SETTINGS_DIALOG = "pim-calendar-settings";
/** A new calendar or task list in a source (M5b, from a group's "+" menu). */
export const NEW_CALENDAR_DIALOG = "pim-new-calendar";
/** Removing a calendar or unsubscribing (M5b). */
export const REMOVE_CALENDAR_DIALOG = "pim-remove-calendar";

/** "Delete the event …?" (R6: the editor's Delete… asks). */
export const DELETE_EVENT_DIALOG = "pim-delete-event";
/** "Delete the task …?" */
export const DELETE_TASK_DIALOG = "pim-delete-task";
/** "Delete the contact …?" / "Delete N contacts?" (card, list, editor). */
export const DELETE_CONTACTS_DIALOG = "pim-delete-contacts";
/** "Delete the group …?" (Contact groups). */
export const DELETE_GROUP_DIALOG = "pim-delete-group";
/** "Remove the source …?" (Calendars and address books). */
export const REMOVE_SOURCE_DIALOG = "pim-remove-source";

/** Every dialog (for the catalog: `window-<name>`). */
export const DIALOGS = [
  EVENT_DIALOG,
  SCOPE_DIALOG,
  CONTACT_DIALOG,
  GROUPS_DIALOG,
  MERGE_DIALOG,
  TASK_DIALOG,
  IMPORT_DIALOG,
  SOURCES_DIALOG,
  CALENDAR_SETTINGS_DIALOG,
  NEW_CALENDAR_DIALOG,
  REMOVE_CALENDAR_DIALOG,
  DELETE_EVENT_DIALOG,
  DELETE_TASK_DIALOG,
  DELETE_CONTACTS_DIALOG,
  DELETE_GROUP_DIALOG,
  REMOVE_SOURCE_DIALOG,
] as const;

/** The settings page's "Calendar & contacts" section widget (M5b). */
export const PIM_SETTINGS_WIDGET = "pim-settings";
