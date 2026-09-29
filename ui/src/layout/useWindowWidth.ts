import { useSyncExternalStore } from "react";

function subscribe(notify: () => void): () => void {
  window.addEventListener("resize", notify);
  let observer: ResizeObserver | null = null;
  if (typeof ResizeObserver !== "undefined") {
    observer = new ResizeObserver(notify);
    observer.observe(document.documentElement);
  }
  return () => {
    window.removeEventListener("resize", notify);
    observer?.disconnect();
  };
}

/** The window width in CSS pixels, updated on resize. */
export function useWindowWidth(): number {
  return useSyncExternalStore(
    subscribe,
    () => window.innerWidth,
    () => 1024,
  );
}
