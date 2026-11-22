/**
 * What a feature's dialog shows before its content: nothing to show (the
 * thing it opened for is gone) or still loading. The feature names its
 * parts: the dialog's (`data-state` `empty` / `loading`) and the text's.
 *
 * R6 (ADR 0239): a dialog with nothing left to do ends with one Done (the
 * shared footer's `done` state), never a lone Close. `useCloseNow` is the
 * close of a dialog after its own Save: it skips the close guard, which
 * would otherwise ask about the changes that were just saved.
 */
import { useCallback, useRef } from "react";
import type { TagNoun } from "../features/mail-actions/run";
import { useTagWindow } from "../features/mail-actions/tags";
import { useT } from "../i18n/I18nProvider";
import { useUiStoreApi } from "../state/uiStore";
import { MAIN_GUARD } from "../layout/closeGuard";
import { DialogFooter, closeWindowNow, useCloseGuard, useGuardKey } from "./dialog";

interface DialogStateProps {
  /** The dialog's part, e.g. `exchange.dialog`. */
  part: string;
  /** The text's part, e.g. `exchange.hint`. */
  textPart: string;
}

/** The text `hintId` and Done. */
export function EmptyDialog({
  part,
  textPart,
  hintId,
  close,
}: DialogStateProps & { hintId: string; close: () => void }) {
  const t = useT();
  return (
    <div data-part={part} data-state="empty">
      <p data-part={textPart}>{t(hintId)}</p>
      <DialogFooter done={{ onDone: close }} />
    </div>
  );
}

/** "Loading…" while the dialog's data arrives. */
export function LoadingDialog({ part, textPart }: DialogStateProps) {
  const t = useT();
  return (
    <div data-part={part} data-state="loading" aria-busy="true">
      <p data-part={textPart}>{t("common-loading")}</p>
    </div>
  );
}

/**
 * Closes the calling dialog's window without asking (its close guard is
 * skipped): for the dialog's own primary after it saved. Outside a window
 * (a dialog rendered on its own) it is `close`.
 */
export function useCloseNow(close: () => void): () => void {
  const ui = useUiStoreApi();
  const window = useGuardKey();
  return useCallback(() => {
    if (window === MAIN_GUARD) close();
    else closeWindowNow(ui, window);
  }, [ui, window, close]);
}

/**
 * A tag window (Gmail's "Apply labels", Exchange's categories) as an
 * editor (R6): `useTagWindow` whose own close after applying skips the
 * close guard; `done` is set once it closed that way.
 */
export function useTagDialog(noun: TagNoun, close: () => void) {
  const done = useRef(false);
  const closeNow = useCloseNow(close);
  const tags = useTagWindow(noun, () => {
    done.current = true;
    closeNow();
  });
  return { ...tags, done };
}

/** The footer of a tag window: Save (the form's submit button) and Cancel. */
export function TagDialogFooter({ busy, close }: { busy: boolean; close: () => void }) {
  const t = useT();
  return (
    <DialogFooter
      primary={{ label: t("common-save"), submit: true, action: "save" }}
      cancel={{ onClick: close }}
      busy={busy}
    />
  );
}

/**
 * The close guard of a tag window over `all` its tags: dirty while a
 * decision differs from what the targets carry; Save applies them.
 */
export function useTagDialogGuard(
  tags: ReturnType<typeof useTagDialog>,
  all: readonly string[],
): void {
  useCloseGuard({
    name: () => tags.summary?.subject ?? "",
    isDirty: () =>
      !tags.done.current &&
      all.some((tag) => {
        const decided = tags.decision(tag);
        return decided !== undefined && tags.mark(tag) !== (decided ? "all" : "none");
      }),
    save: async () => {
      await tags.apply(all);
      return tags.done.current;
    },
  });
}
