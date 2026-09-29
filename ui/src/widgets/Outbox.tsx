import { useState } from "react";
import { errorMessageId, errorMessageIdOf } from "../api/client";
import type { OutboxEntry } from "../api/generated";
import { useLabel, useLocale, useT } from "../i18n/I18nProvider";
import { type WidgetProps } from "../layout/widgetRegistry";
import { displayName } from "../state/addresses";
import { cancelSend } from "../state/compose";
import { useComposeContext } from "../state/hooks";
import { queryKeys, useOutbox } from "../state/queries";

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

  const edit = (entry: OutboxEntry) => run(() => cancelSend(ctx, entry.id));
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

  const format = (iso: string) => {
    const date = new Date(iso);
    return Number.isNaN(date.getTime())
      ? iso
      : new Intl.DateTimeFormat(locale, { dateStyle: "medium", timeStyle: "short" }).format(date);
  };

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
    </section>
  );
}
