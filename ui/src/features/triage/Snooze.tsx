/**
 * Snooze: the picker dialog (`snooze`, opened by `snooze.open`) with quick
 * choices and a date/time field, and the list of snoozed messages (widget
 * `snooze-list`, window `snoozed`) with "Wake up now".
 *
 * Snoozing is a queued write (keyword `$Snoozed` + move to the hidden
 * Snoozed folder); the core wakes the message at its time, offline too.
 */
import { useQueryClient } from "@tanstack/react-query";
import { useId, useMemo, useState, type SubmitEvent } from "react";
import { errorMessageOf } from "../../api/client";
import type { LocalizedMessage, SnoozedMessage } from "../../api/generated";
import { useLabel, useLocale, useT } from "../../i18n/I18nProvider";
import type { DialogProps } from "../../layout/dialogRegistry";
import type { WidgetProps } from "../../layout/widgetRegistry";
import { useNow } from "../../state/clock";
import { useClient } from "../../state/queries";
import { useUiStoreApi } from "../../state/uiStore";
import {
  SNOOZE_PRESETS,
  fromLocalInput,
  presetTime,
  toLocalInput,
  toRfc3339,
  type SnoozePreset,
} from "./model";
import { threadIdsOf } from "../../actions/mailTargets";
import { runMailAction } from "../mail-actions/run";
import { snooze, triageKeys, unsnooze, useSnoozed } from "./queries";
import { useTriage } from "./store";
import { formatDateTime } from "../../i18n/format";
import { nonEmpty } from "../../text/empty";

/** Name of the snooze picker dialog and of the snoozed-list window. */
export const SNOOZE_DIALOG = "snooze";
export const SNOOZED_WINDOW = "snoozed";

function useWriteContext() {
  return { client: useClient(), store: useUiStoreApi(), queryClient: useQueryClient() };
}

function formatTime(date: Date, locale: string): string {
  return formatDateTime(date, locale, {
    weekday: "short",
    day: "numeric",
    month: "short",
    hour: "2-digit",
    minute: "2-digit",
  });
}

/** Whether a wake time has already passed. */
function isPast(date: Date): boolean {
  return date.getTime() <= Date.now();
}

function Problem({ message }: { message: LocalizedMessage | null }) {
  const t = useT();
  if (message === null) return null;
  return (
    <p data-part="snooze.error" role="alert">
      {t(message.id, message.args)}
    </p>
  );
}

/** The snooze picker (dialog `snooze`). */
export function SnoozeDialog({ close }: DialogProps) {
  const t = useT();
  const locale = useLocale();
  const ctx = useWriteContext();
  const ids = useTriage((s) => s.snoozing);
  const targets = useTriage((s) => s.targets);
  const conversations = targets === null ? 0 : threadIdsOf(targets.targets).length;
  const nowMs = useNow();
  const now = useMemo(() => new Date(nowMs), [nowMs]);
  const fieldId = useId();
  const [custom, setCustom] = useState(() => {
    const tomorrow = presetTime("tomorrow", now);
    return tomorrow ? toLocalInput(tomorrow) : "";
  });
  const [problem, setProblem] = useState<LocalizedMessage | null>(null);
  const [busy, setBusy] = useState(false);

  const run = async (date: Date) => {
    if (isPast(date)) {
      setProblem({ id: "snooze-error-past" });
      return;
    }
    setBusy(true);
    setProblem(null);
    try {
      if (targets === null) {
        await snooze(ctx, ids, toRfc3339(date));
        close();
      } else {
        // P3: one batch with one undo (the toast reports a refusal).
        const result = await runMailAction(ctx, targets, {
          type: "snooze",
          until: toRfc3339(date),
          wake_folder_id: null,
        });
        void ctx.queryClient.invalidateQueries({ queryKey: triageKeys.snoozed });
        if (result !== null) close();
      }
    } catch (error) {
      setProblem(errorMessageOf(error));
    } finally {
      setBusy(false);
    }
  };

  const submit = (event: SubmitEvent) => {
    event.preventDefault();
    const date = fromLocalInput(custom);
    if (date === null) setProblem({ id: "snooze-error-time" });
    else void run(date);
  };

  if (ids.length === 0) {
    return (
      <div data-part="snooze" data-state="empty">
        <p data-part="snooze.empty">{t("snooze-nothing-selected")}</p>
      </div>
    );
  }

  return (
    <form data-part="snooze" onSubmit={submit} aria-busy={busy} noValidate>
      <p data-part="snooze.intro">
        {conversations > 0 && conversations === ids.length
          ? t("snooze-intro-conversations", { count: conversations })
          : t("snooze-intro", { count: ids.length })}
      </p>
      <ul data-part="snooze.presets" aria-label={t("snooze-presets-label")}>
        {SNOOZE_PRESETS.map((preset: SnoozePreset) => {
          const time = presetTime(preset, now);
          if (time === null) return null;
          return (
            <li key={preset} data-part="snooze.preset.item">
              <button
                type="button"
                data-part="snooze.preset"
                data-preset={preset}
                disabled={busy}
                onClick={() => void run(time)}
              >
                <span data-part="snooze.preset.label">{t(`snooze-preset-${preset}`)}</span>
                <span data-part="snooze.preset.time">{formatTime(time, locale)}</span>
              </button>
            </li>
          );
        })}
      </ul>
      <div data-part="form.field" data-field="until">
        <label data-part="form.label" htmlFor={fieldId}>
          {t("snooze-custom-label")}
        </label>
        <input
          id={fieldId}
          data-part="form.input"
          type="datetime-local"
          value={custom}
          min={toLocalInput(now)}
          onChange={(event) => {
            setCustom(event.target.value);
          }}
        />
      </div>
      <Problem message={problem} />
      <div data-part="snooze.actions">
        <button type="submit" data-part="form.button" data-variant="primary" disabled={busy}>
          {t("snooze-submit")}
        </button>
        <button type="button" data-part="form.button" onClick={close}>
          {t("snooze-cancel")}
        </button>
      </div>
    </form>
  );
}

