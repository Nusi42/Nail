/**
 * UI state kept across restarts by the core (M5b, ADR 0109:
 * `GetUiState` / `SetUiState` over `<config-dir>/ui-state.json`, pushed as
 * `UiStateChanged`): collapsed folder-pane sections, the last view, the last
 * settings section; M5c adds window geometry. Shared by the shell, the mail
 * widgets and the settings page — read a key with `useUiStateValue`, write
 * it with `useSetUiState`. Keys are `nail_api::views::UI_STATE_*`.
 */
import { useMutation, useQuery, useQueryClient, type QueryClient } from "@tanstack/react-query";
import type { JsonValue } from "../api/generated";
import { useClient } from "./queries";
export {
  UI_STATE_COLLAPSED_SECTIONS,
  UI_STATE_SETTINGS_SECTION,
  UI_STATE_VIEW,
} from "../api/generated";

/** The query key of the whole UI state. */
export const uiStateKey = ["ui-state"] as const;

export type UiStateMap = Readonly<Record<string, JsonValue>>;

/** The whole UI state (empty while loading or when the core lacks it). */
export function useUiState() {
  const client = useClient();
  return useQuery({
    queryKey: uiStateKey,
    queryFn: async (): Promise<UiStateMap> => {
      const response = await client.call({ type: "GetUiState" });
      // A core without the request: nothing is remembered.
      return response.type === "UiState" ? response.state : {};
    },
    // `UiStateChanged` keeps it current.
    staleTime: Number.POSITIVE_INFINITY,
    retry: false,
  });
}

/** One key's value, `null` when unset (or not loaded yet). */
export function useUiStateValue(key: string): JsonValue | null {
  const state = useUiState().data;
  return state !== undefined && Object.hasOwn(state, key) ? (state[key] ?? null) : null;
}

/** Applies a key change to the cached state (events and answers). */
export function applyUiStateChange(
  queryClient: QueryClient,
  key: string,
  value: JsonValue | null,
): void {
  queryClient.setQueryData<UiStateMap>(uiStateKey, (old) => {
    const rest = Object.entries(old ?? {}).filter(([k]) => k !== key);
    return Object.fromEntries(value === null ? rest : [...rest, [key, value]]);
  });
}

/**
 * Writes a key (`null` removes it). The cache changes at once; the core's
 * answer (the whole state) replaces it.
 */
export function useSetUiState() {
  const client = useClient();
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: async ({ key, value }: { key: string; value: JsonValue | null }) =>
      (await client.request({ type: "SetUiState", key, value }, "UiState")).state,
    onMutate: ({ key, value }) => {
      applyUiStateChange(queryClient, key, value);
    },
    onSuccess: (state) => queryClient.setQueryData(uiStateKey, state),
  });
}

/** A string array stored under `key` (e.g. the collapsed sections). */
export function stringList(value: JsonValue | null): string[] {
  return Array.isArray(value) ? value.filter((v): v is string => typeof v === "string") : [];
}
