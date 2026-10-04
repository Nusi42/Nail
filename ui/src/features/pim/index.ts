/**
 * Feature slot of M3a WP74 for calendars, contacts and tasks (contract
 * ADR 0058; UI ADR 0062): calendar views (day, multi-day, week, work
 * week, month, year, agenda, free-time timeline) and the mini-month /
 * agenda widgets, the event editor (recurrence with "this / this and
 * following / all", attendees, alarms, time zones), contacts (list, card,
 * editor, groups, merge dialog), the task list and editor, reminder toasts
 * (snooze / dismiss), calendars and sources, and `.ics` / `.vcf` import.
 *
 * M5b (WP164, ADR 0109): the calendar view's one-line toolbar with the
 * segmented view switch, the calendar list with colour squares and "…" /
 * "+" menus (dialogs instead of inline forms), readable event cards, the
 * look of the calendar, contact and task views (`skin.ts`), and the
 * settings page's "Calendar & contacts" section (`pim-settings`).
 *
 * Only the PIM package edits the files in this directory (plus
 * `crates/nail-config/src/widgets/pim.rs` and `locales/{en,de}/pim.ftl`).
 * The central registry (`../registry.ts`) merges these exports into the
 * app; see `../README.md`.
 */
import type { Event as NailEvent } from "../../api/generated";
import type { ActionContext } from "../../actions/registry";
import type { Feature, FeatureEventHandler } from "../types";
import { CalendarWidget } from "./Calendar";
import { GroupsDialog, MergeDialog } from "./ContactDialogs";
import { ContactCard } from "./ContactCard";
import { ContactList } from "./ContactList";
import { ImportDialog } from "./ImportDialog";
import { ContactEditorDialog, EventEditorDialog, TaskEditorDialog } from "./lazyDialogs";
import {
  CALENDAR_SETTINGS_DIALOG,
  CONTACT_DIALOG,
  EVENT_DIALOG,
  GROUPS_DIALOG,
  IMPORT_DIALOG,
  MERGE_DIALOG,
  NEW_CALENDAR_DIALOG,
  PIM_SETTINGS_WIDGET,
  REMOVE_CALENDAR_DIALOG,
  SCOPE_DIALOG,
  SOURCES_DIALOG,
  TASK_DIALOG,
} from "./names";
import { openNewEvent, openNewTask } from "./newEvent";
import { pimKeys } from "./queries";
import { PimToasts } from "./Reminders";
import { ScopeDialog } from "./ScopeDialog";
import { Agenda, MiniMonth } from "./SmallWidgets";
import { CalendarList } from "./CalendarList";
import { CalendarSettingsDialog, NewCalendarDialog, RemoveCalendarDialog } from "./CalendarDialogs";
import { CALENDAR_LIST_INFO, PIM_SETTINGS_INFO, PimSettings } from "./PimSettings";
import { SourcesDialog } from "./Sources";
import { pimStoreFor } from "./store";
import { TaskList } from "./TaskList";
import {
  isViewKind,
  localZone,
  parseDay,
  stepAnchor,
  todayIn,
  dateString,
  DEFAULT_VIEW_OPTIONS,
} from "./time";

export { pimKeys } from "./queries";
export {
  CONTACT_DIALOG,
  EVENT_DIALOG,
  GROUPS_DIALOG,
  IMPORT_DIALOG,
  MERGE_DIALOG,
  SCOPE_DIALOG,
  SOURCES_DIALOG,
  TASK_DIALOG,
} from "./names";
export { pimStoreFor } from "./store";
export { loadPimEditors } from "./lazyDialogs";

/** The core's PIM events (sources, collections, items, reminders, queue). */
export const PIM_EVENTS = [
  "PimSourcesChanged",
  "CalendarsChanged",
  "AddressBooksChanged",
  "EventsChanged",
  "TasksChanged",
  "ContactsChanged",
  "ReminderDue",
  "RemindersChanged",
  "PimOpStatus",
] as const satisfies readonly NailEvent["type"][];

/** Layout widget kinds → components. */
export const widgets: Feature["widgets"] = {
  calendar: CalendarWidget,
  agenda: Agenda,
  "mini-month": MiniMonth,
  "task-list": TaskList,
  "contact-list": ContactList,
  "contact-card": ContactCard,
  "calendar-list": CalendarList,
  // M5b (WP164): the settings page's "Calendar & contacts" section.
  [PIM_SETTINGS_WIDGET]: PimSettings,
};

