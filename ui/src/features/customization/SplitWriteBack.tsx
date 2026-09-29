/**
 * Background component (a feature effect): writes dragged split sizes into
 * `layout.kdl` once dragging has settled (see `writeBack.ts`).
 */
import { useEffect, useRef } from "react";
import { useLayoutConfig } from "../../layout/LayoutContext";
import { useWindowWidth } from "../../layout/useWindowWidth";
import { useClient } from "../../state/queries";
import { useUiStoreApi } from "../../state/uiStore";
import { WRITE_DELAY_MS, splitPatches, updateLayout } from "./writeBack";

export function SplitWriteBack() {
  const client = useClient();
  const store = useUiStoreApi();
  const layout = useLayoutConfig();
  const width = useWindowWidth();
  // Latest layout and width for the timer callback.
  const current = useRef({ layout, width });
  useEffect(() => {
    current.current = { layout, width };
  }, [layout, width]);
  const written = useRef(new Map<string, string>());

  useEffect(() => {
    // Sizes present when the app starts were not dragged in this session.
    for (const [key, sizes] of Object.entries(store.getState().splitSizes)) {
      written.current.set(key, sizes.join(" "));
    }
    let timer: ReturnType<typeof setTimeout> | null = null;
    const flush = () => {
      timer = null;
      const { layout: l, width: w } = current.current;
      const patches = splitPatches(store.getState().splitSizes, written.current, l, w);
      updateLayout(client, patches).catch((error: unknown) => {
        console.warn("nail: cannot write the split sizes to layout.kdl", error);
      });
    };
    const unsubscribe = store.subscribe((state, previous) => {
      if (state.splitSizes === previous.splitSizes) return;
      if (timer !== null) clearTimeout(timer);
      timer = setTimeout(flush, WRITE_DELAY_MS);
    });
    return () => {
      unsubscribe();
      if (timer !== null) {
        clearTimeout(timer);
        flush();
      }
    };
  }, [client, store]);

  return null;
}
