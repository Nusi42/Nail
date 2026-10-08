/**
 * Helpers for tests that run the whole app against the mock core.
 *
 * Waits are bounded by event-loop turns, not by wall-clock time: the mock
 * core answers within a turn or two and nothing here needs a real delay, so
 * a loaded machine (cargo builds in parallel) makes the waits slower but
 * never flaky, unlike `findBy*` / `waitFor` with their 1 s budget.
 */
import { act, render } from "@testing-library/react";
import { App } from "../App";
import { createEnv, setWindowWidth, stubElementSizes, type Env } from "./harness";

/** How many event-loop turns a wait takes at most. */
export const TURNS = 100;

/** One event-loop turn inside `act`, so React commits what it received. */
export async function turn(): Promise<void> {
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 0));
  });
}

/** Turns until `query` finds something. */
export async function settleUntil<T>(
  query: () => T | null,
  what: string,
  turns = TURNS,
): Promise<T> {
  for (let n = 0; n < turns; n++) {
    const found = query();
    if (found !== null) return found;
    await turn();
  }
  throw new Error(`${what} did not appear within ${String(turns)} event-loop turns`);
}

/**
 * Turns until `assertion` passes (a turn-bounded `waitFor`). It starts with
 * a turn, so what the last event started (an action's request and the store
 * updates after its answer) finishes inside `act`.
 */
export async function eventually(assertion: () => void): Promise<void> {
  let failure: unknown = null;
  for (let n = 0; n < TURNS; n++) {
    await turn();
    try {
      assertion();
      return;
    } catch (error) {
      failure = error;
    }
  }
  throw failure;
}

/** Renders the whole app on `env` (by default a fresh, still connecting one). */
export function renderApp(env: Env = createEnv({ state: { connection: "connecting" } })) {
  const result = render(
    <App client={env.client} queryClient={env.queryClient} store={env.store} />,
  );
  return { ...result, env };
}

/** A 1200 px wide window with stubbed element sizes around every test of the file. */
export function appWindowEachTest(): void {
  let restore: () => void = () => undefined;
  beforeEach(() => {
    restore = stubElementSizes();
    setWindowWidth(1200);
  });
  afterEach(() => {
    restore();
    setWindowWidth(1024);
  });
}
