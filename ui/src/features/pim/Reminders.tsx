/**
 * Reminder toasts: a reminder that fires (`ReminderDue`, or one already
 * fired when the UI starts) shows as an in-app toast with "Open", snooze
 * choices and "Dismiss" (plus "Dismiss all"). Snoozing and dismissing go
 * to the core, which syncs them (RFC 9074) where the server keeps them.
 * PIM writes the server refused (`PimOpStatus` failed) show here too. The
 * desktop shell's OS notification and the remote UI's Web Notification
 * come from the core's `NotificationRequested` (triage feature).
 */
import { useEffect, useId, useState } from "react";
import type { Reminder } from "../../api/generated";
import { useDateFormat } from "../../i18n/format";
import { useT } from "../../i18n/I18nProvider";
import { useNow } from "../../state/clock";
import { Problem, problemOf, useChangeContext } from "./common";
import { formatInstant } from "./fmt";
import { EVENT_DIALOG, TASK_DIALOG } from "./names";
import { dismissReminders, snoozeReminders, useReminders } from "./queries";
import { usePim, type PimToast } from "./store";
import { instantMs, localZone, todayIn, toZoned, utcString } from "./time";

/** Snooze choices: minutes, or `tomorrow` (09:00 the next day). */
export const SNOOZE_CHOICES = ["5", "10", "15", "30", "60", "tomorrow"] as const;
export type SnoozeChoice = (typeof SNOOZE_CHOICES)[number];

/** When a snooze choice ends. */
export function snoozeUntil(choice: SnoozeChoice, now: number, tz: string): string {
  if (choice === "tomorrow") {
    return utcString(toZoned(todayIn(tz, now).add({ days: 1 }), tz).add({ hours: 9 }));
  }
  return utcString(now + Number(choice) * 60_000);
}

/** Whether a listed reminder should show as a toast now. */
export function isDue(reminder: Reminder, now: number): boolean {
  if (reminder.state === "fired") return true;
  if (reminder.state === "snoozed")
    return reminder.snoozed_until !== null && instantMs(reminder.snoozed_until) <= now;
  return false;
}

/** Background: shows reminders already due when the UI starts. */
export function ReminderStartup() {
  const reminders = useReminders();
  const push = usePim((s) => s.pushToast);
  const now = useNow();
  useEffect(() => {
    for (const reminder of reminders.data ?? []) {
      if (isDue(reminder, now)) push({ kind: "reminder", id: `reminder-${reminder.id}`, reminder });
    }
  }, [reminders.data, push, now]);
  return null;
}

