/**
 * The status bar's queue of message writes comes from the core (ADR 0230):
 * the summary `GetOpsSummary` answers and `OpsChanged` pushes, never a
 * list of op ids the UI keeps. The tracker reads it on start, after a
 * reconnect and on `Resync`, applies every `OpsChanged`, and reads it again
 * while writes are waiting and no summary came for a while — so the count
 * returns to 0 when the core's queue is empty, even if events were lost.
 */
import type { NailClient } from "../api/client";
import type { OpsSummary } from "../api/generated";
import type { UiState, UiStore } from "./uiStore";

/**
 * How long the tracker waits for a summary while writes are waiting before
 * it asks the core itself (lost events, an answer older than an event).
 */
export const OPS_RECHECK_MS = 2500;

/**
 * Writes waiting to be saved: pending plus in flight. The same rule as the
 * core's `OpsSummary::waiting` (`crates/nail-api`).
 */
export function opsWaiting(summary: OpsSummary | null): number {
  return summary === null ? 0 : summary.pending + summary.in_flight;
}

/**
 * The status bar's "Saving N changes": the core's waiting writes plus the
 * ones queued since its last summary (`opQueued`; the next summary counts
 * them).
 */
export function waitingCount(state: Pick<UiState, "opsSummary" | "pendingOps">): number {
  return opsWaiting(state.opsSummary) + state.pendingOps.length;
}

/** Whether `next` reports more failed writes than `previous` (the lists show the core's state again). */
export function failuresGrew(previous: OpsSummary | null, next: OpsSummary): boolean {
  return next.failed > (previous?.failed ?? 0);
}

export interface OpsTracker {
  /** Applies a pushed `OpsChanged` summary. */
  pushed: (summary: OpsSummary) => void;
  /** Reads the summary from the core (start, reconnect, `Resync`). */
  refresh: () => void;
  /** Stops the timer and the store subscription. */
  dispose: () => void;
}

export interface OpsTrackerOptions {
  client: Pick<NailClient, "request">;
  store: UiStore;
  /** Called when the number of failed writes grew (refetch the lists). */
  onFailuresGrew: () => void;
  /** Re-check delay (tests). */
  recheckMs?: number;
}

/** Follows the core's queue summary into `store` (see the module comment). */
export function createOpsTracker(options: OpsTrackerOptions): OpsTracker {
  const { client, store, onFailuresGrew } = options;
  const recheckMs = options.recheckMs ?? OPS_RECHECK_MS;
  // Bumped by every pushed summary: an answer to a request sent before a
  // newer push is dropped (the push is at least as new).
  let pushes = 0;
  let timer: ReturnType<typeof setTimeout> | null = null;
  let disposed = false;

  const apply = (summary: OpsSummary) => {
    const previous = store.getState().opsSummary;
    store.getState().setOpsSummary(summary);
    if (failuresGrew(previous, summary)) onFailuresGrew();
  };

  const refresh = () => {
    if (disposed) return;
    const sent = pushes;
    client
      .request({ type: "GetOpsSummary" }, "OpsSummary")
      .then(({ summary }) => {
        if (!disposed && sent === pushes) apply(summary);
      })
      .catch((error: unknown) => {
        // An older core without the request, or the connection is gone:
        // the next push or reconnect brings the summary.
        console.warn("nail: reading the queue summary failed", error);
      });
  };

  // While writes are waiting, a summary must come within `recheckMs` of the
  // last change; otherwise the tracker asks (every `recheckMs`) until the
  // core says nothing is waiting.
  const arm = (state: UiState) => {
    if (timer !== null) clearTimeout(timer);
    timer = null;
    if (disposed || waitingCount(state) === 0) return;
    timer = setTimeout(() => {
      timer = null;
      refresh();
      arm(store.getState());
    }, recheckMs);
  };
  const unsubscribe = store.subscribe((state, previous) => {
    if (state.opsSummary !== previous.opsSummary || state.pendingOps !== previous.pendingOps) {
      arm(state);
    }
  });
  arm(store.getState());

  return {
    pushed: (summary) => {
      pushes++;
      apply(summary);
    },
    refresh,
    dispose: () => {
      disposed = true;
      unsubscribe();
      if (timer !== null) clearTimeout(timer);
      timer = null;
    },
  };
}
