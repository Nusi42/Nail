/**
 * The window of the confirmation before an action on a whole list
 * (`./bulk.ts`): the true count, the action as the button's text, and
 * Cancel focused (`MailConfirm`).
 */
import { useState } from "react";
import { useT } from "../../i18n/I18nProvider";
import { useUiStoreApi } from "../../state/uiStore";
import { MailConfirm } from "../../widgets/MailConfirm";
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
    <MailConfirm
      kind="bulk"
      {...(type === undefined ? {} : { dataRole: type })}
      variant={destructive ? "danger" : "primary"}
      question={
        conversations > 0
          ? t("mail-bulk-question-conversations", { conversations, messages })
          : t("mail-bulk-question-messages", { messages })
      }
      confirmLabel={t("mail-bulk-confirm", label ?? { action: "other", on: "yes" })}
      canConfirm={messages > 0}
      onConfirm={() => {
        answerBulk(store, true);
      }}
      onCancel={() => {
        answerBulk(store, false);
      }}
    >
      {type === "delete-permanently" && (
        <p data-part="mail-confirm.note">{t("mail-delete-permanently-note")}</p>
      )}
    </MailConfirm>
  );
}
