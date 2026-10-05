/**
 * Lets the envs a test created settle before the test's DOM is unmounted.
 *
 * Why: many widget tests end at their last assertion while the app still
 * works — a save invalidates a list that refetches, a mutation's `onSettled`
 * runs, the mock core answers a request on its next tick. When the machine
 * is idle, `setup.ts` unmounts before any of that lands; under load the
 * pending answer lands first, outside `act`, and React's "not wrapped in
 * act(...)" warning fails the test (`console.ts`). `setup.ts` therefore
 * waits here first: until every env's queries and mutations are idle (as
 * `settleQueries` in the harness does), then lets React commit inside `act`.
 *
 * Kept free of app imports: `setup.ts` loads it into every test file.
 */
import { act, waitFor } from "@testing-library/react";
import { vi } from "vitest";

/** What `settleLiveEnvs` needs of an env (the harness' `Env`). */
export interface Settleable {
  queryClient: { isFetching: () => number; isMutating: () => number };
}

const live = new Set<Settleable>();

/** Registers an env for the current test (`createEnv` calls it). */
export function trackEnv(env: Settleable): void {
  live.add(env);
}

function idle(env: Settleable): boolean {
  return env.queryClient.isFetching() === 0 && env.queryClient.isMutating() === 0;
}

/**
 * Waits (at most `timeout` ms, without failing) until the envs created since
 * the last call are idle, then flushes React inside `act`. Under fake timers
 * it waits for nothing: the test owns the clock.
 */
export async function settleLiveEnvs(timeout = 3_000): Promise<void> {
  const envs = [...live];
  live.clear();
  if (envs.length === 0 || vi.isFakeTimers()) return;
  try {
    await waitFor(
      () => {
        if (!envs.every(idle)) throw new Error("still busy");
      },
      { timeout },
    );
  } catch {
    // A request the test left unanswered on purpose: the unmount ends it.
  }
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 0));
  });
}
