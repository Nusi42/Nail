/**
 * Opening an attachment the core flagged as risky (ADR 0211; the flags are
 * `MessageWarning` `dangerous-attachment`, ADR 0032): the card does not open
 * it at once but asks in the modal confirmation {@link MAIL_OPEN_ATTACHMENT_DIALOG}
 * ("Open anyway" / "Cancel", Cancel focused), as a flagged link does
 * (`ReaderLinks.tsx`, ADR 0206). Only "Open anyway" runs the card's open.
 */
import { useState } from "react";
import type { AttachmentRisk } from "../api/generated";
import { useT } from "../i18n/I18nProvider";
import type { DialogProps } from "../layout/dialogRegistry";
import { useComposeContext } from "../state/hooks";
import type { UiStore } from "../state/uiStore";
import { MailConfirm, pendingSlot } from "./MailConfirm";

/** The confirmation's window kind (dialog name). */
export const MAIL_OPEN_ATTACHMENT_DIALOG = "mail-open-attachment";

/** A flagged attachment waiting for the user's answer. */
export interface PendingAttachment {
  /** The file name the warning names. */
  name: string;
  /** Why it is flagged. */
  risk: AttachmentRisk;
  /** Opens it (the card's own open, with its status line). */
  run: () => void;
}

const slot = pendingSlot<PendingAttachment>();

/** The attachment the open confirmation is about (`null` = none). */
export function pendingAttachment(store: UiStore): PendingAttachment | null {
  return slot.get(store);
}

/** Opens the confirmation for `what`. */
export function askToOpenAttachment(store: UiStore, what: PendingAttachment): void {
  slot.set(store, what);
  store.getState().openWindow(MAIL_OPEN_ATTACHMENT_DIALOG);
}

/** The confirmation before a flagged attachment opens. */
export function OpenAttachmentDialog({ close }: DialogProps) {
  const t = useT();
  const { store } = useComposeContext();
  const [what] = useState(() => slot.get(store));
  const done = () => {
    slot.clear(store);
    close();
  };
  return (
    <MailConfirm
      kind="open-attachment"
      question={t("mail-open-attachment-question")}
      canOpen={what !== null}
      onOpen={() => {
        done();
        what?.run();
      }}
      onCancel={done}
    >
      {what !== null && (
        <p data-part="mail-confirm.warning">
          {t(`security-risk-${what.risk}`, { name: what.name })}
        </p>
      )}
    </MailConfirm>
  );
}
