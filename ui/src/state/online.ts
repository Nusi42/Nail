/**
 * Whether the device has a network connection (`navigator.onLine`, kept
 * current by the `online` / `offline` events). Check for new mail is off
 * while it has none (ADR 0183).
 */
import { useSyncExternalStore } from "react";

function subscribe(notify: () => void): () => void {
  window.addEventListener("online", notify);
  window.addEventListener("offline", notify);
  return () => {
    window.removeEventListener("online", notify);
    window.removeEventListener("offline", notify);
  };
}

/** `true` while the device is online (also where nothing can tell). */
export function useOnline(): boolean {
  return useSyncExternalStore(
    subscribe,
    () => navigator.onLine,
    () => true,
  );
}