/**
 * The settings page's search index (M5b, ADR 0109): the setting labels of
 * the PIM widgets a `settings-page` section can hold.
 */
export const settings: NonNullable<Feature["settings"]> = {
  [PIM_SETTINGS_WIDGET]: PIM_SETTINGS_INFO,
  "calendar-list": CALENDAR_LIST_INFO,
};

function pimOf(ctx: ActionContext) {
  return pimStoreFor(ctx.store);
}

/** Moves the calendar views a page (`1`) or back (`-1`). */
function step(ctx: ActionContext, direction: 1 | -1): void {
  const pim = pimOf(ctx).getState();
  const tz = localZone();
  const anchor = (pim.anchor ? parseDay(pim.anchor) : null) ?? todayIn(tz);
  const shown = pim.shown ?? { view: pim.view ?? "week", days: DEFAULT_VIEW_OPTIONS.days };
  const next = stepAnchor(pim.view ?? shown.view, anchor, direction, {
    ...DEFAULT_VIEW_OPTIONS,
    days: shown.days,
  });
  pim.setAnchor(dateString(next));
}

const hasMessage = (ctx: ActionContext) => ctx.store.getState().selectedMessageId !== null;

/** Action ids → handlers. */
export const actions: Feature["actions"] = {
  /** Shows today in the calendar views. */
  "calendar.goto-today": {
    run: (ctx) => {
      pimOf(ctx).getState().setAnchor(null);
    },
  },
  "calendar.next": {
    run: (ctx) => {
      step(ctx, 1);
    },
  },
  "calendar.previous": {
    run: (ctx) => {
      step(ctx, -1);
    },
  },
  /** `calendar.view "month"`: switches the calendar views. */
  "calendar.view": {
    run: (ctx, args) => {
      const view = args[0];
      if (isViewKind(view)) pimOf(ctx).getState().setView(view);
    },
    enabled: (_ctx, args) => isViewKind(args[0]),
  },
  /** A new event on the day shown, 09:00. */
  "calendar.new-event": {
    run: (ctx) => {
      const pim = pimOf(ctx);
      const tz = localZone();
      const day =
        (pim.getState().anchor ? parseDay(pim.getState().anchor ?? "") : null) ?? todayIn(tz);
      openNewEvent({ store: ctx.store, pim }, day, { hour: 9, minute: 0 }, tz);
    },
  },
  "calendar.calendars": {
    run: (ctx) => {
      ctx.store.getState().openWindow(SOURCES_DIALOG);
    },
  },
  "pim.import": {
    run: (ctx) => {
      ctx.store.getState().openWindow(IMPORT_DIALOG);
    },
  },
  "pim.sources": {
    run: (ctx) => {
      ctx.store.getState().openWindow(SOURCES_DIALOG);
    },
  },
  "pim.sync": {
    run: async (ctx) => {
      await ctx.client.request({ type: "SyncPimSources", source_id: null }, "Done");
    },
  },
  "tasks.new": {
    run: (ctx) => {
      openNewTask({ store: ctx.store, pim: pimOf(ctx) }, null);
    },
  },
  "contacts.new": {
    run: (ctx) => {
      pimOf(ctx).getState().editContact({ mode: "new", addressBookId: null, fields: null });
      ctx.store.getState().openWindow(CONTACT_DIALOG);
    },
  },
  "contacts.groups": {
    run: (ctx) => {
      ctx.store.getState().openWindow(GROUPS_DIALOG);
    },
  },
  /** Merges the selected contacts, or finds duplicates. */
  "contacts.merge": {
    run: (ctx) => {
      const pim = pimOf(ctx).getState();
      pim.setMergeIds(pim.selectedContacts.length >= 2 ? pim.selectedContacts : []);
      ctx.store.getState().openWindow(MERGE_DIALOG);
    },
  },
  /** "Create event from email": the core suggests the fields, the editor opens. */
  "calendar.event-from-message": {
    run: async (ctx) => {
      const id = ctx.store.getState().selectedMessageId;
      if (id === null) return;
      const { fields } = await ctx.client.request(
        { type: "EventFromMessage", message_id: id },
        "EventSuggested",
      );
      const tz = localZone();
      openNewEvent({ store: ctx.store, pim: pimOf(ctx) }, todayIn(tz), null, tz, null, fields);
    },
    enabled: hasMessage,
  },
  /** "Create task from email": keeps a link to the message. */
  "tasks.from-message": {
    run: async (ctx) => {
      const id = ctx.store.getState().selectedMessageId;
      if (id === null) return;
      const { fields } = await ctx.client.request(
        { type: "TaskFromMessage", message_id: id },
        "TaskSuggested",
      );
      openNewTask({ store: ctx.store, pim: pimOf(ctx) }, null, null, fields);
    },
    enabled: hasMessage,
  },
};

