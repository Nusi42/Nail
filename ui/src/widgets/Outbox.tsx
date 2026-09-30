import { useState } from "react";
import { errorMessageId, errorMessageIdOf } from "../api/client";
import type { Draft, OutboxEntry } from "../api/generated";
import { useLabel, useLocale, useT } from "../i18n/I18nProvider";
import { type WidgetProps } from "../layout/widgetRegistry";
import { displayName } from "../state/addresses";
import { cancelRichSend, openRichDraft } from "../features/composer/flows";
import { useComposeContext } from "../state/hooks";
import { queryKeys, useDrafts, useOutbox } from "../state/queries";
import { DATE_TIME, formatDateTime } from "../i18n/format";

/** Entries the user can still take back (edit) or send at once. */
export function canTakeBack(entry: OutboxEntry): boolean {
  return entry.state === "scheduled" || entry.state === "failed";
}

/** Messages waiting to be sent: undo-send countdowns, send-later, failures. */
export function Outbox({ node }: WidgetProps) {
  const t = useT();
  const label = useLabel();
  const locale = useLocale();
  const ctx = useComposeContext();
  const outbox = useOutbox();
  const drafts = useDrafts();
  const [error, setError] = useState<string | null>(null);

  const run = async (work: () => Promise<void>) => {
    setError(null);
    try {
      await work();
    } catch (e) {
      setError(errorMessageIdOf(e));
      void ctx.queryClient.invalidateQueries({ queryKey: queryKeys.outbox });
    }
  };

  const edit = (entry: OutboxEntry) => run(() => cancelRichSend(ctx, entry.id));
  const continueDraft = (draft: Draft) =>
    run(async () => {
      if (draft.id === null) return;
      await openRichDraft(ctx, draft.id);
    });
  const discardDraft = (draft: Draft) =>
    run(async () => {
      if (draft.id === null) return;
      await ctx.client.request({ type: "DeleteDraft", draft_id: draft.id }, "Done");
      await ctx.queryClient.invalidateQueries({ queryKey: queryKeys.drafts });
    });
  const sendNow = (entry: OutboxEntry) =>
    run(async () => {
      const { draft_id } = await ctx.client.request(
        { type: "CancelSend", outbox_id: entry.id },
        "SendCancelled",
      );
      const { entry: queued } = await ctx.client.request(
        { type: "SendDraft", draft_id, delay_secs: 0, send_at: null },
        "SendQueued",
      );
      ctx.store.getState().setLastSend(queued);
      await ctx.queryClient.invalidateQueries({ queryKey: queryKeys.outbox });
    });

  const format = (iso: string) => formatDateTime(iso, locale, DATE_TIME);

  const entries = outbox.data ?? [];
  return (
    <section
      data-part="outbox"
      data-node-id={node.id ?? undefined}
      aria-label={node.label ? label(node.label) : t("outbox-label")}
      aria-busy={outbox.isFetching}
    >
      {outbox.isError && (
        <div data-part="outbox.error" role="alert">
          {t(errorMessageIdOf(outbox.error))}
        </div>
      )}
      {error && (
        <div data-part="outbox.error" role="alert">
          {t(error)}
        </div>
      )}
      {outbox.isSuccess && entries.length === 0 && (
        <div data-part="outbox.empty">{t("outbox-empty")}</div>
      )}
      <ul data-part="outbox.list">
        {entries.map((entry) => (
          <li
            key={entry.id}
            data-part="outbox.item"
            data-state={entry.state}
            data-account={entry.account_id}
          >
            <div data-part="outbox.item.subject">{entry.subject || t("message-no-subject")}</div>
            <div data-part="outbox.item.recipients">
              {t("outbox-recipients", {
                recipients: entry.recipients.map((a) => displayName(a)).join(", "),
              })}
            </div>
            <div data-part="outbox.item.status">
              <span data-part="outbox.item.state">{t(`outbox-state-${entry.state}`)}</span>
              <span data-part="outbox.item.time">
                {t("outbox-send-at", { time: format(entry.send_at) })}
              </span>
              {entry.error && (
                <span data-part="outbox.item.error">{t(errorMessageId(entry.error))}</span>
              )}
            </div>
            {canTakeBack(entry) && (
              <div data-part="outbox.item.actions">
                <button
                  type="button"
                  data-part="form.button"
                  data-action="edit"
                  onClick={() => void edit(entry)}
                >
                  {t("outbox-edit")}
                </button>
                <button
                  type="button"
                  data-part="form.button"
                  data-action="send-now"
                  onClick={() => void sendNow(entry)}
                >
                  {t("outbox-send-now")}
                </button>
              </div>
            )}
          </li>
        ))}
      </ul>
      {(drafts.data?.length ?? 0) > 0 && (
        <section data-part="outbox.drafts" aria-label={t("outbox-drafts-label")}>
          <h3 data-part="outbox.drafts.title">{t("outbox-drafts-label")}</h3>
          <ul data-part="outbox.list">
            {(drafts.data ?? []).map((draft) => (
              <li key={draft.id ?? ""} data-part="outbox.item" data-state="draft">
                <div data-part="outbox.item.subject">
                  {draft.subject || t("message-no-subject")}
                </div>
                <div data-part="outbox.item.recipients">
                  {t("outbox-recipients", {
                    recipients: [...draft.to, ...draft.cc].map((a) => displayName(a)).join(", "),
                  })}
                </div>
                <div data-part="outbox.item.actions">
                  <button
                    type="button"
                    data-part="form.button"
                    data-action="continue-draft"
                    onClick={() => void continueDraft(draft)}
                  >
                    {t("outbox-draft-continue")}
                  </button>
                  <button
                    type="button"
                    data-part="form.button"
                    data-action="discard-draft"
                    onClick={() => void discardDraft(draft)}
                  >
                    {t("outbox-draft-discard")}
                  </button>
                </div>
              </li>
            ))}
          </ul>
        </section>
      )}
    </section>
  );
}
