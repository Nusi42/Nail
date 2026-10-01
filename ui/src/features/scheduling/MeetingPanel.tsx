/**
 * Scheduling in the event editor (mount point `MeetingPanel`, for a saved
 * event with attendees):
 *
 * - organizer: who answered what, "send invitations / the update" to
 *   everyone or chosen attendees with a note, "cancel the meeting" (dialog
 *   `scheduling-cancel-meeting`: the whole meeting, one occurrence, or
 *   some attendees);
 * - attendee: Accept / Maybe / Decline (for the series or this occurrence)
 *   with a comment, and "propose a new time".
 *
 * Plus the cancel dialog and the scheduling settings dialog.
 */
import { useQueryClient } from "@tanstack/react-query";
import { useId, useState } from "react";
import type {
  CalendarEvent,
  CancelScope,
  InvitationResponse,
  InvitationTarget,
  LocalizedMessage,
  PimTime,
  SchedulingSettings,
} from "../../api/generated";
import { errorMessageOf } from "../../api/client";
import { useDateFormat } from "../../i18n/format";
import { useT } from "../../i18n/I18nProvider";
import type { DialogProps } from "../../layout/dialogRegistry";
import { useClient } from "../../state/queries";
import { useUiStoreApi } from "../../state/uiStore";
import { Problem } from "../pim/common";
import { useCalendars } from "../pim/queries";
import { localZone } from "../pim/time";
import { AnswerForm, ProposeTimeForm } from "./Answer";
import { timeRangeText } from "./fmt";
import { CANCEL_SCOPES, SETTING_SWITCHES } from "./messages";
import { answerCounts, isMine, RESPONSES } from "./model";
import { CANCEL_DIALOG } from "./names";
import {
  scheduleWrite,
  schedulingKeys,
  useMyAddresses,
  useSchedulingSettings,
  type SchedulingWrite,
} from "./queries";
import { schedulingStoreFor, useScheduling } from "./store";

export interface MeetingPanelProps {
  event: CalendarEvent;
  /** The occurrence the editor shows, `null` = the series / a single event. */
  recurrenceId: PimTime | null;
}

function useRunner() {
  const client = useClient();
  const queryClient = useQueryClient();
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState<LocalizedMessage | null>(null);
  const [done, setDone] = useState<LocalizedMessage | null>(null);
  const run = async (request: SchedulingWrite, success: LocalizedMessage): Promise<boolean> => {
    setBusy(true);
    setProblem(null);
    setDone(null);
    try {
      await scheduleWrite({ client, queryClient }, request);
      setDone(success);
      return true;
    } catch (error) {
      setProblem(errorMessageOf(error));
      return false;
    } finally {
      setBusy(false);
    }
  };
  return { busy, problem, done, run };
}

/** The meeting panel; renders nothing for events without attendees. */
export function MeetingPanel({ event, recurrenceId }: MeetingPanelProps) {
  const mine = useMyAddresses();
  if (event.fields.attendees.length === 0) return null;
  if (isMine(event.fields.organizer?.email, mine)) {
    return <OrganizerPanel event={event} recurrenceId={recurrenceId} mine={mine} />;
  }
  if (event.fields.attendees.some((a) => isMine(a.email, mine))) {
    return <AttendeePanel event={event} recurrenceId={recurrenceId} mine={mine} />;
  }
  return null;
}

