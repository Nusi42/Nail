/**
 * A coarse shared clock for relative dates ("5 minutes ago"), exposed as an
 * external store so rendering stays pure.
 */
import { useSyncExternalStore } from "react";

/** One day in milliseconds (24 hours; calendar days come from the time zone). */
export const DAY_MS = 86_400_000;

const TICK_MS = 30_000;
let now = Date.now();
const listeners = new Set<() => void>();
let timer: ReturnType<typeof setInterval> | null = null;

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  timer ??= setInterval(() => {
    now = Date.now();
    for (const l of listeners) l();
  }, TICK_MS);
  return () => {
    listeners.delete(listener);
    if (listeners.size === 0 && timer !== null) {
      clearInterval(timer);
      timer = null;
    }
  };
}

/** Current time in ms, refreshed every 30 s. */
export function useNow(): number {
  return useSyncExternalStore(
    subscribe,
    () => now,
    () => now,
  );
}
