/**
 * Writing the layout editor's changes (M5d WP181): every change is one
 * `UpdateLayout` with the layout history's revision as `base_revision`, so
 * a `layout.kdl` changed on disk answers `conflict` instead of moving the
 * wrong node; the core writes the file (comments kept), records the undo
 * entry and pushes `ConfigChanged` + `EditHistoryChanged`.
 */
import { useQueryClient } from "@tanstack/react-query";
import { useCallback } from "react";
import { NailError, errorMessageOf } from "../../api/client";
import type { LayoutPatch, LocalizedMessage } from "../../api/generated";
import {
  cachedRevision,
  refreshEditHistory,
  useEditHistory,
  useEditHistoryStep,
} from "../../state/editHistory";
import { useClient } from "../../state/queries";
import { editorStore, setMessage } from "./store";

/**
 * Sends `patches`; `done` is announced when they were written. The session
 * stays busy until the history names the revision this write left, so the
 * next edit (its buttons wait for `busy`) never sends the one it replaced.
 */
export function useLayoutEdit(): (
  patches: LayoutPatch[],
  done: LocalizedMessage,
  focusPane?: string | null,
) => Promise<boolean> {
  const client = useClient();
  const queryClient = useQueryClient();
  // Keeps the history loaded: its revision goes with every write.
  useEditHistory("layout");
  return useCallback(
    async (patches, done, focusPane = null) => {
      if (patches.length === 0) return false;
      editorStore.setState({ busy: true, moving: null, dragging: false, hover: null });
      const revision = cachedRevision(queryClient, "layout");
      try {
        await client.request(
          revision === undefined
            ? { type: "UpdateLayout", patches }
            : { type: "UpdateLayout", patches, base_revision: revision },
          "Done",
        );
      } catch (error) {
        editorStore.setState({
          busy: false,
          message: { kind: "error", text: errorMessageOf(error) },
        });
        return false;
      }
      // Written; its `EditHistoryChanged` may still be on its way.
      await refreshEditHistory(queryClient, client, "layout").catch(() => undefined);
      editorStore.setState({ busy: false, message: { kind: "status", text: done }, focusPane });
      return true;
    },
    [client, queryClient],
  );
}

/**
 * Undo / Redo of the layout history, with "Undo anyway" after a conflict.
 * A step marks the session busy until it is answered, and a step asked for
 * while busy is ignored — a double-click undoes one entry (ADR 0138).
 */
export function useLayoutStep(): (direction: "undo" | "redo", force?: boolean) => void {
  const step = useEditHistoryStep("layout");
  const { mutate } = step;
  return useCallback(
    (direction, force = false) => {
      if (editorStore.getState().busy) return;
      editorStore.setState({ busy: true });
      mutate(
        { direction, force },
        {
          onSettled: () => {
            editorStore.setState({ busy: false });
          },
          onSuccess: () => {
            setMessage({
              kind: "status",
              text: {
                id: direction === "undo" ? "layout-editor-undone" : "layout-editor-redone",
              },
            });
          },
          onError: (error) => {
            const conflict = error instanceof NailError && error.code === "conflict";
            setMessage({
              kind: "error",
              text: errorMessageOf(error),
              ...(conflict ? { forceStep: direction } : {}),
            });
          },
        },
      );
    },
    [mutate],
  );
}
