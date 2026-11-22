/**
 * The window of the confirmation before an action on a whole list
 * (`./bulk.ts`): the true count and the action as the verb, in the shared
 * `ConfirmDialog` (R6, ADR 0239). Deleting or moving to Trash is
 * destructive (red, the focus on Cancel, Enter never runs it); every other
 * action is a normal confirmation (the verb has the focus, Enter runs it).
 */
import { useState } from "react";
import { useT } from "../../i18n/I18nProvider";
import { useUiStoreApi } from "../../state/uiStore";
import { ConfirmDialog } from "../../widgets/dialog";
import { answerBulk, bulkLabelArgs, pendingBulk } from "./bulk";

/** The confirmation window's content. */
export function BulkConfirmDialog() {
  const t = useT();
  const store = useUiStoreApi();
  const [question] = useState(() => pendingBulk(store));
  const label = question === null ? null : bulkLabelArgs(question.action);
  const messages = question?.resolution.messages ?? 0;
  const conversations = question?.resolution.conversations ?? 0;
  const type = question?.action.type;
  const destructive = type === "delete-permanently" || type === "trash";
  return (
    <ConfirmDialog
      kind="bulk"
      risk={destructive ? "destructive" : "normal"}
      question={
        conversations > 0
          ? t("mail-bulk-question-conversations", { conversations, messages })
          : t("mail-bulk-question-messages", { messages })
      }
      {...(type === "delete-permanently"
        ? { detail: <p data-part="dialog.note">{t("mail-delete-permanently-note")}</p> }
        : {})}
      action={t("mail-bulk-confirm", label ?? { action: "other", on: "yes" })}
      canConfirm={messages > 0}
      onConfirm={() => {
        answerBulk(store, true);
      }}
      cancel={{
        onClick: () => {
          answerBulk(store, false);
        },
      }}
    />
  );
}
