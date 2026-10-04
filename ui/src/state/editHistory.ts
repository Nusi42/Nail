/**
 * The in-app editors' undo histories (M5d, ADR 0131: `GetEditHistory`,
 * `UndoEdit`, `RedoEdit`, pushed as `EditHistoryChanged`). One history per
 * scope: `layout` (`layout.kdl`; the layout editor and the toolbar
 * customizer) and `appearance` (`theme.kdl` and the user themes; Appearance
 * and the theme editor). Every write the core makes to those files is an
 * entry, also the M1b editors' divider / toolbar / key edits and
 * `SetTheme` / `SetThemePreferences`.
 *
 * `history.revision` is the scope's main file's revision now: pass it as
 * `base_revision` of `UpdateLayout` / `EditTheme`, so an edit made on a
 * layout that changed on disk answers `conflict` (`edit-issue-conflict`)
 * instead of changing the wrong node. Undo / redo answer `conflict` too when
 * a file changed on disk; offer "Undo anyway" (`force`).
 */
import { useMutation, useQuery, useQueryClient, type QueryClient } from "@tanstack/react-query";
import type { EditHistory, HistoryScope } from "../api/generated";
import { useClient } from "./queries";

/** The query key of a scope's history. */
export const editHistoryKey = (scope: HistoryScope) => ["edit-history", scope] as const;

/** A history without entries (before the first answer, or an old core). */
export function emptyHistory(scope: HistoryScope): EditHistory {
  return { scope, revision: "none", undo: [], redo: [] };
}

/** The scope's undo history; `EditHistoryChanged` keeps it current. */
export function useEditHistory(scope: HistoryScope) {
  const client = useClient();
  return useQuery({
    queryKey: editHistoryKey(scope),
    queryFn: async (): Promise<EditHistory> => {
      const response = await client.call({ type: "GetEditHistory", scope });
      // A core without the request: nothing to undo.
      return response.type === "EditHistory" ? response.history : emptyHistory(scope);
    },
    staleTime: Number.POSITIVE_INFINITY,
    retry: false,
  });
}

/** Puts a history (event or answer) into the cache. */
export function applyEditHistory(queryClient: QueryClient, history: EditHistory): void {
  queryClient.setQueryData(editHistoryKey(history.scope), history);
}

/** The next entry Undo / Redo would take, `null` when there is none. */
export function nextUndo(history: EditHistory | undefined) {
  return history?.undo.at(-1) ?? null;
}

/** See {@link nextUndo}. */
export function nextRedo(history: EditHistory | undefined) {
  return history?.redo.at(-1) ?? null;
}

/**
 * Undo (`direction: "undo"`) or redo the scope's last entry; `force` undoes
 * over changes made on disk. Rejects with the core's error (`conflict`,
 * `invalid_request` for nothing to undo).
 */
export function useEditHistoryStep(scope: HistoryScope) {
  const client = useClient();
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: async ({ direction, force }: { direction: "undo" | "redo"; force?: boolean }) =>
      (
        await client.request(
          direction === "undo"
            ? { type: "UndoEdit", scope, force: force ?? false }
            : { type: "RedoEdit", scope, force: force ?? false },
          "EditHistory",
        )
      ).history,
    onSuccess: (history) => {
      applyEditHistory(queryClient, history);
    },
  });
}
