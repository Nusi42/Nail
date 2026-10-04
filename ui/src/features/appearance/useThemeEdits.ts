/**
 * Live theme edits (M5d WP183): every change shows at once as a draft (the
 * editor's fields and preview), and is written as `EditTheme` patches after
 * a short pause — several changes in that pause are one write and one undo
 * entry. Writes never overlap: changes made while one is on its way go in
 * the next, with the revision that write left (`base_revision`), so a
 * theme file changed on disk answers `conflict` instead of being
 * overwritten. Pending changes are written when the editor closes, and
 * before Undo / Redo (`./pendingWrites.ts`, ADR 0138); `flush` resolves
 * only once everything, including a write already on its way, is written.
 */
import { useQueryClient } from "@tanstack/react-query";
import { useCallback, useEffect, useRef, useState } from "react";
import type { ThemeDetails, ThemeEditTarget, ThemePatch } from "../../api/generated";
import { themeEditorKeys, useEditTheme } from "./editorQueries";
import { registerPendingWrite } from "./pendingWrites";

let commitDelay = 400;

/** The pause before drafts are written (tests shorten it). */
export function setThemeEditDelay(ms: number): void {
  commitDelay = ms;
}

/** The pause before live changes are written (theme edits, Appearance sliders). */
export function themeEditDelay(): number {
  return commitDelay;
}

export interface ThemeEdits {
  /** Unsaved values by draft key (`null` = removed). */
  drafts: ReadonlyMap<string, string | null>;
  /** Shows `value` under `key` at once and writes `patch` soon (or now). */
  change: (key: string, patch: ThemePatch, value: string | null, now?: boolean) => void;
  /** Writes pending changes now. */
  flush: () => Promise<void>;
  /** Whether changes are waiting or on their way. */
  saving: boolean;
  /** The last write's error (cleared by the next successful one). */
  failure: unknown;
  /** Forgets the error. */
  clearFailure: () => void;
}

export function useThemeEdits(themeId: string | null): ThemeEdits {
  const queryClient = useQueryClient();
  const edit = useEditTheme();
  const [drafts, setDrafts] = useState<ReadonlyMap<string, string | null>>(() => new Map());
  const [saving, setSaving] = useState(false);
  const [failure, setFailure] = useState<unknown>(null);
  const pending = useRef(new Map<string, ThemePatch>());
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  /** The write on its way, if any. */
  const flight = useRef<Promise<void> | null>(null);
  const flushRef = useRef<() => Promise<void>>(() => Promise.resolve());

  /** Writes one batch of the pending changes. */
  const write = useCallback(
    async (id: string, batch: Map<string, ThemePatch>) => {
      const target: ThemeEditTarget = { kind: "theme", theme_id: id };
      // The revision the last answer left (read now, not at render time).
      const revision = queryClient.getQueryData<ThemeDetails>(
        themeEditorKeys.detailsOf(id),
      )?.revision;
      try {
        await edit.mutateAsync({
          target,
          patches: [...batch.values()],
          ...(revision === undefined ? {} : { baseRevision: revision }),
        });
        setFailure(null);
      } catch (error) {
        setFailure(error);
      } finally {
        setDrafts((current) => {
          const next = new Map(current);
          for (const key of batch.keys()) if (!pending.current.has(key)) next.delete(key);
          return next;
        });
      }
    },
    [edit, queryClient],
  );

  const flush = useCallback(async () => {
    if (timer.current !== null) {
      clearTimeout(timer.current);
      timer.current = null;
    }
    if (themeId === null) return;
    // Writes never overlap: wait for the one on its way, then write the rest.
    for (;;) {
      if (flight.current !== null) {
        await flight.current;
        continue;
      }
      if (pending.current.size === 0) break;
      const batch = new Map(pending.current);
      pending.current.clear();
      const run = write(themeId, batch).finally(() => {
        flight.current = null;
      });
      flight.current = run;
      await run;
    }
    setSaving(false);
  }, [themeId, write]);

  useEffect(() => {
    flushRef.current = flush;
  }, [flush]);

  // Undo / Redo write what waits first (ADR 0138).
  useEffect(() => registerPendingWrite(() => flushRef.current()), []);

  // Another theme, or the editor closes: write what is pending.
  useEffect(
    () => () => {
      void flushRef.current();
    },
    [themeId],
  );

  const change = useCallback(
    (key: string, patch: ThemePatch, value: string | null, now = false) => {
      setDrafts((current) => new Map(current).set(key, value));
      pending.current.set(key, patch);
      setSaving(true);
      if (timer.current !== null) clearTimeout(timer.current);
      if (now) {
        timer.current = null;
        void flushRef.current();
      } else {
        timer.current = setTimeout(() => {
          timer.current = null;
          void flushRef.current();
        }, commitDelay);
      }
    },
    [],
  );

  const clearFailure = useCallback(() => {
    setFailure(null);
  }, []);

  return { drafts, change, flush, saving, failure, clearFailure };
}
