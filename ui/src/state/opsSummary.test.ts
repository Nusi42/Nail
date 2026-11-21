/**
 * The status bar's queue summary (ADR 0230): read from the core, followed
 * by `OpsChanged`, and read again while writes wait without news — so the
 * count returns to 0 when the core's queue is empty, even if events were
 * lost.
 */
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { NailClient } from "../api/client";
import type { OpsSummary } from "../api/generated";
import {
  OPS_RECHECK_MS,
  createOpsTracker,
  failuresGrew,
  opsWaiting,
  waitingCount,
  type OpsTracker,
} from "./opsSummary";
import { createUiStore, type UiStore } from "./uiStore";

function summary(pending: number, inFlight = 0, failed = 0): OpsSummary {
  const busy = pending + inFlight + failed > 0;
  return {
    pending,
    in_flight: inFlight,
    failed,
    accounts: busy ? [{ account_id: "acc-work", pending, in_flight: inFlight, failed }] : [],
    newest_failure:
      failed > 0
        ? {
            op_id: "op-1",
            account_id: "acc-work",
            code: "network",
            message: null,
            failed_at: "2026-11-21T10:00:00Z",
          }
        : null,
  };
}

/** A core whose queue the test sets; answers `GetOpsSummary` after `delay` ms. */
function fakeCore(delay = 0) {
  const core = {
    queue: summary(0),
    asked: 0,
    client: {
      request: () => {
        core.asked++;
        const answer = core.queue;
        return new Promise((resolve) => {
          setTimeout(() => {
            resolve({ type: "OpsSummary", summary: answer });
          }, delay);
        });
      },
    } as unknown as Pick<NailClient, "request">,
  };
  return core;
}

describe("queue summary rules", () => {
  it("counts pending and in-flight writes as waiting, not failed ones", () => {
    expect(opsWaiting(null)).toBe(0);
    expect(opsWaiting(summary(3, 2, 4))).toBe(5);
  });

  it("adds writes queued since the last summary", () => {
    expect(waitingCount({ opsSummary: summary(2), pendingOps: ["op-a", "op-b"] })).toBe(4);
    expect(waitingCount({ opsSummary: null, pendingOps: [] })).toBe(0);
  });

  it("sees failures grow only when the failed count rises", () => {
    expect(failuresGrew(null, summary(0, 0, 1))).toBe(true);
    expect(failuresGrew(summary(0, 0, 2), summary(0, 0, 1))).toBe(false);
    expect(failuresGrew(summary(0, 0, 1), summary(0, 0, 1))).toBe(false);
  });
});

describe("the ops tracker", () => {
  let store: UiStore;
  let tracker: OpsTracker | null = null;

  beforeEach(() => {
    vi.useFakeTimers();
    store = createUiStore();
  });
  afterEach(() => {
    tracker?.dispose();
    tracker = null;
    vi.useRealTimers();
  });

  it("reads the summary when asked (start, reconnect, Resync)", async () => {
    const core = fakeCore();
    core.queue = summary(7, 1);
    tracker = createOpsTracker({ client: core.client, store, onFailuresGrew: vi.fn() });
    tracker.refresh();
    await vi.runOnlyPendingTimersAsync();
    expect(waitingCount(store.getState())).toBe(8);
  });

  it("follows pushed summaries and clears the writes queued before them", () => {
    tracker = createOpsTracker({ client: fakeCore().client, store, onFailuresGrew: vi.fn() });
    store.getState().opQueued(["op-1", "op-2", "op-3"]);
    expect(waitingCount(store.getState())).toBe(3);
    tracker.pushed(summary(3));
    expect(store.getState().pendingOps).toEqual([]);
    expect(waitingCount(store.getState())).toBe(3);
    tracker.pushed(summary(0));
    expect(waitingCount(store.getState())).toBe(0);
  });

  it("drops an answer older than a pushed summary", async () => {
    const core = fakeCore(50);
    core.queue = summary(500);
    tracker = createOpsTracker({ client: core.client, store, onFailuresGrew: vi.fn() });
    tracker.refresh();
    // The core finished the pass meanwhile and said so.
    tracker.pushed(summary(0));
    await vi.advanceTimersByTimeAsync(60);
    expect(waitingCount(store.getState())).toBe(0);
  });

  it("returns to idle when the events that would say so were lost", async () => {
    const core = fakeCore();
    tracker = createOpsTracker({ client: core.client, store, onFailuresGrew: vi.fn() });
    // The last summary that arrived still had writes waiting …
    tracker.pushed(summary(14));
    // … the ones that said "done" were lost; the core's queue is empty.
    core.queue = summary(0);
    expect(core.asked).toBe(0);
    await vi.advanceTimersByTimeAsync(OPS_RECHECK_MS + 1);
    expect(core.asked).toBe(1);
    expect(waitingCount(store.getState())).toBe(0);
    // Nothing waits: no more questions.
    await vi.advanceTimersByTimeAsync(OPS_RECHECK_MS * 5);
    expect(core.asked).toBe(1);
  });

  it("asks again while writes keep waiting (offline) and stops when they are saved", async () => {
    const core = fakeCore();
    core.queue = summary(2);
    tracker = createOpsTracker({ client: core.client, store, onFailuresGrew: vi.fn() });
    store.getState().opQueued(["op-1", "op-2"]);
    await vi.advanceTimersByTimeAsync(OPS_RECHECK_MS + 1);
    expect(core.asked).toBe(1);
    expect(waitingCount(store.getState())).toBe(2);
    await vi.advanceTimersByTimeAsync(OPS_RECHECK_MS + 1);
    expect(core.asked).toBe(2);
    core.queue = summary(0);
    await vi.advanceTimersByTimeAsync(OPS_RECHECK_MS + 1);
    expect(waitingCount(store.getState())).toBe(0);
    const asked = core.asked;
    await vi.advanceTimersByTimeAsync(OPS_RECHECK_MS * 5);
    expect(core.asked).toBe(asked);
  });

  it("does not ask while summaries keep coming", async () => {
    const core = fakeCore();
    tracker = createOpsTracker({ client: core.client, store, onFailuresGrew: vi.fn() });
    for (let left = 10; left > 0; left--) {
      tracker.pushed(summary(left));
      await vi.advanceTimersByTimeAsync(OPS_RECHECK_MS / 2);
    }
    tracker.pushed(summary(0));
    await vi.advanceTimersByTimeAsync(OPS_RECHECK_MS * 3);
    expect(core.asked).toBe(0);
  });

  it("reports growing failures (the lists show the core's state again)", () => {
    const onFailuresGrew = vi.fn();
    tracker = createOpsTracker({ client: fakeCore().client, store, onFailuresGrew });
    tracker.pushed(summary(1));
    expect(onFailuresGrew).not.toHaveBeenCalled();
    tracker.pushed(summary(0, 0, 1));
    expect(onFailuresGrew).toHaveBeenCalledTimes(1);
    tracker.pushed(summary(0, 0, 1));
    expect(onFailuresGrew).toHaveBeenCalledTimes(1);
  });

  it("stops asking once disposed", async () => {
    const core = fakeCore();
    tracker = createOpsTracker({ client: core.client, store, onFailuresGrew: vi.fn() });
    tracker.pushed(summary(3));
    tracker.dispose();
    tracker.refresh();
    await vi.advanceTimersByTimeAsync(OPS_RECHECK_MS * 3);
    expect(core.asked).toBe(0);
  });
});
