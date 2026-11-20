/** Debouncing for values that change while the user types. */
import { useCallback, useEffect, useRef, useState } from "react";

/** The typing pause before a search or a suggestion asks the core. */
export const TYPING_PAUSE_MS = 150;

/**
 * `value`, but only after it stopped changing for `ms`; `same` tells two
 * values apart (a fresh list with the same items is the same value).
 */
export function useDebounced<T>(
  value: T,
  ms: number,
  same: (a: T, b: T) => boolean = Object.is,
): T {
  const [settled, setSettled] = useState(value);
  useEffect(() => {
    if (same(settled, value)) return;
    const timer = setTimeout(() => {
      setSettled(value);
    }, ms);
    return () => {
      clearTimeout(timer);
    };
  }, [value, settled, ms, same]);
  return settled;
}

/**
 * The typing pause before the search field searches what was typed (live
 * search, ADR 0219): longer than a suggestion's pause, so a word is not
 * searched letter by letter. Enter searches at once.
 */
export const LIVE_SEARCH_PAUSE_MS = 400;

/**
 * Calls `action(value)` once `value` stopped changing for `ms` and differs
 * from the value last acted on (never for the first value). The returned
 * `flush` acts at once on the current value (Enter) and drops the pending
 * call; `enabled: false` drops it too.
 */
export function useDebouncedAction<T>(
  value: T,
  ms: number,
  action: (value: T) => void,
  enabled = true,
): () => void {
  const actionRef = useRef(action);
  const valueRef = useRef(value);
  const doneRef = useRef(value);
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  useEffect(() => {
    actionRef.current = action;
  }, [action]);
  const cancel = useCallback(() => {
    if (timerRef.current !== null) clearTimeout(timerRef.current);
    timerRef.current = null;
  }, []);
  const flush = useCallback(() => {
    cancel();
    doneRef.current = valueRef.current;
    actionRef.current(valueRef.current);
  }, [cancel]);
  useEffect(() => {
    valueRef.current = value;
    cancel();
    if (!enabled || Object.is(value, doneRef.current)) return;
    timerRef.current = setTimeout(flush, ms);
    return cancel;
  }, [value, ms, enabled, cancel, flush]);
  return flush;
}