function OrganizerPanel({
  event,
  recurrenceId,
  mine,
}: MeetingPanelProps & { mine: readonly string[] }) {
  const t = useT();
  const store = useUiStoreApi();
  const ids = { comment: useId(), who: useId() };
  const { busy, problem, done, run } = useRunner();
  const [who, setWho] = useState<"everyone" | "chosen">("everyone");
  const [chosen, setChosen] = useState<string[]>([]);
  const [comment, setComment] = useState("");
  const others = event.fields.attendees.filter((a) => !isMine(a.email, mine));
  const counts = answerCounts(others.map((a) => a.status));

  return (
    <section
      data-part="scheduling.meeting"
      data-role="organizer"
      aria-label={t("scheduling-meeting-label", {
        title: event.fields.title || t("invitation-untitled"),
      })}
    >
      <p data-part="scheduling.meeting.summary">
        {t("invitation-attendees-summary", {
          count: others.length,
          accepted: counts.accepted,
          declined: counts.declined,
          tentative: counts.tentative,
          pending: counts["needs-action"],
        })}
      </p>
      <fieldset data-part="scheduling.meeting.recipients">
        <legend data-part="form.section.title">{t("scheduling-send-legend")}</legend>
        {(["everyone", "chosen"] as const).map((value) => (
          <label key={value} data-part="form.check" data-choice={value}>
            <input
              type="radio"
              name={ids.who}
              checked={who === value}
              onChange={() => setWho(value)}
            />
            <span>{t(`scheduling-send-to-${value}`)}</span>
          </label>
        ))}
        {who === "chosen" && (
          <ul data-part="scheduling.meeting.people">
            {others.map((a) => (
              <li key={a.email} data-part="scheduling.meeting.person">
                <label data-part="form.check">
                  <input
                    type="checkbox"
                    checked={chosen.includes(a.email)}
                    onChange={(e) =>
                      setChosen((c) =>
                        e.target.checked ? [...c, a.email] : c.filter((x) => x !== a.email),
                      )
                    }
                  />
                  <span>
                    {a.name ? t("invitation-person", { name: a.name, email: a.email }) : a.email}
                  </span>
                </label>
              </li>
            ))}
          </ul>
        )}
      </fieldset>
      <div data-part="form.field" data-field="note">
        <label data-part="form.label" htmlFor={ids.comment}>
          {t("scheduling-send-note")}
        </label>
        <textarea
          id={ids.comment}
          data-part="form.textarea"
          value={comment}
          onChange={(e) => setComment(e.target.value)}
        />
      </div>
      <div data-part="scheduling.meeting.actions">
        <button
          type="button"
          data-part="form.button"
          data-variant="primary"
          data-action="send-invitations"
          disabled={busy || (who === "chosen" && chosen.length === 0)}
          onClick={() =>
            void run(
              {
                type: "SendInvitations",
                event_id: event.id,
                recurrence_id: recurrenceId,
                attendees: who === "chosen" ? chosen : [],
                comment: comment.trim() || null,
              },
              { id: "scheduling-sent-invitations" },
            )
          }
        >
          {t("scheduling-send-invitations")}
        </button>
        <button
          type="button"
          data-part="form.button"
          data-variant="danger"
          data-action="cancel-meeting"
          disabled={busy}
          onClick={() => {
            schedulingStoreFor(store)
              .getState()
              .setCancelTarget({
                eventId: event.id,
                title: event.fields.title,
                recurrenceId,
                recurring: event.recurrence !== null,
                attendees: others,
              });
            store.getState().openWindow(CANCEL_DIALOG);
          }}
        >
          {t("scheduling-cancel-meeting")}
        </button>
      </div>
      <Problem message={problem} part="scheduling.problem" />
      <p data-part="scheduling.done" role="status">
        {done ? t(done.id, done.args) : ""}
      </p>
    </section>
  );
}

function AttendeePanel({
  event,
  recurrenceId,
  mine,
}: MeetingPanelProps & { mine: readonly string[] }) {
  const t = useT();
  const settings = useSchedulingSettings();
  const { busy, problem, done, run } = useRunner();
  const [onlyThis, setOnlyThis] = useState(recurrenceId !== null);
  const [proposing, setProposing] = useState(false);
  const me = event.fields.attendees.find((a) => isMine(a.email, mine));
  const occurrence = event.recurrence !== null && recurrenceId !== null;
  const target: InvitationTarget = {
    type: "event",
    event_id: event.id,
    recurrence_id: occurrence && onlyThis ? recurrenceId : null,
  };
  const organizer = event.fields.organizer
    ? (event.fields.organizer.name ?? event.fields.organizer.email)
    : null;
  return (
    <section
      data-part="scheduling.meeting"
      data-role="attendee"
      aria-label={t("scheduling-meeting-label", {
        title: event.fields.title || t("invitation-untitled"),
      })}
    >
      {organizer && (
        <p data-part="scheduling.meeting.organizer">
          {t("scheduling-meeting-organized-by", { name: organizer })}
        </p>
      )}
      {occurrence && (
        <label data-part="form.check" data-field="only-this">
          <input
            type="checkbox"
            checked={onlyThis}
            onChange={(e) => setOnlyThis(e.target.checked)}
          />
          <span>{t("scheduling-answer-only-this")}</span>
        </label>
      )}
      {proposing ? (
        <ProposeTimeForm
          start={event.fields.start}
          end={event.fields.end}
          tz={localZone()}
          busy={busy}
          problem={problem}
          onCancel={() => setProposing(false)}
          onSubmit={(start, end, comment) =>
            void run(
              { type: "ProposeNewTime", target, start, end, comment },
              { id: "invitation-applied-propose" },
            ).then((ok) => ok && setProposing(false))
          }
        />
      ) : (
        <>
          <AnswerForm
            current={me?.status ?? null}
            available={RESPONSES}
            organizer={organizer}
            confirm={settings.data?.confirm_replies ?? false}
            busy={busy}
            onAnswer={(response: InvitationResponse, comment, send) =>
              void run(
                {
                  type: "RespondToInvitation",
                  target,
                  response,
                  comment,
                  send,
                  calendar_id: null,
                },
                { id: send ? `invitation-done-${response}-sent` : `invitation-done-${response}` },
              )
            }
          />
          <button
            type="button"
            data-part="form.button"
            data-action="propose-new-time"
            disabled={busy}
            onClick={() => setProposing(true)}
          >
            {t("invitation-action-propose-new-time")}
          </button>
          <Problem message={problem} part="scheduling.problem" />
        </>
      )}
      <p data-part="scheduling.done" role="status">
        {done ? t(done.id, done.args) : ""}
      </p>
    </section>
  );
}

