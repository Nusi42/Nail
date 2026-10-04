/**
 * Feature state of the MCP slot (Zustand, outside `uiStore`): the latest
 * access of an agent to the UI context, shown by the on-screen indicator.
 */
import { useStore } from "zustand";
import { createStore } from "zustand/vanilla";

/** How long the indicator stays after the last access (ms). */
export const INDICATOR_MS = 4000;

/** An agent read (or edited) what is on screen. */
export interface ContextAccess {
  clientId: string;
  clientName: string;
  edit: boolean;
  /** Increases with every access; a hide timer only clears its own access. */
  seq: number;
}

export interface McpFeatureState {
  /** The access the indicator shows, `null` when hidden. */
  access: ContextAccess | null;
  /** Records an access and hides it again after `INDICATOR_MS`. */
  noteAccess: (clientId: string, clientName: string, edit: boolean) => void;
  /** Hides the indicator. */
  clear: () => void;
}

let seq = 0;

export const mcpStore = createStore<McpFeatureState>()((set, get) => ({
  access: null,
  noteAccess: (clientId, clientName, edit) => {
    const access = { clientId, clientName, edit, seq: ++seq };
    set({ access });
    setTimeout(() => {
      if (get().access?.seq === access.seq) set({ access: null });
    }, INDICATOR_MS);
  },
  clear: () => {
    set({ access: null });
  },
}));

export function useMcpState<T>(selector: (state: McpFeatureState) => T): T {
  return useStore(mcpStore, selector);
}
