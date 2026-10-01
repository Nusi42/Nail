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
] as const;
