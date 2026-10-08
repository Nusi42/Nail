/** Debouncing for values that change while the user types. */
import { useEffect, useState } from "react";

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
