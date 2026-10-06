/**
 * "Delete permanently?" (M5c, plan section 10; P3, ADR 0164):
 * `mail.delete-permanently` (and Delete in Trash) asks before it deletes, in
 * a modal confirmation window (`mail-delete-permanently`,
 * `{ confirmation: true }` in `./feature.ts`). The targets are the
 * selection when the action ran — the selected conversation, the picked
 * rows or a card's message (kept per UI store), so a selection change
 * behind the window cannot change what is deleted — and the question names
 * the number of messages the core resolved them to (`ResolveMailTargets`).
 * Confirming deletes exactly those messages: one that reached Trash or Junk
 * while the question was open (a sync, another window) was not counted and
 * stays.
 */
import { useQueryClient } from "@tanstack/react-query";
import { useEffect, useRef, useState } from "react";
import type { ActionTargets } from "../actions/mailTargets";
import type { MessageId } from "../api/generated";
import { errorMessageOf } from "../api/client";
import { mailActionsStoreFor } from "../features/mail-actions/store";
import { runMailAction } from "../features/mail-actions/run";
import { useT } from "../i18n/I18nProvider";
import type { DialogProps } from "../layout/dialogRegistry";
import type { WriteContext } from "../state/mailOps";
import { useClient } from "../state/queries";
import { useUiStoreApi, type UiStore } from "../state/uiStore";

/** The confirmation's window kind (dialog name). */
export const DELETE_PERMANENTLY_DIALOG = "mail-delete-permanently";

/** What the open confirmation would delete. */
export interface PendingDelete {
  targets: ActionTargets;
  /** Messages the targets resolve to. */
  messages: number;
  /** Those messages: what Delete permanently acts on. */
  messageIds: readonly MessageId[];
  /** Conversations among the targets that resolve to a message. */
  conversations: number;
}

const pending = new WeakMap<UiStore, PendingDelete>();

/** What the open confirmation would delete (`null` = nothing). */
export function pendingPermanentDelete(store: UiStore): PendingDelete | null {
  return pending.get(store) ?? null;
}

/**
 * Asks before deleting `targets` permanently: resolves them first (the
 * count; a refusal such as "not in Trash" shows in the toast instead).
 */
export async function askDeletePermanently(
  ctx: WriteContext,
  targets: ActionTargets,
): Promise<void> {
  if (targets.targets.length === 0) return;
  try {
    const { resolution } = await ctx.client.request(
      {
        type: "ResolveMailTargets",
        targets: [...targets.targets],
        action: { type: "delete-permanently" },
      },
      "MailTargets",
    );
    if (resolution.message_ids.length === 0) return;
    pending.set(ctx.store, {
      targets,
      messages: resolution.message_ids.length,
      messageIds: resolution.message_ids,
      conversations: resolution.conversations,
    });
  } catch (error) {
    mailActionsStoreFor(ctx.store)
      .getState()
      .show({ kind: "error", message: errorMessageOf(error) });
    return;
  }
  ctx.store.getState().openWindow(DELETE_PERMANENTLY_DIALOG);
}

/** Deletes the pending targets permanently and closes the confirmation. */
export async function confirmDeletePermanently(ctx: WriteContext): Promise<void> {
  const what = pendingPermanentDelete(ctx.store);
  pending.delete(ctx.store);
  ctx.store.getState().closeWindow(DELETE_PERMANENTLY_DIALOG);
  if (what === null) return;
  await runMailAction(
    ctx,
    what.targets,
    { type: "delete-permanently" },
    {
      apply: what.messageIds.map((id) => ({ kind: "message", message_id: id })),
      conversations: what.conversations,
    },
  );
}

/** The confirmation window's content. */
export function DeletePermanentlyDialog({ close }: DialogProps) {
  const t = useT();
  const store = useUiStoreApi();
  const client = useClient();
  const queryClient = useQueryClient();
  const [what] = useState(() => pendingPermanentDelete(store));
  const [busy, setBusy] = useState(false);
  const cancel = useRef<HTMLButtonElement>(null);
  const messages = what?.messages ?? 0;
  const conversations = what?.conversations ?? 0;
  // The safe choice has the focus (after the frame focused its first button).
  useEffect(() => {
    queueMicrotask(() => cancel.current?.focus());
  }, []);
  return (
    <div data-part="mail-confirm" data-confirm="delete-permanently" aria-busy={busy}>
      <p data-part="mail-confirm.question">
        {conversations > 0 && messages > 1
          ? t("mail-actions-delete-permanently-conversations", { messages, conversations })
          : t("mail-delete-permanently-question", { count: messages })}
      </p>
      <p data-part="mail-confirm.note">{t("mail-delete-permanently-note")}</p>
      <div data-part="mail-confirm.actions">
        <button
          type="button"
          data-part="form.button"
          data-variant="danger"
          disabled={busy || messages === 0}
          onClick={() => {
            setBusy(true);
            void confirmDeletePermanently({ client, store, queryClient }).finally(() => {
              setBusy(false);
            });
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
