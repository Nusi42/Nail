/**
 * Undo for invitation answers (R8, ADR 0251; ADR 0069): an answer the
 * core sends as iMIP mail waits in the outbox for the undo-send delay like
 * any message. Its toast says so and offers Undo, which takes the mail back
 * out of the outbox (`CancelSend`) and throws its automatic draft away
 * (`DeleteDraft`) — the answer stays in the calendar, only the mail to the
 * organizer is not sent. The toast goes once the mail left the outbox.
 */
import { useEffect, useState } from "react";
import { errorMessageOf } from "../../api/client";
import { useT } from "../../i18n/I18nProvider";
import { queryKeys, useOutbox } from "../../state/queries";
import { useChangeContext } from "./common";
import { pimStoreFor, usePim, type PimToast } from "./store";
import { TextToast } from "./TextToast";
import type { SchedulingResult } from "../../api/generated";
import type { UiStore } from "../../state/uiStore";

/** How long after its send time the toast looks whether the mail left (ms). */
const SENT_CHECK_MS = 250;

/** The toast id of the answer waiting as outbox entry `outboxId`. */
export const replyToastId = (outboxId: string) => `reply-${outboxId}`;

export function ReplyToast({ toast }: { toast: Extract<PimToast, { kind: "reply" }> }) {
  const t = useT();
  const ctx = useChangeContext();
  const dismissToast = usePim((s) => s.dismissToast);
  const outbox = useOutbox();
  const entry = outbox.data?.find((e) => e.id === toast.outboxId);
  // Sent (or gone): nothing to undo any more. A list from before the answer does not count.
  const [shownAt] = useState(() => Date.now());
  const fresh = outbox.isSuccess && outbox.dataUpdatedAt >= shownAt;
  const gone = toast.state === "waiting" && fresh && entry?.state !== "scheduled";
  useEffect(() => {
    if (gone) dismissToast(toast.id);
  }, [gone, dismissToast, toast.id]);
  // When the undo-send delay is over, look again: the mail is on its way then.
  const sendAt = entry?.state === "scheduled" ? entry.send_at : null;
  useEffect(() => {
    if (sendAt === null) return;
    const timer = setTimeout(
      () => {
        void ctx.queryClient.invalidateQueries({ queryKey: queryKeys.outbox });
      },
      Math.max(0, Date.parse(sendAt) - Date.now()) + SENT_CHECK_MS,
    );
    return () => {
      clearTimeout(timer);
    };
  }, [sendAt, ctx.queryClient]);

  const undo = async () => {
    const pim = ctx.pim.getState();
    try {
      const { draft_id } = await ctx.client.request(
        { type: "CancelSend", outbox_id: toast.outboxId },
        "SendCancelled",
      );
      await ctx.client.request({ type: "DeleteDraft", draft_id }, "Done");
      pim.pushToast({ ...toast, state: "undone" });
    } catch (error) {
      pim.pushToast({ kind: "problem", id: toast.id, message: errorMessageOf(error) });
    } finally {
      void ctx.queryClient.invalidateQueries({ queryKey: queryKeys.outbox });
    }
  };

  const waiting = toast.state === "waiting";
  return (
    <TextToast
      id={toast.id}
      kind="reply"
      state={toast.state}
      role="status"
      text={t(waiting ? "pim-act-reply-waiting" : "pim-act-reply-undone")}
    >
      {waiting && (
        <button
          type="button"
          data-part="form.button"
          data-variant="primary"
          data-action="undo"
          onClick={() => void undo()}
        >
          {t("common-undo")}
        </button>
      )}
    </TextToast>
  );
}

/** Offers Undo for every answer mail a scheduling write queued (`SchedulingResult.sent`). */
export function offerReplyUndo(store: UiStore, result: SchedulingResult): void {
  const pim = pimStoreFor(store).getState();
  for (const sent of result.sent) {
    if (sent.outbox_id === null) continue;
    pim.pushToast({
      kind: "reply",
      id: replyToastId(sent.outbox_id),
      outboxId: sent.outbox_id,
      state: "waiting",
    });
  }
}