function SnoozedItem({
  item,
  onWake,
  busy,
}: {
  item: SnoozedMessage;
  onWake: () => void;
  busy: boolean;
}) {
  const t = useT();
  const locale = useLocale();
  const until = new Date(item.until);
  const sender =
    nonEmpty(item.message.from?.name) ??
    nonEmpty(item.message.from?.email) ??
    t("snooze-unknown-sender");
  return (
    <li data-part="snooze-list.item" data-message={item.message.id}>
      <span data-part="snooze-list.subject">{item.message.subject || t("message-no-subject")}</span>
      <span data-part="snooze-list.from">{sender}</span>
      <span data-part="snooze-list.until">
        {t("snooze-list-until", {
          time: Number.isNaN(until.getTime()) ? item.until : formatTime(until, locale),
        })}
      </span>
      <button
        type="button"
        data-part="form.button"
        data-action="wake"
        disabled={busy}
        aria-label={t("snooze-list-wake-label", { subject: item.message.subject })}
        onClick={onWake}
      >
        {t("snooze-list-wake")}
      </button>
    </li>
  );
}

/** The snoozed messages of every account (widget `snooze-list`). */
export function SnoozeList({ node }: Partial<WidgetProps>) {
  const t = useT();
  const label = useLabel();
  const ctx = useWriteContext();
  const snoozed = useSnoozed();
  const [busy, setBusy] = useState<string | null>(null);
  const [problem, setProblem] = useState<LocalizedMessage | null>(null);

  const wake = async (id: string) => {
    setBusy(id);
    setProblem(null);
    try {
      await unsnooze(ctx, [id]);
    } catch (error) {
      setProblem(errorMessageOf(error));
    } finally {
      setBusy(null);
    }
  };

  const items = [...(snoozed.data ?? [])].sort((a, b) => a.until.localeCompare(b.until));
  return (
    <section
      data-part="snooze-list"
      data-node-id={node?.id ?? undefined}
      aria-label={node?.label ? label(node.label) : t("snooze-list-label")}
      aria-busy={snoozed.isFetching}
    >
      {snoozed.isError && <Problem message={errorMessageOf(snoozed.error)} />}
      <Problem message={problem} />
      {snoozed.isSuccess && items.length === 0 && (
        <p data-part="snooze-list.empty">{t("snooze-list-empty")}</p>
      )}
      {items.length > 0 && (
        <ul data-part="snooze-list.items">
          {items.map((item) => (
            <SnoozedItem
              key={item.message.id}
              item={item}
              busy={busy === item.message.id}
              onWake={() => void wake(item.message.id)}
            />
          ))}
        </ul>
      )}
    </section>
  );
}

/** The snoozed list as a dialog, when the layout has no `snoozed` window. */
export function SnoozedDialog() {
  return <SnoozeList />;
}