/** Dialog `scheduling-cancel-meeting`. */
export function CancelMeetingDialog({ close }: DialogProps) {
  const t = useT();
  const format = useDateFormat();
  const store = useUiStoreApi();
  const target = useScheduling((s) => s.cancelTarget);
  const ids = { scope: useId(), note: useId() };
  const { busy, problem, run } = useRunner();
  const [scope, setScope] = useState<CancelScope>(
    target?.recurring && target.recurrenceId ? "occurrence" : "meeting",
  );
  const [chosen, setChosen] = useState<string[]>([]);
  const [comment, setComment] = useState("");
  const finish = () => {
    schedulingStoreFor(store).getState().setCancelTarget(null);
    close();
  };
  if (target === null) {
    return (
      <div data-part="scheduling.dialog" data-state="empty">
        <p data-part="scheduling.hint">{t("scheduling-cancel-nothing")}</p>
        <button type="button" data-part="form.button" onClick={finish}>
          {t("scheduling-close")}
        </button>
      </div>
    );
  }
  const scopes = CANCEL_SCOPES.filter(
    (s) => s !== "occurrence" || (target.recurring && target.recurrenceId !== null),
  );
  return (
    <form
      data-part="scheduling.dialog"
      data-dialog="cancel-meeting"
      aria-busy={busy}
      onSubmit={(e) => {
        e.preventDefault();
        void run(
          {
            type: "CancelMeeting",
            event_id: target.eventId,
            scope,
            recurrence_id: scope === "occurrence" ? target.recurrenceId : null,
            attendees: scope === "attendees" ? chosen : [],
            comment: comment.trim() || null,
          },
          { id: "scheduling-cancelled" },
        ).then((ok) => ok && finish());
      }}
    >
      <p data-part="scheduling.dialog.intro">
        {t("scheduling-cancel-intro", { title: target.title || t("invitation-untitled") })}
      </p>
      <fieldset data-part="scheduling.cancel.scopes">
        <legend data-part="form.section.title">{t("scheduling-cancel-legend")}</legend>
        {scopes.map((value) => (
          <label key={value} data-part="form.check" data-scope={value}>
            <input
              type="radio"
              name={ids.scope}
              checked={scope === value}
              onChange={() => setScope(value)}
            />
            <span>
              {t(`scheduling-cancel-scope-${value}`, {
                occurrence: target.recurrenceId
                  ? timeRangeText(t, format, target.recurrenceId, target.recurrenceId, localZone())
                  : "",
              })}
            </span>
          </label>
        ))}
      </fieldset>
      {scope === "attendees" && (
        <ul data-part="scheduling.cancel.people" aria-label={t("scheduling-cancel-people")}>
          {target.attendees.map((a) => (
            <li key={a.email} data-part="scheduling.cancel.person">
              <label data-part="form.check">
                <input
                  type="checkbox"
                  checked={chosen.includes(a.email)}
                  onChange={(e) =>
                    setChosen((c) =>
                      e.target.checked ? [...c, a.email] : c.filter((x) => x !== a.email),
                    )
                  }
                />
                <span>
                  {a.name ? t("invitation-person", { name: a.name, email: a.email }) : a.email}
                </span>
              </label>
            </li>
          ))}
        </ul>
      )}
      <div data-part="form.field" data-field="note">
        <label data-part="form.label" htmlFor={ids.note}>
          {t("scheduling-cancel-note")}
        </label>
        <textarea
          id={ids.note}
          data-part="form.textarea"
          value={comment}
          onChange={(e) => setComment(e.target.value)}
        />
      </div>
      <Problem message={problem} part="scheduling.problem" />
      <div data-part="scheduling.dialog.actions">
        <button
          type="submit"
          data-part="form.button"
          data-variant="danger"
          disabled={busy || (scope === "attendees" && chosen.length === 0)}
        >
          {t("scheduling-cancel-confirm")}
        </button>
        <button type="button" data-part="form.button" onClick={finish}>
          {t("scheduling-keep-meeting")}
        </button>
      </div>
    </form>
  );
}

