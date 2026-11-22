/**
 * The system whose button order dialogs follow (R6, ADR 0239; the order
 * itself is `./order.ts`).
 */
import { createContext, useContext, useMemo } from "react";
import type { Os } from "../../api/generated";
import { viewerOs } from "./order";

/**
 * The system whose button order dialogs follow: the viewer's, unless a
 * provider pins one (tests, previews of another system).
 */
export const DialogOsContext = createContext<Os | null>(null);

/** The system dialogs follow (see `DialogOsContext`). */
export function useDialogOs(): Os {
  const pinned = useContext(DialogOsContext);
  return useMemo(() => pinned ?? viewerOs(), [pinned]);
}
