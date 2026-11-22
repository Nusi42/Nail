/**
 * The undo toast (P3, ADR 0164): what an action did — "Conversation
 * archived", "3 conversations moved to Trash", "Message deleted" — with
 * Undo while the batch can be undone (`mail.undo`, `z`, does the same), or
 * why the core refused it. The shared `Toast` widget shows it (look,
 * timing, one toast at a time).
 */
import { useQueryClient } from "@tanstack/react-query";
import { useT } from "../../i18n/I18nProvider";
import { useClient } from "../../state/queries";
import { useUiStoreApi } from "../../state/uiStore";
import { Toast } from "../../widgets/Toast";
import { undoLast } from "./run";
import { useMailActions, type MailToast } from "./store";

/** The toast's text: Fluent id and arguments. */
export function toastMessage(toast: MailToast): {
  id: string;
  args?: Record<string, number | string>;
} {
  switch (toast.kind) {
    case "done":
      if (toast.leftOut !== undefined && toast.leftOut > 0) {
        return {
          id: "mail-actions-done-partial",
          args: { messages: toast.messages, left: toast.leftOut },
        };
      }
      if (toast.tags !== undefined) {
        return {
          id: `mail-actions-done-tags-${toast.tags.change}`,
          args: {
            conversations: toast.conversations,
            messages: toast.messages,
            noun: toast.tags.noun,
            tags: toast.tags.count,
          },
        };
      }
      return {
        id: `mail-actions-done-${toast.action}`,
        args: { conversations: toast.conversations, messages: toast.messages },
      };
    case "undone":
      return { id: "mail-actions-undone" };
    case "error":
      return { id: toast.message.id };
  }
}

export function MailActionToast() {
  const t = useT();
  const toast = useMailActions((s) => s.toast);
  const dismiss = useMailActions((s) => s.dismiss);
  const client = useClient();
  const store = useUiStoreApi();
  const queryClient = useQueryClient();

  if (toast === null) return null;
  const message = toastMessage(toast);
  const text =
    toast.kind === "error" ? t(toast.message.id, toast.message.args) : t(message.id, message.args);
  const batchId = toast.kind === "done" ? toast.batchId : null;
  return (
    <Toast
      kind={toast.kind}
      seq={toast.seq}
      text={text}
      action={toast.kind === "done" ? toast.action : undefined}
      onDismiss={dismiss}
      undo={
        batchId === null
          ? undefined
          : {
              label: t("common-undo"),
              run: () => {
                dismiss();
                void undoLast({ client, store, queryClient }, batchId);
              },
            }
      }
    />
  );
}
