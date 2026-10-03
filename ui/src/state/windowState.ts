/**
 * Remembered window state and pop-out windows (M5c, ADR 0116): the core's
 * `GetWindowStates` / `SetWindowState` / `ResetWindowStates` over
 * `<config-dir>/window-state.json` (pushed as `WindowStateChanged`) and
 * `ListPopOuts` / `PopOutWindow` / `ReturnPopOut` / `ClosePopOut` /
 * `FocusPopOut` (desktop only, pushed as `PopOutChanged`). `App.tsx`
 * applies both events. Shared contract file (WP171 remembers geometry,
 * WP173 pops windows out); state is kept per window *kind*.
 */
import { useMutation, useQuery, useQueryClient, type QueryClient } from "@tanstack/react-query";
import type { JsonValue, PopOut, WindowState } from "../api/generated";
import { useClient } from "./queries";
import { windowKind } from "./windows";

/** The query key of every remembered window state. */
export const windowStatesKey = ["windowStates"] as const;
/** The query key of the open pop-outs. */
export const popOutsKey = ["popOuts"] as const;

export type WindowStateMap = Readonly<Record<string, WindowState>>;

/** Every remembered window state (empty while loading or without a core that has it). */
export function useWindowStates() {
  const client = useClient();
  return useQuery({
    queryKey: windowStatesKey,
    queryFn: async (): Promise<WindowStateMap> => {
      const response = await client.call({ type: "GetWindowStates" });
      return response.type === "WindowStates" ? response.states : {};
    },
    // `WindowStateChanged` keeps it current.
    staleTime: Number.POSITIVE_INFINITY,
    retry: false,
  });
}

/** The remembered state of a window (by id or kind), `null` when none or not loaded. */
export function useWindowState(window: string): WindowState | null {
  const states = useWindowStates().data;
  const kind = windowKind(window);
  return states !== undefined && Object.hasOwn(states, kind) ? (states[kind] ?? null) : null;
}

/** Applies a kind's change to the cached states (events and answers). */
export function applyWindowStateChange(
  queryClient: QueryClient,
  window: string,
  state: WindowState | null,
): void {
  queryClient.setQueryData<WindowStateMap>(windowStatesKey, (old) => {
    const rest = Object.entries(old ?? {}).filter(([k]) => k !== window);
    return Object.fromEntries(state === null ? rest : [...rest, [window, state]]);
  });
}

/**
 * Remembers a kind's state (`null` forgets it; an id is reduced to its
 * kind). The cache changes at once; the core's answer replaces it.
 */
export function useSetWindowState() {
  const client = useClient();
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: async ({ window, state }: { window: string; state: WindowState | null }) =>
      (
        await client.request(
          { type: "SetWindowState", window: windowKind(window), state },
          "WindowStates",
        )
      ).states,
    onMutate: ({ window, state }) => applyWindowStateChange(queryClient, windowKind(window), state),
    onSuccess: (states) => queryClient.setQueryData(windowStatesKey, states),
  });
}

/** Forgets every remembered window state ("Reset window positions"). */
export function useResetWindowStates() {
  const client = useClient();
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: async () =>
      (await client.request({ type: "ResetWindowStates" }, "WindowStates")).states,
    onSuccess: (states) => queryClient.setQueryData(windowStatesKey, states),
  });
}

/** The open pop-out windows (empty outside the desktop app). */
export function usePopOuts() {
  const client = useClient();
  return useQuery({
    queryKey: popOutsKey,
    queryFn: async (): Promise<readonly PopOut[]> => {
      const response = await client.call({ type: "ListPopOuts" });
      // Remote sessions are refused (`forbidden`): nothing is popped out.
      return response.type === "PopOuts" ? response.pop_outs : [];
    },
    // `PopOutChanged` keeps it current.
    staleTime: Number.POSITIVE_INFINITY,
    retry: false,
  });
}

/** Replaces the cached pop-out list (from `PopOutChanged` or an answer). */
export function applyPopOuts(queryClient: QueryClient, popOuts: readonly PopOut[]): void {
  queryClient.setQueryData<readonly PopOut[]>(popOutsKey, popOuts);
}

/** The pop-out requests; each answer replaces the cached list. */
export function usePopOutActions() {
  const client = useClient();
  const queryClient = useQueryClient();
  const run = async (
    request:
      | { type: "PopOutWindow"; window: string; title?: string; context?: JsonValue }
      | { type: "ReturnPopOut"; window: string }
      | { type: "ClosePopOut"; window: string }
      | { type: "FocusPopOut"; window: string },
  ): Promise<readonly PopOut[]> => {
    const { pop_outs } = await client.request(request, "PopOuts");
    applyPopOuts(queryClient, pop_outs);
    return pop_outs;
  };
  return {
    popOut: (window: string, title?: string, context?: JsonValue) =>
      run({
        type: "PopOutWindow",
        window,
        ...(title === undefined ? {} : { title }),
        ...(context === undefined ? {} : { context }),
      }),
    returnToPage: (window: string) => run({ type: "ReturnPopOut", window }),
    close: (window: string) => run({ type: "ClosePopOut", window }),
    focus: (window: string) => run({ type: "FocusPopOut", window }),
  };
}
