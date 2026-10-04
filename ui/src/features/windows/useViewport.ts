/**
 * The page size the window layer lays windows out on (`window.innerWidth` ×
 * `window.innerHeight`), updated when the browser window or the desktop
 * window changes size.
 */
import { useMemo, useSyncExternalStore } from "react";
import type { Viewport } from "./geometry";

function subscribe(notify: () => void): () => void {
  window.addEventListener("resize", notify);
  return () => {
    window.removeEventListener("resize", notify);
  };
}

/** Used where there is no window (server rendering). */
export const FALLBACK_VIEWPORT: Viewport = { width: 1280, height: 800 };

const snapshot = () => `${String(window.innerWidth)}x${String(window.innerHeight)}`;
const serverSnapshot = () =>
  `${String(FALLBACK_VIEWPORT.width)}x${String(FALLBACK_VIEWPORT.height)}`;

/** The current page size (a stable object while the size stays the same). */
export function useViewport(): Viewport {
  const key = useSyncExternalStore(subscribe, snapshot, serverSnapshot);
  return useMemo(() => {
    const [width = 0, height = 0] = key.split("x").map(Number);
    return {
      width: width > 0 ? width : FALLBACK_VIEWPORT.width,
      height: height > 0 ? height : FALLBACK_VIEWPORT.height,
    };
  }, [key]);
}
