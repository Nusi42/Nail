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
import { obtain } from "./uiStore";
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

/** Writes of each key sent and not answered yet, per query client. */
const pendingWrites = new WeakMap<QueryClient, Map<string, number>>();

function countPending(queryClient: QueryClient, key: string, by: 1 | -1): void {
  const pending = obtain(pendingWrites, queryClient, () => new Map<string, number>());
  const count = (pending.get(key) ?? 0) + by;
  if (count > 0) pending.set(key, count);
  else pending.delete(key);
}

/**
 * Applies a `UiStateChanged` event, unless a write of its key is on its way:
 * the event may then be an older write's, and the newest write's answer
 * sets the key (R8: under load the events of quickly shown settings
 * sections showed the older sections again, one after the other, for
 * seconds).
 */
export function applyUiStateEvent(
  queryClient: QueryClient,
  key: string,
  value: JsonValue | null,
): void {
  if (pendingWrites.get(queryClient)?.has(key) === true) return;
  applyUiStateChange(queryClient, key, value);
}

/** The newest write of each key, per query client (see `useSetUiState`). */
const latestWrites = new WeakMap<QueryClient, Map<string, number>>();

/** Numbers a write of `key`; the number is the newest until the next write of that key. */
function numberWrite(queryClient: QueryClient, key: string): number {
  const writes = obtain(latestWrites, queryClient, () => new Map<string, number>());
  const write = (writes.get(key) ?? 0) + 1;
  writes.set(key, write);
  return write;
}

/**
 * Writes a key (`null` removes it). The cache changes at once; the core's
 * answer then sets that key, unless a newer write of it was sent meanwhile.
 * Only the written key is taken from the answer (the whole state): it can
 * be older than writes of other keys sent after it, whose values are in
 * the cache already, and `UiStateChanged` keeps every key current (R8: the
 * answer to showing the Settings view brought the previous section back for
 * a moment, and the section shown meanwhile lost its unsaved changes).
 */
export function useSetUiState() {
  const client = useClient();
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: async ({ key, value }: { key: string; value: JsonValue | null }) =>
      (await client.request({ type: "SetUiState", key, value }, "UiState")).state,
    onMutate: ({ key, value }) => {
      applyUiStateChange(queryClient, key, value);
      countPending(queryClient, key, 1);
      return { write: numberWrite(queryClient, key) };
    },
    onSettled: (_state, _error, { key }) => {
      countPending(queryClient, key, -1);
    },
    onSuccess: (state, { key }, context) => {
      if (latestWrites.get(queryClient)?.get(key) !== context.write) return;
      applyUiStateChange(queryClient, key, Object.hasOwn(state, key) ? (state[key] ?? null) : null);
    },
  });
}

/** A string array stored under `key` (e.g. the collapsed sections). */
export function stringList(value: JsonValue | null): string[] {
  return Array.isArray(value) ? value.filter((v): v is string => typeof v === "string") : [];
}