const SWITCH_FIELDS: Record<(typeof SETTING_SWITCHES)[number], keyof SchedulingSettings> = {
  "add-invitations": "add_invitations",
  "apply-updates": "apply_updates",
  "apply-replies": "apply_replies",
  "apply-cancellations": "apply_cancellations",
  "confirm-replies": "confirm_replies",
};

/** Dialog `scheduling-settings`: automatic processing of invitations. */
export function SchedulingSettingsDialog({ close }: DialogProps) {
  const t = useT();
  const client = useClient();
  const queryClient = useQueryClient();
  const settings = useSchedulingSettings();
  const calendars = useCalendars();
  const ids = { calendar: useId() };
  const [draft, setDraft] = useState<SchedulingSettings | null>(null);
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState<LocalizedMessage | null>(null);
  const current = draft ?? settings.data ?? null;
  if (settings.isError) {
    return (
      <div data-part="scheduling.dialog" data-state="error">
        <Problem message={errorMessageOf(settings.error)} part="scheduling.problem" />
        <button type="button" data-part="form.button" onClick={close}>
          {t("scheduling-close")}
        </button>
      </div>
    );
  }
  if (current === null) {
    return (
      <div data-part="scheduling.dialog" data-state="loading" aria-busy="true">
        <p data-part="scheduling.hint">{t("scheduling-loading")}</p>
      </div>
    );
  }
  const writable = (calendars.data ?? []).filter(
    (c) => !c.read_only && c.components.includes("event"),
  );
  const save = async () => {
    setBusy(true);
    setProblem(null);
    try {
      const { settings: saved } = await client.request(
        { type: "SetSchedulingSettings", settings: current },
        "SchedulingSettings",
      );
      queryClient.setQueryData(schedulingKeys.settings, saved);
      close();
    } catch (error) {
      setProblem(errorMessageOf(error));
    } finally {
      setBusy(false);
    }
  };
  return (
    <form
      data-part="scheduling.dialog"
      data-dialog="settings"
      aria-busy={busy}
      onSubmit={(e) => {
        e.preventDefault();
        void save();
      }}
    >
      <fieldset data-part="scheduling.settings.switches">
        <legend data-part="form.section.title">{t("scheduling-settings-legend")}</legend>
        {SETTING_SWITCHES.map((key) => {
          const field = SWITCH_FIELDS[key];
          return (
            <label key={key} data-part="form.check" data-field={key}>
              <input
                type="checkbox"
                checked={current[field] === true}
                aria-describedby={`${ids.calendar}-${key}`}
                onChange={(e) => setDraft({ ...current, [field]: e.target.checked })}
              />
              <span>{t(`scheduling-setting-${key}`)}</span>
              <span id={`${ids.calendar}-${key}`} data-part="form.hint">
                {t(`scheduling-setting-${key}-hint`)}
              </span>
            </label>
          );
        })}
      </fieldset>
      <div data-part="form.field" data-field="calendar">
        <label data-part="form.label" htmlFor={ids.calendar}>
          {t("scheduling-setting-calendar")}
        </label>
        <select
          id={ids.calendar}
          data-part="form.select"
          value={current.calendar_id ?? ""}
          onChange={(e) => setDraft({ ...current, calendar_id: e.target.value || null })}
        >
          <option value="">{t("scheduling-setting-calendar-default")}</option>
          {writable.map((c) => (
            <option key={c.id} value={c.id}>
              {c.settings.name}
            </option>
          ))}
        </select>
      </div>
      <Problem message={problem} part="scheduling.problem" />
      <div data-part="scheduling.dialog.actions">
        <button type="submit" data-part="form.button" data-variant="primary" disabled={busy}>
          {t("scheduling-save")}
        </button>
        <button type="button" data-part="form.button" onClick={close}>
          {t("scheduling-cancel")}
        </button>
      </div>
    </form>
  );
}
