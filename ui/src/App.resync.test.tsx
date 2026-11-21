/**
 * Lost events (ADR 0230): after `Resync` the app reads the queue summary
 * again, refetches the open list and drops selected messages the list no
 * longer shows; the status bar follows the core's summary, not op ids.
 */
import { act, fireEvent, screen } from "@testing-library/react";
import { opsSummaryOf } from "./api/mockOps";
import type { Env } from "./test/harness";
import { mockFolderId } from "./api/mockData";
import { appWindowEachTest, eventually, renderApp, settleUntil } from "./test/app";
import { listSelectionFor } from "./actions/mailTargets";
import { must } from "./test/must";

appWindowEachTest();

async function ready() {
  return settleUntil(() => {
    const rows = screen.queryAllByRole("option");
    return rows.length > 1 ? rows : null;
  }, "message rows");
}

/** Clicks a row and returns the message it selected. */
async function select(env: Env, row: HTMLElement): Promise<string> {
  fireEvent.click(row);
  await eventually(() => {
    expect(env.store.getState().selectedMessageId).not.toBeNull();
  });
  return must(env.store.getState().selectedMessageId);
}

/** Moves the message's whole conversation away in the mock core, sending no event. */
function moveAwaySilently(env: Env, messageId: string): void {
  const thread = env.transport.data.messages.find((m) => m.id === messageId)?.thread_id ?? null;
  for (const message of env.transport.data.messages) {
    if (message.id === messageId || (thread !== null && message.thread_id === thread)) {
      message.folder_id = mockFolderId(message.account_id, "Archive");
    }
  }
}

function opsItem(): HTMLElement | null {
  return document.querySelector('[data-part="status-bar.item"][data-item="ops"]');
}

describe("resync after lost events", () => {
  it("refetches the open list and drops the selection it no longer shows", async () => {
    const { env } = renderApp();
    const rows = await ready();
    const row = must(rows[1]);
    const subject = must(row.textContent);
    const before = rows.length;
    moveAwaySilently(env, await select(env, row));
    // Without an event the list keeps the stale row.
    expect(screen.queryAllByRole("option").map((r) => r.textContent)).toContain(subject);

    act(() => {
      env.transport.emit({ type: "Resync" });
    });
    await eventually(() => {
      expect(screen.queryAllByRole("option")).toHaveLength(before - 1);
      expect(env.store.getState().selectedMessageId).toBeNull();
    });
    expect(screen.queryAllByRole("option").map((r) => r.textContent)).not.toContain(subject);
    expect(env.transport.requests).toContainEqual({ type: "GetOpsSummary" });
  });

  it("keeps a selection the refetched list still shows and prunes picked rows", async () => {
    const { env } = renderApp();
    const rows = await ready();
    const kept = must(rows[0]);
    const gone = must(rows[2]);
    const goneId = await select(env, gone);
    fireEvent.click(kept, { ctrlKey: true });
    await eventually(() => {
      expect(listSelectionFor(env.store).getState().picked).toHaveLength(2);
    });
    // `kept` was clicked last: it is the selection and the list still shows it.
    const selected = env.store.getState().selectedMessageId;
    moveAwaySilently(env, goneId);
    act(() => {
      env.transport.emit({ type: "Resync" });
    });
    await eventually(() => {
      expect(listSelectionFor(env.store).getState().picked).toEqual([]);
    });
    expect(env.store.getState().selectedMessageId).toBe(selected);
  });

  it("ends idle when the core's queue is empty although the done events were lost", async () => {
    const { env } = renderApp();
    await ready();
    // The core queued 14 writes and said so …
    for (let n = 0; n < 14; n++) {
      env.transport.data.opQueue.set(`lost-${String(n)}`, {
        accountId: "acc-work",
        state: "pending",
        error: null,
        failedAt: null,
      });
    }
    act(() => {
      env.transport.emit({ type: "OpsChanged", summary: opsSummaryOf(env.transport.data.opQueue) });
    });
    await eventually(() => {
      expect(opsItem()).toHaveTextContent("Saving 14 changes…");
    });
    // … then saved them, but every event saying so was lost; the lag ends
    // with `Resync`.
    env.transport.data.opQueue.clear();
    act(() => {
      env.transport.emit({ type: "Resync" });
    });
    await eventually(() => {
      expect(opsItem()).toBeNull();
    });
  });
});
