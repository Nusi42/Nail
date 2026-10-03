/**
 * "Delete permanently?" (M5c, plan section 10): `mail.delete-permanently`
 * asks before it deletes, in a modal confirmation window
 * (`mail-delete-permanently`, `{ confirmation: true }` in `./feature.ts`).
 * The messages are the selection when the action ran (kept per UI store),
 * so a selection change behind the window cannot change what is deleted.
 */
import { useQueryClient } from "@tanstack/react-query";
import { useEffect, useRef, useState } from "react";
import type { MessageId } from "../api/generated";
import { useT } from "../i18n/I18nProvider";
import type { DialogProps } from "../layout/dialogRegistry";
import { deleteMessages, type WriteContext } from "../state/mailOps";
import { useClient } from "../state/queries";
import { useUiStoreApi, type UiStore } from "../state/uiStore";

/** The confirmation's window kind (dialog name). */
export const DELETE_PERMANENTLY_DIALOG = "mail-delete-permanently";

const pending = new WeakMap<UiStore, readonly MessageId[]>();

/** The messages the open confirmation would delete. */
export function pendingPermanentDelete(store: UiStore): readonly MessageId[] {
  return pending.get(store) ?? [];
}

/** Asks before deleting `ids` permanently (nothing to ask for no messages). */
export function askDeletePermanently(ctx: WriteContext, ids: readonly MessageId[]): void {
  if (ids.length === 0) return;
  pending.set(ctx.store, [...ids]);
  ctx.store.getState().openWindow(DELETE_PERMANENTLY_DIALOG);
}

/** Deletes the pending messages permanently and closes the confirmation. */
export async function confirmDeletePermanently(ctx: WriteContext): Promise<void> {
  const ids = pendingPermanentDelete(ctx.store);
  pending.delete(ctx.store);
  ctx.store.getState().closeWindow(DELETE_PERMANENTLY_DIALOG);
  await deleteMessages(ctx, ids, true);
}

/** The confirmation window's content. */
export function DeletePermanentlyDialog({ close }: DialogProps) {
  const t = useT();
  const store = useUiStoreApi();
  const client = useClient();
  const queryClient = useQueryClient();
  const [count] = useState(() => pendingPermanentDelete(store).length);
  const [busy, setBusy] = useState(false);
  const cancel = useRef<HTMLButtonElement>(null);
  // The safe choice has the focus (after the frame focused its first button).
  useEffect(() => queueMicrotask(() => cancel.current?.focus()), []);
  return (
    <div data-part="mail-confirm" data-confirm="delete-permanently" aria-busy={busy}>
      <p data-part="mail-confirm.question">{t("mail-delete-permanently-question", { count })}</p>
      <p data-part="mail-confirm.note">{t("mail-delete-permanently-note")}</p>
      <div data-part="mail-confirm.actions">
        <button
          type="button"
          data-part="form.button"
          data-variant="danger"
          disabled={busy || count === 0}
          onClick={() => {
            setBusy(true);
            void confirmDeletePermanently({ client, store, queryClient }).finally(() =>
              setBusy(false),
            );
          }}
        >
          {t("mail-delete-permanently-confirm")}
        </button>
        <button
          ref={cancel}
          type="button"
          data-part="form.button"
          disabled={busy}
          onClick={() => {
            pending.delete(store);
            close();
          }}
        >
          {t("mail-delete-permanently-cancel")}
        </button>
      </div>
    </div>
  );
}
