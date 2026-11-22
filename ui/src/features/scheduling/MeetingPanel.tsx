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
 *
 * R6 (ADR 0239): "Cancel meeting" is a destructive `ConfirmDialog` — the
 * red "Cancel meeting", "Keep meeting" as its cancel role with the focus,
 * Enter never cancelling the meeting. The settings dialog has the shared
 * footer (Save, Cancel) and asks before unsaved changes are lost.
 */
import { useQueryClient } from "@tanstack/react-query";
import { useId, useRef, useState, type Dispatch, type SetStateAction } from "react";
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
import { useBusyAction } from "../../state/busyAction";
import { EmptyDialog, LoadingDialog } from "../../widgets/DialogStates";
import { ConfirmDialog, DialogFooter, DialogForm, useCloseGuard } from "../../widgets/dialog";
import { TextAreaField } from "../../widgets/forms";

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
        title: event.fields.title || t("common-no-title"),
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
              onChange={() => {
                setWho(value);
              }}
            />
            <span>{t(`scheduling-send-to-${value}`)}</span>
          </label>
        ))}
        {who === "chosen" && (
          <ul data-part="scheduling.meeting.people">
            <PeopleChecks
              people={others}
              part="scheduling.meeting.person"
              chosen={chosen}
              setChosen={setChosen}
            />
          </ul>
        )}
      </fieldset>
      <TextAreaField
        id={ids.comment}
        name="note"
        labelId="scheduling-cancel-note"
        value={comment}
        onChange={(v) => {
          setComment(v);
        }}
      />
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
        title: event.fields.title || t("common-no-title"),
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
            onChange={(e) => {
              setOnlyThis(e.target.checked);
            }}
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
          onCancel={() => {
            setProposing(false);
          }}
          onSubmit={(start, end, comment) =>
            void run(
              { type: "ProposeNewTime", target, start, end, comment },
              { id: "invitation-applied-propose" },
            ).then((ok) => {
              if (ok) setProposing(false);
            })
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
            onClick={() => {
              setProposing(true);
            }}
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
      <EmptyDialog
        part="scheduling.dialog"
        textPart="scheduling.hint"
        hintId="scheduling-cancel-nothing"
        close={finish}
      />
    );
  }
  const scopes = CANCEL_SCOPES.filter(
    (s) => s !== "occurrence" || (target.recurring && target.recurrenceId !== null),
  );
  const cancelMeeting = () => {
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
    ).then((ok) => {
      if (ok) finish();
    });
  };
  return (
    <ConfirmDialog
      kind="cancel-meeting"
      risk="destructive"
      question={t("scheduling-cancel-intro", { title: target.title || t("common-no-title") })}
      detail={
        <>
          <fieldset data-part="scheduling.cancel.scopes">
            <legend data-part="form.section.title">{t("scheduling-cancel-legend")}</legend>
            {scopes.map((value) => (
              <label key={value} data-part="form.check" data-scope={value}>
                <input
                  type="radio"
                  name={ids.scope}
                  checked={scope === value}
                  onChange={() => {
                    setScope(value);
                  }}
                />
                <span>
                  {t(`scheduling-cancel-scope-${value}`, {
                    occurrence: target.recurrenceId
                      ? timeRangeText(
                          t,
                          format,
                          target.recurrenceId,
                          target.recurrenceId,
                          localZone(),
                        )
                      : "",
                  })}
                </span>
              </label>
            ))}
          </fieldset>
          {scope === "attendees" && (
            <ul data-part="scheduling.cancel.people" aria-label={t("scheduling-cancel-people")}>
              <PeopleChecks
                people={target.attendees}
                part="scheduling.cancel.person"
                chosen={chosen}
                setChosen={setChosen}
              />
            </ul>
          )}
          <TextAreaField
            id={ids.note}
            name="note"
            labelId="scheduling-cancel-note"
            value={comment}
            onChange={(v) => {
              setComment(v);
            }}
          />
        </>
      }
      action={t("scheduling-cancel-confirm")}
      canConfirm={scope !== "attendees" || chosen.length > 0}
      busy={busy}
      error={problem === null ? null : t(problem.id, problem.args)}
      onConfirm={cancelMeeting}
      cancel={{ label: t("scheduling-keep-meeting"), onClick: finish }}
    />
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
  const { busy, problem, run } = useBusyAction();
  const current = draft ?? settings.data ?? null;
  /** Saved: closing no longer asks. */
  const stored = useRef(false);
  const dirty = draft !== null && JSON.stringify(draft) !== JSON.stringify(settings.data ?? null);
  /** Saves the settings: `true` when saved. */
  const persist = async (): Promise<boolean> => {
    if (current === null) return false;
    const ok = await run(async () => {
      const { settings: saved } = await client.request(
        { type: "SetSchedulingSettings", settings: current },
        "SchedulingSettings",
      );
      queryClient.setQueryData(schedulingKeys.settings, saved);
    });
    if (ok) stored.current = true;
    return ok;
  };
  useCloseGuard({
    name: () => t("window-scheduling-settings"),
    isDirty: () => !stored.current && dirty,
    save: persist,
  });
  if (settings.isError) {
    return (
      <div data-part="scheduling.dialog" data-state="error">
        <Problem message={errorMessageOf(settings.error)} part="scheduling.problem" />
        <DialogFooter done={{ onDone: close }} />
      </div>
    );
  }
  if (current === null) {
    return <LoadingDialog part="scheduling.dialog" textPart="scheduling.hint" />;
  }
  const writable = (calendars.data ?? []).filter(
    (c) => !c.read_only && c.components.includes("event"),
  );
  return (
    <DialogForm
      part="scheduling.dialog"
      dialog="settings"
      busy={busy}
      onSubmit={() => {
        void persist().then((ok) => {
          if (ok) close();
        });
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
                onChange={(e) => {
                  setDraft({ ...current, [field]: e.target.checked });
                }}
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
          onChange={(e) => {
            setDraft({ ...current, calendar_id: e.target.value || null });
          }}
        >
          <option value="">{t("common-default-calendar")}</option>
          {writable.map((c) => (
            <option key={c.id} value={c.id}>
              {c.settings.name}
            </option>
          ))}
        </select>
      </div>
      <Problem message={problem} part="scheduling.problem" />
      <DialogFooter
        primary={{ label: t("common-save"), submit: true, action: "save" }}
        cancel={{ onClick: close }}
        busy={busy}
      />
    </DialogForm>
  );
}

/** One check box per person (`name <email>`), checked when chosen. */
function PeopleChecks({
  people,
  part,
  chosen,
  setChosen,
}: {
  people: readonly { name: string | null; email: string }[];
  part: string;
  chosen: readonly string[];
  setChosen: Dispatch<SetStateAction<string[]>>;
}) {
  const t = useT();
  return (
    <>
      {people.map((a) => (
        <li key={a.email} data-part={part}>
          <label data-part="form.check">
            <input
              type="checkbox"
              checked={chosen.includes(a.email)}
              onChange={(e) => {
                setChosen((c) =>
                  e.target.checked ? [...c, a.email] : c.filter((x) => x !== a.email),
                );
              }}
            />
            <span>
              {a.name ? t("common-name-email", { name: a.name, email: a.email }) : a.email}
            </span>
          </label>
        </li>
      ))}
    </>
  );
}
