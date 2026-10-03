/**
 * UI state of the PIM feature, one store per UI store: the day the
 * calendar views show (shared by every calendar, mini-month and agenda
 * widget), a view chosen by `calendar.view`, the selected event / contact /
 * task, what the editors and dialogs are about, and the reminder toasts.
 */
import { useStore } from "zustand";
import { createStore, type StoreApi } from "zustand/vanilla";
import type {
  CalendarComponent,
  CalendarId,
  ContactFields,
  ContactId,
  EventFields,
  EventInstance,
  LocalizedMessage,
  PimTime,
  Reminder,
  TaskFields,
  TaskId,
  AddressBookId,
} from "../../api/generated";
import { useUiStoreApi, type UiStore } from "../../state/uiStore";
import type { ViewKind } from "./time";

/** What the event editor edits. */
export type EventEditorTarget =
  | {
      mode: "new";
      start: PimTime;
      end: PimTime;
      calendarId: CalendarId | null;
      /** Pre-filled fields ("create event from email"). */
      fields: EventFields | null;
    }
  | {
      mode: "edit";
      eventId: string;
      /** The occurrence (recurring events), `null` = the series / a single event. */
      recurrenceId: PimTime | null;
      /** The occurrence's start / end as shown. */
      start: PimTime | null;
      end: PimTime | null;
    };

/** A change of a recurring event waiting for "this / this and following / all". */
export type PendingChange =
  | { kind: "move"; instance: EventInstance; start: PimTime; end: PimTime }
  | { kind: "delete"; instance: EventInstance };

export type ContactEditorTarget =
  | { mode: "new"; addressBookId: AddressBookId | null; fields: ContactFields | null }
  | { mode: "edit"; contactId: ContactId };

export type TaskEditorTarget =
  | { mode: "new"; listId: CalendarId | null; parentId: TaskId | null; fields: TaskFields | null }
  | { mode: "edit"; taskId: TaskId };

/** What the new-calendar dialog creates: a calendar or task list in a source. */
export interface NewCalendarTarget {
  sourceId: string;
  component: CalendarComponent;
}

/** A toast: a fired reminder, or a PIM write the server refused. */
export type PimToast =
  | { kind: "reminder"; id: string; reminder: Reminder }
  | { kind: "problem"; id: string; message: LocalizedMessage };

export interface PimState {
  /** The day the views show (`YYYY-MM-DD`), `null` = today. */
  anchor: string | null;
  /** View chosen with `calendar.view`, `null` = each widget's own. */
  view: ViewKind | null;
  /** The view and page size the last calendar widget showed (for `calendar.next`). */
  shown: { view: ViewKind; days: number } | null;
  /** Focused / selected occurrence in the calendar views. */
  selected: { eventId: string; recurrenceId: PimTime | null } | null;
  eventEditor: EventEditorTarget | null;
  pendingChange: PendingChange | null;
  /** The contact the card shows. */
  contactId: ContactId | null;
  /** Contacts selected in the list (for merging / deleting). */
  selectedContacts: ContactId[];
  contactEditor: ContactEditorTarget | null;
  /** Contacts the merge dialog merges (`[]` = find duplicates). */
  mergeIds: ContactId[];
  taskId: TaskId | null;
  taskEditor: TaskEditorTarget | null;
  toasts: PimToast[];
  /** A desktop file to import (a launch argument), taken by the import dialog. */
  importPath: string | null;
  /** A `webcal:` link to subscribe to (a launch argument), taken by the sources dialog. */
  subscribeUrl: string | null;
  /** The calendar the calendar-settings dialog edits (M5b). */
  calendarSettingsId: CalendarId | null;
  /** What the new-calendar dialog creates (M5b). */
  newCalendar: NewCalendarTarget | null;
  /** The calendar the remove dialog asks about (M5b). */
  removeCalendarId: CalendarId | null;

  setAnchor(anchor: string | null): void;
  setView(view: ViewKind | null): void;
  setShown(shown: { view: ViewKind; days: number } | null): void;
  select(selected: PimState["selected"]): void;
  editEvent(target: EventEditorTarget | null): void;
  setPendingChange(change: PendingChange | null): void;
  showContact(id: ContactId | null): void;
  setSelectedContacts(ids: ContactId[]): void;
  editContact(target: ContactEditorTarget | null): void;
  setMergeIds(ids: ContactId[]): void;
  showTask(id: TaskId | null): void;
  editTask(target: TaskEditorTarget | null): void;
  pushToast(toast: PimToast): void;
  dismissToast(id: string): void;
  setImportPath(path: string | null): void;
  setSubscribeUrl(url: string | null): void;
  editCalendar(id: CalendarId | null): void;
  setNewCalendar(target: NewCalendarTarget | null): void;
  setRemoveCalendar(id: CalendarId | null): void;
}

const stores = new WeakMap<UiStore, StoreApi<PimState>>();

export function pimStoreFor(ui: UiStore): StoreApi<PimState> {
  const existing = stores.get(ui);
  if (existing) return existing;
  const store = createStore<PimState>()((set) => ({
    anchor: null,
    view: null,
    shown: null,
    selected: null,
    eventEditor: null,
    pendingChange: null,
    contactId: null,
    selectedContacts: [],
    contactEditor: null,
    mergeIds: [],
    taskId: null,
    taskEditor: null,
    toasts: [],
    importPath: null,
    subscribeUrl: null,
    calendarSettingsId: null,
    newCalendar: null,
    removeCalendarId: null,
    setAnchor: (anchor) => set({ anchor }),
    setView: (view) => set({ view }),
    setShown: (shown) => set({ shown }),
    select: (selected) => set({ selected }),
    editEvent: (eventEditor) => set({ eventEditor }),
    setPendingChange: (pendingChange) => set({ pendingChange }),
    showContact: (contactId) => set({ contactId }),
    setSelectedContacts: (selectedContacts) => set({ selectedContacts }),
    editContact: (contactEditor) => set({ contactEditor }),
    setMergeIds: (mergeIds) => set({ mergeIds }),
    showTask: (taskId) => set({ taskId }),
    editTask: (taskEditor) => set({ taskEditor }),
    pushToast: (toast) =>
      set((s) => ({ toasts: [...s.toasts.filter((t) => t.id !== toast.id), toast] })),
    dismissToast: (id) => set((s) => ({ toasts: s.toasts.filter((t) => t.id !== id) })),
    setImportPath: (importPath) => set({ importPath }),
    setSubscribeUrl: (subscribeUrl) => set({ subscribeUrl }),
    editCalendar: (calendarSettingsId) => set({ calendarSettingsId }),
    setNewCalendar: (newCalendar) => set({ newCalendar }),
    setRemoveCalendar: (removeCalendarId) => set({ removeCalendarId }),
  }));
  stores.set(ui, store);
  return store;
}

export function usePim<T>(selector: (state: PimState) => T): T {
  return useStore(pimStoreFor(useUiStoreApi()), selector);
}

/** The PIM store of the current UI. */
export function usePimStore(): StoreApi<PimState> {
  return pimStoreFor(useUiStoreApi());
}