/** Dialog names → components. */
export const dialogs: Feature["dialogs"] = {
  [EVENT_DIALOG]: EventEditorDialog,
  [SCOPE_DIALOG]: ScopeDialog,
  [CONTACT_DIALOG]: ContactEditorDialog,
  [GROUPS_DIALOG]: GroupsDialog,
  [MERGE_DIALOG]: MergeDialog,
  [TASK_DIALOG]: TaskEditorDialog,
  [IMPORT_DIALOG]: ImportDialog,
  [SOURCES_DIALOG]: SourcesDialog,
  [CALENDAR_SETTINGS_DIALOG]: CalendarSettingsDialog,
  [NEW_CALENDAR_DIALOG]: NewCalendarDialog,
  [REMOVE_CALENDAR_DIALOG]: RemoveCalendarDialog,
};

/**
 * The window model of this feature's windows (M5c, ADR 0116): tool windows
 * are non-modal with their default and minimum sizes; only confirmations
 * (`confirmation: true`) are modal. The layout's `window` properties win.
 */
export const windows: NonNullable<Feature["windows"]> = {
  [EVENT_DIALOG]: { size: { width: 720, height: 680 }, min_size: { width: 520, height: 480 } },
  [SCOPE_DIALOG]: { confirmation: true, size: { width: 480, height: 320 } },
  [CONTACT_DIALOG]: { size: { width: 640, height: 640 }, min_size: { width: 480, height: 420 } },
  [GROUPS_DIALOG]: { size: { width: 560, height: 520 }, min_size: { width: 420, height: 360 } },
  [MERGE_DIALOG]: { size: { width: 720, height: 560 }, min_size: { width: 520, height: 400 } },
  [TASK_DIALOG]: { size: { width: 600, height: 560 }, min_size: { width: 440, height: 400 } },
  [IMPORT_DIALOG]: { size: { width: 640, height: 520 }, min_size: { width: 480, height: 360 } },
  [SOURCES_DIALOG]: { size: { width: 720, height: 560 }, min_size: { width: 520, height: 400 } },
  [CALENDAR_SETTINGS_DIALOG]: {
    size: { width: 520, height: 520 },
    min_size: { width: 400, height: 380 },
  },
  [NEW_CALENDAR_DIALOG]: {
    size: { width: 520, height: 460 },
    min_size: { width: 400, height: 340 },
  },
  [REMOVE_CALENDAR_DIALOG]: { confirmation: true },
};

/** PIM data changed: refetch what shows it; reminders and failed writes become toasts. */
export const handlePimEvent: FeatureEventHandler = (event, { queryClient, store }) => {
  if (!(PIM_EVENTS as readonly string[]).includes(event.type)) return;
  void queryClient.invalidateQueries({ queryKey: pimKeys.all });
  const pim = pimStoreFor(store).getState();
  if (event.type === "ReminderDue") {
    pim.pushToast({
      kind: "reminder",
      id: `reminder-${event.reminder.id}`,
      reminder: event.reminder,
    });
  } else if (event.type === "PimOpStatus" && event.status.state === "failed") {
    pim.pushToast({
      kind: "problem",
      id: `op-${event.status.op_id}`,
      message: event.status.localized ?? { id: "pim-op-failed" },
    });
  }
};

/** Handlers for the core's events. */
export const events: Feature["events"] = [handlePimEvent];
/** Background components: the reminder toasts. */
export const effects: Feature["effects"] = [PimToasts];
