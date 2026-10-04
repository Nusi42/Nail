/**
 * UI state of the remote feature (a Zustand store of its own): security
 * alerts waiting to be acknowledged, the user the pairing dialog opens
 * for, and — in a browser on the remote UI — the session this page runs
 * in and why it ended.
 */
import { useStore } from "zustand";
import { createStore } from "zustand/vanilla";
import type {
  RemoteAlert,
  RemoteSession,
  RemoteUserId,
  SessionEndReason,
} from "../../api/generated";

/** A shown alert. */
export interface ShownAlert {
  id: number;
  alert: RemoteAlert;
  /** When it arrived (RFC 3339). */
  at: string;
}

/** At most this many alerts are kept; older ones drop off. */
export const MAX_ALERTS = 5;

export interface RemoteFeatureState {
  alerts: ShownAlert[];
  pushAlert: (alert: RemoteAlert) => void;
  dismissAlert: (id: number) => void;
  /** The user the `remote-pair` dialog preselects. */
  pairUser: RemoteUserId | null;
  setPairUser: (id: RemoteUserId | null) => void;
  /** The remote-UI session of this browser (set after login). */
  session: RemoteSession | null;
  /** Why this browser's session ended (then the app shows the login page). */
  ended: SessionEndReason | null;
  setSession: (session: RemoteSession | null) => void;
  endSession: (reason: SessionEndReason) => void;
  clear: () => void;
}

let nextAlert = 1;

export const remoteStore = createStore<RemoteFeatureState>()((set) => ({
  alerts: [],
  pushAlert: (alert) => {
    set((s) => ({
      alerts: [...s.alerts, { id: nextAlert++, alert, at: new Date().toISOString() }].slice(
        -MAX_ALERTS,
      ),
    }));
  },
  dismissAlert: (id) => {
    set((s) => ({ alerts: s.alerts.filter((a) => a.id !== id) }));
  },
  pairUser: null,
  setPairUser: (pairUser) => {
    set({ pairUser });
  },
  session: null,
  ended: null,
  setSession: (session) => {
    set({ session, ended: null });
  },
  endSession: (ended) => {
    set({ ended });
  },
  clear: () => {
    set({ alerts: [], pairUser: null, session: null, ended: null });
  },
}));

export function useRemoteState<T>(selector: (state: RemoteFeatureState) => T): T {
  return useStore(remoteStore, selector);
}
