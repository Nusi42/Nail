/**
 * UI state of the scheduling feature (one store per UI store, like the PIM
 * feature's): what the "cancel meeting" dialog cancels and what the "find
 * a time" dialog starts with.
 */
import { useStore } from "zustand";
import { createStore, type StoreApi } from "zustand/vanilla";
import type { Attendee, EventId, PimTime } from "../../api/generated";
import { useUiStoreApi, type UiStore } from "../../state/uiStore";

/** The meeting the cancel dialog is about. */
export interface CancelTarget {
  eventId: EventId;
  title: string;
  /** The occurrence shown in the editor, `null` = the series / a single event. */
  recurrenceId: PimTime | null;
  recurring: boolean;
  attendees: Attendee[];
}

/** What "find a time" starts with. */
export interface FindTimeSeed {
  attendees: string[];
  durationMinutes: number;
}

export interface SchedulingState {
  cancelTarget: CancelTarget | null;
  findTime: FindTimeSeed | null;
  setCancelTarget(target: CancelTarget | null): void;
  setFindTime(seed: FindTimeSeed | null): void;
}

const stores = new WeakMap<UiStore, StoreApi<SchedulingState>>();

export function schedulingStoreFor(ui: UiStore): StoreApi<SchedulingState> {
  const existing = stores.get(ui);
  if (existing) return existing;
  const store = createStore<SchedulingState>()((set) => ({
    cancelTarget: null,
    findTime: null,
    setCancelTarget: (cancelTarget) => set({ cancelTarget }),
    setFindTime: (findTime) => set({ findTime }),
  }));
  stores.set(ui, store);
  return store;
}

export function useScheduling<T>(selector: (state: SchedulingState) => T): T {
  return useStore(schedulingStoreFor(useUiStoreApi()), selector);
}

export function useSchedulingStore(): StoreApi<SchedulingState> {
  return schedulingStoreFor(useUiStoreApi());
}
