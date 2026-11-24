/**
 * Reminder toasts: a reminder that fires (`ReminderDue`, or one already
 * fired when the UI starts) shows as an in-app toast with "Open", snooze
 * choices and "Dismiss" (plus "Dismiss all"). Snoozing and dismissing go
 * to the core, which syncs them (RFC 9074) where the server keeps them.
 * PIM writes the server refused (`PimOpStatus` failed) show here too. The
 * desktop shell's OS notification and the remote UI's Web Notification
 * come from the core's `NotificationRequested` (triage feature).
 */
import { useEffect, useState } from "react";
import type { Reminder } from "../../api/generated";
import { useDateFormat, type DateFormatter } from "../../i18n/format";
import { useT } from "../../i18n/I18nProvider";
import { useNow } from "../../state/clock";
import { colorOf, geomId, Problem, problemOf, Swatch, useChangeContext } from "./common";
import { lengthText } from "./eventLook";
import { formatInstant } from "./fmt";
import { EVENT_DIALOG, TASK_DIALOG } from "./names";
import { PimMenu } from "./PimMenu";
import { ReplyToast } from "./ReplyToast";
import { TextToast } from "./TextToast";
import { dismissReminders, snoozeReminders, useCalendars, useReminders } from "./queries";
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

/** The snooze the split button's main part offers. */
export const DEFAULT_SNOOZE: SnoozeChoice = "10";

/** "10 minutes" / "Tomorrow morning": a snooze choice as text. */
function snoozeText(t: ReturnType<typeof useT>, locale: string, choice: SnoozeChoice): string {
  return choice === "tomorrow" ? t("pim-snooze-tomorrow") : lengthText(t, locale, Number(choice));
}

/**
 * When a reminder's item starts, relative to now ("in 30 minutes", "now",
 * "5 minutes ago"; all-day items "today" / "tomorrow"), `null` without a
 * start.
 */
export function reminderWhen(
  format: DateFormatter,
  reminder: Reminder,
  now: number,
): string | null {
  if (reminder.starts_at === null) return null;
  return format.relative(new Date(instantMs(reminder.starts_at)), now);
}

/**
 * A fired reminder (R7 C3, ADR 0246): the title with how soon it starts
 * ("in 30 minutes"), the time and the calendar's name, the calendar's
 * colour on the toast's edge, Dismiss as the primary action, snoozing as
 * one split button (the main part snoozes for ten minutes, its menu offers
 * the other times) and Open.
 */
function ReminderToast({ toast }: { toast: Extract<PimToast, { kind: "reminder" }> }) {
  const t = useT();
  const format = useDateFormat();
  const ctx = useChangeContext();
  const calendars = useCalendars();
  const now = useNow();
  const dismissToast = usePim((s) => s.dismissToast);
  const [problem, setProblem] = useState<ReturnType<typeof problemOf> | null>(null);
  const r = toast.reminder;
  const tz = localZone();
  const calendar = calendars.data?.find((c) => c.id === r.calendar_id) ?? null;
  const color = colorOf(calendars.data, r.calendar_id, null);
  const time = r.starts_at
    ? r.all_day
      ? formatInstant(format, r.starts_at, tz, { dateStyle: "full" })
      : formatInstant(format, r.starts_at, tz, {
          weekday: "short",
          hour: "numeric",
          minute: "2-digit",
        })
    : null;
  const relative = reminderWhen(format, r, now);
  const calendarName = calendar?.settings.name ?? null;
  const meta =
    time !== null && calendarName !== null
      ? t("pim-look-reminder-meta", { time, calendar: calendarName })
      : (time ?? calendarName);

  const act = async (fn: () => Promise<unknown>) => {
    setProblem(null);
    try {
      await fn();
      dismissToast(toast.id);
    } catch (error) {
      setProblem(problemOf(error));
    }
  };
  const snooze = (choice: SnoozeChoice) =>
    void act(() => snoozeReminders(ctx, [r.id], snoozeUntil(choice, Date.now(), tz)));

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
        <Swatch geom={geomId("reminder", r.id, color)} color={color} part="pim.reminder.edge" />
        <div data-part="pim.reminder.head">
          <p data-part="pim.toast.title" role="alert">
            {r.target.type === "task"
              ? t("pim-reminder-task", { title: r.title })
              : t("pim-reminder-event", { title: r.title })}
          </p>
          {relative && <span data-part="pim.reminder.when">{relative}</span>}
        </div>
        {meta && <p data-part="pim.reminder.meta">{meta}</p>}
        {r.location && <p data-part="pim.toast.location">{r.location}</p>}
        <Problem message={problem} />
        <div data-part="pim.reminder.actions">
          <button
            type="button"
            data-part="form.button"
            data-action="dismiss"
            data-variant="primary"
            onClick={() => void act(() => dismissReminders(ctx, [r.id]))}
          >
            {t("common-dismiss")}
          </button>
          <div data-part="pim.reminder.split" role="group" aria-label={t("common-snooze")}>
            <button
              type="button"
              data-part="form.button"
              data-action="snooze"
              onClick={() => {
                snooze(DEFAULT_SNOOZE);
              }}
            >
              {t("pim-look-snooze", { length: snoozeText(t, format.locale, DEFAULT_SNOOZE) })}
            </button>
            <PimMenu
              label={t("pim-look-snooze-more")}
              icon="chevron-down"
              field="reminder-snooze"
              part="pim.reminder.more"
              entries={SNOOZE_CHOICES.map((choice) => ({
                type: "item" as const,
                id: choice,
                label: snoozeText(t, format.locale, choice),
                onSelect: () => {
                  snooze(choice);
                },
              }))}
            />
          </div>
          <button
            type="button"
            data-part="form.button"
            data-action="open"
            data-variant="ghost"
            onClick={open}
          >
            {t("common-open")}
          </button>
        </div>
      </div>
    </li>
  );
}

function ProblemToast({ toast }: { toast: Extract<PimToast, { kind: "problem" }> }) {
  const t = useT();
  return (
    <TextToast
      id={toast.id}
      kind="problem"
      role="alert"
      text={t(toast.message.id, toast.message.args)}
    />
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
      <section data-part="pim.toasts" aria-label={t("pim-alarms-legend")}>
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
            ) : toast.kind === "reply" ? (
              <ReplyToast key={toast.id} toast={toast} />
            ) : (
              <ProblemToast key={toast.id} toast={toast} />
            ),
          )}
        </ul>
      </section>
    </>
  );
}