function ReminderToast({ toast }: { toast: Extract<PimToast, { kind: "reminder" }> }) {
  const t = useT();
  const format = useDateFormat();
  const ctx = useChangeContext();
  const dismissToast = usePim((s) => s.dismissToast);
  const select = useId();
  const [choice, setChoice] = useState<SnoozeChoice>("10");
  const [problem, setProblem] = useState<ReturnType<typeof problemOf> | null>(null);
  const r = toast.reminder;
  const tz = localZone();
  const when = r.starts_at
    ? r.all_day
      ? formatInstant(format, r.starts_at, tz, { dateStyle: "full" })
      : formatInstant(format, r.starts_at, tz, {
          weekday: "short",
          hour: "numeric",
          minute: "2-digit",
        })
    : null;

  const act = async (fn: () => Promise<unknown>) => {
    setProblem(null);
    try {
      await fn();
      dismissToast(toast.id);
    } catch (error) {
      setProblem(problemOf(error));
    }
  };

  const open = () => {
    const pim = ctx.pim.getState();
    if (r.target.type === "event") {
      pim.editEvent({
        mode: "edit",
        eventId: r.target.event_id,
        recurrenceId: r.target.recurrence_id,
        start: null,
        end: null,
      });
      ctx.store.getState().openWindow(EVENT_DIALOG);
    } else {
      pim.editTask({ mode: "edit", taskId: r.target.task_id });
      ctx.store.getState().openWindow(TASK_DIALOG);
    }
  };

  return (
    <li data-part="pim.toast" data-kind="reminder" data-reminder={r.id}>
      <div
        data-part="pim.toast.body"
        role="group"
        aria-label={t("pim-reminder-label", { title: r.title })}
      >
        <p data-part="pim.toast.title" role="alert">
          {r.target.type === "task"
            ? t("pim-reminder-task", { title: r.title })
            : t("pim-reminder-event", { title: r.title })}
        </p>
        {when && <p data-part="pim.toast.time">{t("pim-reminder-when", { time: when })}</p>}
        {r.location && <p data-part="pim.toast.location">{r.location}</p>}
        <Problem message={problem} />
        <div data-part="pim.toast.actions">
          <button type="button" data-part="form.button" onClick={open}>
            {t("pim-reminder-open")}
          </button>
          <label data-part="form.label" htmlFor={select}>
            {t("pim-reminder-snooze-for")}
          </label>
          <select
            id={select}
            data-part="form.select"
            value={choice}
            onChange={(e) => {
              setChoice(e.target.value as SnoozeChoice);
            }}
          >
            {SNOOZE_CHOICES.map((c) => (
              <option key={c} value={c}>
                {c === "tomorrow"
                  ? t("pim-snooze-tomorrow")
                  : t("pim-snooze-minutes", { minutes: Number(c) })}
              </option>
            ))}
          </select>
          <button
            type="button"
            data-part="form.button"
            data-action="snooze"
            onClick={() =>
              void act(() => snoozeReminders(ctx, [r.id], snoozeUntil(choice, Date.now(), tz)))
            }
          >
            {t("pim-reminder-snooze")}
          </button>
          <button
            type="button"
            data-part="form.button"
            data-action="dismiss"
            data-variant="primary"
            onClick={() => void act(() => dismissReminders(ctx, [r.id]))}
          >
            {t("pim-reminder-dismiss")}
          </button>
        </div>
      </div>
    </li>
  );
}

function ProblemToast({ toast }: { toast: Extract<PimToast, { kind: "problem" }> }) {
  const t = useT();
  const dismissToast = usePim((s) => s.dismissToast);
  return (
    <li data-part="pim.toast" data-kind="problem">
      <p data-part="pim.toast.title" role="alert">
        {t(toast.message.id, toast.message.args)}
      </p>
      <div data-part="pim.toast.actions">
        <button
          type="button"
          data-part="form.button"
          onClick={() => {
            dismissToast(toast.id);
          }}
        >
          {t("pim-close")}
        </button>
      </div>
    </li>
  );
}

/** The toast stack (an effect component: mounted once in the app shell). */
export function PimToasts() {
  const t = useT();
  const ctx = useChangeContext();
  const toasts = usePim((s) => s.toasts);
  const reminders = toasts.filter(
    (x): x is Extract<PimToast, { kind: "reminder" }> => x.kind === "reminder",
  );
  if (toasts.length === 0) return <ReminderStartup />;
  return (
    <>
      <ReminderStartup />
      <section data-part="pim.toasts" aria-label={t("pim-toasts-label")}>
        {reminders.length > 1 && (
          <button
            type="button"
            data-part="form.button"
            data-action="dismiss-all"
            onClick={() => {
              void dismissReminders(
                ctx,
                reminders.map((r) => r.reminder.id),
              ).then(() => {
                for (const r of reminders) ctx.pim.getState().dismissToast(r.id);
              });
            }}
          >
            {t("pim-reminder-dismiss-all", { count: reminders.length })}
          </button>
        )}
        <ul data-part="pim.toasts.list">
          {toasts.map((toast) =>
            toast.kind === "reminder" ? (
              <ReminderToast key={toast.id} toast={toast} />
            ) : (
              <ProblemToast key={toast.id} toast={toast} />
            ),
          )}
        </ul>
      </section>
    </>
  );
}
