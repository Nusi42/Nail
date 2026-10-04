/**
 * Appearance writes that wait (M5d review, ADR 0138): the Appearance
 * sliders / colour picker and the theme editor write a change only after a
 * short pause. Undo / Redo must not overtake such a write — the delayed
 * write would put the undone value back — so every form with a waiting
 * write registers how to write it now, and the history buttons write them
 * all first (`flushPendingWrites`), then undo the change they made.
 */

type Flush = () => Promise<void>;

const flushes = new Set<Flush>();

/** Registers a form's "write what waits now"; returns the unregister call. */
export function registerPendingWrite(flush: Flush): () => void {
  flushes.add(flush);
  return () => {
    flushes.delete(flush);
  };
}

/** Writes every waiting appearance change and waits until they are written. */
export async function flushPendingWrites(): Promise<void> {
  await Promise.all([...flushes].map((flush) => flush()));
}
