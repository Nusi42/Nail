/**
 * Lists that live as long as the page (toasts, notices): a page left
 * running for days must not let them grow without end (ADR 0207).
 */

/**
 * `list` with `item` appended last: an entry with the same id is replaced
 * (moved to the end), and only the newest `max` entries are kept.
 */
export function pushBounded<T extends { id: string }>(
  list: readonly T[],
  item: T,
  max: number,
): T[] {
  return [...list.filter((entry) => entry.id !== item.id), item].slice(-max);
}
