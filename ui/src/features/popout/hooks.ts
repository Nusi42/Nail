/**
 * Pop-out hooks (M5c WP173): whether this core has pop-outs, the context a
 * pop-out window was opened with, and naming a pop-out's OS window (its
 * taskbar entry, e.g. after the composer's subject).
 */
import { useQuery } from "@tanstack/react-query";
import { createContext, useContext, useEffect } from "react";
import type { JsonValue } from "../../api/generated";
import { useClient } from "../../state/queries";
import { usePopOuts } from "../../state/windowState";
import { WindowInstanceContext } from "../../state/windows";
import { PROBE_WINDOW, popOutContextOf, probeSaysAvailable } from "./model";

/** The query key of the availability probe. */
export const popOutsAvailableKey = ["popOuts", "available"] as const;

/**
 * Whether windows can pop out here: only in the desktop app (the browser
 * UI — `nail serve`, remote sessions — keeps every window in the page).
 * `false` while not known yet.
 */
export function usePopOutsAvailable(): boolean {
  const client = useClient();
  const query = useQuery({
    queryKey: popOutsAvailableKey,
    queryFn: async () =>
      probeSaysAvailable(await client.call({ type: "FocusPopOut", window: PROBE_WINDOW })),
    staleTime: Number.POSITIVE_INFINITY,
    retry: false,
  });
  return query.data === true;
}

/** Sets the title of the pop-out window this component is in. */
export const PopOutTitleContext = createContext<((title: string | null) => void) | null>(null);

/**
 * Names the OS window (and its taskbar entry) of the pop-out this
 * component is shown in, e.g. the composer's subject; `null` or an empty
 * text = the window's own title. Does nothing in the main window.
 */
export function usePopOutTitle(title: string | null): void {
  const setTitle = useContext(PopOutTitleContext);
  const clean = title === null || title.trim() === "" ? null : title.trim();
  useEffect(() => {
    if (setTitle === null) return;
    setTitle(clean);
    return () => setTitle(null);
  }, [setTitle, clean]);
}

/**
 * The context the window this component is in was popped out with (e.g.
 * `{"draft": "d42"}`), `null` in the main window or without one.
 */
export function usePopOutContext(): JsonValue | null {
  const window = useContext(WindowInstanceContext);
  const popOuts = usePopOuts().data ?? [];
  return window === null ? null : popOutContextOf(popOuts, window);
}
