/**
 * Pieces shared by the invitation card and the event editor's meeting
 * panel: the answer buttons with an optional comment and "send a response"
 * switch (and a confirmation step when the scheduling settings ask for
 * one), the "propose a new time" form with the PIM date / time picker (UI
 * locale), the "forward invitation" form, and the "series or one
 * occurrence" choice for recurring invitations.
 */
import type { CalendarDate } from "@internationalized/date";
import { useId, useState, type ReactNode, type SubmitEvent } from "react";
import type {
  InvitationResponse,
  LocalizedMessage,
  ParticipationStatus,
  PimTime,
} from "../../api/generated";
import { useT } from "../../i18n/I18nProvider";
import { Problem, ScopeRadios } from "../pim/common";
import { DateTimeInput, type Clock } from "../pim/DateTimeInput";
import { makePimTime, pimTimeIn, splitPimTime, toCalendarDate } from "../pim/time";
import { RESPONSES, responseOfStatus, splitAddresses } from "./model";
import { plausibleEmail } from "../../state/addresses";
import { TextAreaField } from "../../widgets/forms";
import { ConfirmDialog, DialogFooter } from "../../widgets/dialog";

export interface AnswerSegmentsProps {
  /** The user's current answer (the pressed segment). */
  current: ParticipationStatus | null;
  /** Answers on offer (shown in the order of `RESPONSES`: Accept first). */
  available: readonly InvitationResponse[];
  busy: boolean;
  onAnswer: (response: InvitationResponse) => void;
}

/**
 * Accept / Maybe / Decline as one segmented control (R7, ADR 0246): toggle
 * buttons in a group named "Your answer", the current answer pressed. An
 * unanswered invitation has none pressed; Accept is first and stands out.
 */
export function AnswerSegments({ current, available, busy, onAnswer }: AnswerSegmentsProps) {
  const t = useT();
  const pressed = responseOfStatus(current);
  return (
    <div
      data-part="segmented"
      data-field="answer"
      data-state={pressed === null ? "open" : undefined}
      role="group"
      aria-label={t("invitation-my-status")}
    >
      {RESPONSES.filter((r) => available.includes(r)).map((response) => (
        <button
          key={response}
          type="button"
          data-part="segmented.option"
          data-action={response}
          aria-pressed={pressed === response}
          // aria-disabled, not disabled: the focus stays on the answer while it is sent.
          aria-disabled={busy || undefined}
          onClick={() => {
            if (!busy) onAnswer(response);
          }}
        >
          {t(`scheduling-answer-${response}`)}
        </button>
      ))}
    </div>
  );
}

export interface AnswerFormProps {
  /** The user's current answer (pressed button). */
  current: ParticipationStatus | null;
  /** Answers on offer. */
  available: readonly InvitationResponse[];
  /** The organizer's name or address (for the confirmation). */
  organizer: string | null;
  /** Ask before sending (`SchedulingSettings.confirm_replies`). */
  confirm: boolean;
  busy: boolean;
  onAnswer: (response: InvitationResponse, comment: string | null, send: boolean) => void;
  /**
   * Only the answer control (the quick look): no note, the answer is sent.
   * The full form adds "Add a note" and the "send my answer" switch.
   */
  compact?: boolean;
  /** Shown in the answer row after the segments (the card's secondary actions). */
  children?: ReactNode;
}

/**
 * The answer control (`AnswerSegments`) with, below it, "Add a note" (a
 * comment for the organizer, opened on request) and the "send my answer"
 * switch; with the scheduling setting it asks before sending.
 */
export function AnswerForm({
  current,
  available,
  organizer,
  confirm,
  busy,
  onAnswer,
  compact = false,
  children,
}: AnswerFormProps) {
  const t = useT();
  const commentId = useId();
  const [comment, setComment] = useState("");
  const [noteOpen, setNoteOpen] = useState(false);
  const [send, setSend] = useState(true);
  const [pending, setPending] = useState<InvitationResponse | null>(null);
  const note = () => (noteOpen && send ? comment.trim() || null : null);
  const answer = (response: InvitationResponse) => {
    if (confirm && send) setPending(response);
    else onAnswer(response, note(), send);
  };

  if (pending !== null) {
    // "Send the answer?" — the shared confirmation (R6): Send, Don't send,
    // Back in the order of the viewer's system.
    return (
      <ConfirmDialog
        kind="send-answer"
        question={t("scheduling-answer-confirm", {
          answer: t(`scheduling-answer-${pending}`),
          organizer: organizer ?? t("scheduling-organizer-unknown"),
        })}
        action={t("common-send")}
        onConfirm={() => {
          setPending(null);
          onAnswer(pending, note(), true);
        }}
        secondary={{
          label: t("scheduling-answer-confirm-no-send"),
          action: "dont-send",
          onClick: () => {
            setPending(null);
            onAnswer(pending, null, false);
          },
        }}
        cancel={{
          label: t("common-back"),
          onClick: () => {
            setPending(null);
          },
        }}
        busy={busy}
      />
    );
  }

  return (
    <div data-part="scheduling.answer" data-state={compact ? "compact" : undefined}>
      <div data-part="scheduling.answer.row">
        <AnswerSegments current={current} available={available} busy={busy} onAnswer={answer} />
        {children}
      </div>
      {!compact && (
        <div data-part="scheduling.answer.options">
          {!noteOpen && (
            <button
              type="button"
              data-part="form.button"
              data-variant="ghost"
              data-action="add-note"
              disabled={!send}
              onClick={() => {
                setNoteOpen(true);
              }}
            >
              {t("pim-look-add-note")}
            </button>
          )}
          <label data-part="form.check" data-field="send">
            <input
              type="checkbox"
              checked={send}
              onChange={(e) => {
                setSend(e.target.checked);
              }}
            />
            <span>{t("scheduling-answer-send")}</span>
          </label>
        </div>
      )}
      {!compact && noteOpen && (
        <div data-part="form.field" data-field="comment">
          <label data-part="form.label" htmlFor={commentId}>
            {t("scheduling-answer-comment")}
          </label>
          <textarea
            id={commentId}
            data-part="form.textarea"
            value={comment}
            disabled={!send}
            // eslint-disable-next-line jsx-a11y/no-autofocus -- opened on request ("Add a note"): the note field takes the focus.
            autoFocus
            onChange={(e) => {
              setComment(e.target.value);
            }}
          />
        </div>
      )}
    </div>
  );
}

/** A start / end pair as the pickers edit it. */
interface Picked {
  day: CalendarDate;
  time: Clock | null;
}

function picked(time: PimTime, tz: string): Picked {
  const split = splitPimTime(time);
  if (split.time === null) return { day: split.day, time: null };
  // Show zoned times in the viewer's zone.
  if (time.type === "zoned") {
    const local = pimTimeIn(time, tz);
    return {
      day: toCalendarDate(local),
      time: { hour: local.hour, minute: local.minute },
    };
  }
  return { day: split.day, time: split.time };
}

export interface ProposeTimeFormProps {
  start: PimTime;
  end: PimTime;
  /** The viewer's zone (new times are written in it). */
  tz: string;
  busy: boolean;
  problem: LocalizedMessage | null;
  onSubmit: (start: PimTime, end: PimTime, comment: string | null) => void;
  onCancel: () => void;
}

/** "Propose a new time": start, end (PIM pickers in the UI locale) and a comment. */
export function ProposeTimeForm({
  start,
  end,
  tz,
  busy,
  problem,
  onSubmit,
  onCancel,
}: ProposeTimeFormProps) {
  const t = useT();
  const ids = { comment: useId(), problem: useId() };
  const [from, setFrom] = useState<Picked>(() => picked(start, tz));
  const [to, setTo] = useState<Picked>(() => picked(end, tz));
  const [comment, setComment] = useState("");
  const allDay = from.time === null;
  const startTime = makePimTime(from.day, from.time, allDay ? null : tz);
  const endTime = makePimTime(to.day, to.time, allDay ? null : tz);
  const invalid =
    pimTimeIn(endTime, tz).toDate().getTime() <= pimTimeIn(startTime, tz).toDate().getTime();

  const submit = (event: SubmitEvent) => {
    event.preventDefault();
    if (!invalid) onSubmit(startTime, endTime, comment.trim() || null);
  };

  return (
    <form
      data-part="scheduling.propose"
      aria-label={t("invitation-action-propose-new-time")}
      onSubmit={submit}
      aria-busy={busy}
    >
      <p data-part="scheduling.propose.intro">{t("scheduling-propose-intro")}</p>
      <DateTimeInput
        label={t("scheduling-propose-start")}
        field="propose-start"
        day={from.day}
        time={from.time}
        tz={tz}
        onChange={(day, time) => {
          // Keep the length: move the end along with the start.
          const delta = day.compare(from.day);
          setFrom({ day, time });
          if (delta !== 0) setTo((old) => ({ ...old, day: old.day.add({ days: delta }) }));
        }}
      />
      <DateTimeInput
        label={t("scheduling-propose-end")}
        field="propose-end"
        day={to.day}
        time={to.time}
        tz={tz}
        invalid={invalid}
        describedBy={invalid ? ids.problem : undefined}
        onChange={(day, time) => {
          setTo({ day, time });
        }}
      />
      {invalid && (
        <p id={ids.problem} data-part="scheduling.problem" role="alert">
          {t("common-end-after-start")}
        </p>
      )}
      <TextAreaField
        id={ids.comment}
        name="propose-comment"
        labelId="scheduling-forward-comment"
        value={comment}
        onChange={(v) => {
          setComment(v);
        }}
      />
      <Problem message={problem} part="scheduling.problem" />
      <DialogFooter
        primary={{
          label: t("scheduling-propose-send"),
          submit: true,
          action: "propose",
          disabled: invalid,
        }}
        cancel={{ onClick: onCancel }}
        busy={busy}
      />
    </form>
  );
}

export interface ForwardFormProps {
  busy: boolean;
  problem: LocalizedMessage | null;
  onSubmit: (to: string[], comment: string | null) => void;
  onCancel: () => void;
}

/** "Forward the invitation" to other people (RFC 5546 §2.1.5). */
export function ForwardForm({ busy, problem, onSubmit, onCancel }: ForwardFormProps) {
  const t = useT();
  const ids = { to: useId(), comment: useId(), problem: useId() };
  const [to, setTo] = useState("");
  const [comment, setComment] = useState("");
  const [touched, setTouched] = useState(false);
  const addresses = splitAddresses(to);
  const invalid = addresses.length === 0 || addresses.some((a) => !plausibleEmail(a));
  return (
    <form
      data-part="scheduling.forward"
      aria-label={t("scheduling-forward-title")}
      aria-busy={busy}
      onSubmit={(event) => {
        event.preventDefault();
        setTouched(true);
        if (!invalid) onSubmit(addresses, comment.trim() || null);
      }}
    >
      <div data-part="form.field" data-field="forward-to">
        <label data-part="form.label" htmlFor={ids.to}>
          {t("scheduling-forward-to")}
        </label>
        <input
          id={ids.to}
          data-part="form.input"
          type="text"
          inputMode="email"
          autoComplete="off"
          value={to}
          aria-invalid={touched && invalid}
          aria-describedby={touched && invalid ? ids.problem : undefined}
          onChange={(e) => {
            setTo(e.target.value);
          }}
        />
      </div>
      {touched && invalid && (
        <p id={ids.problem} data-part="scheduling.problem" role="alert">
          {t("scheduling-forward-invalid")}
        </p>
      )}
      <TextAreaField
        id={ids.comment}
        name="forward-comment"
        labelId="scheduling-forward-comment"
        value={comment}
        onChange={(v) => {
          setComment(v);
        }}
      />
      <Problem message={problem} part="scheduling.problem" />
      <DialogFooter
        primary={{ label: t("common-forward"), submit: true, action: "forward" }}
        cancel={{ onClick: onCancel }}
        busy={busy}
      />
    </form>
  );
}

/** Whether an answer goes to the whole series or one occurrence. */
export type AnswerScope = "series" | "occurrence";

export interface ScopeChoiceProps {
  scope: AnswerScope;
  onScope: (scope: AnswerScope) => void;
  /** The occurrence's day. */
  day: CalendarDate;
  onDay: (day: CalendarDate) => void;
  tz: string;
}

/** "All occurrences" or "only the occurrence on …" for a recurring invitation. */
export function ScopeChoice({ scope, onScope, day, onDay, tz }: ScopeChoiceProps) {
  const t = useT();
  return (
    <ScopeRadios
      part="scheduling.scope"
      legend={t("scheduling-scope-legend")}
      values={["series", "occurrence"] as const}
      value={scope}
      label={(value) => t(`scheduling-scope-${value}`)}
      onChange={onScope}
    >
      {scope === "occurrence" && (
        <DateTimeInput
          label={t("scheduling-scope-day")}
          field="occurrence"
          day={day}
          time={null}
          tz={tz}
          onChange={(picked) => {
            onDay(picked);
          }}
        />
      )}
    </ScopeRadios>
  );
}

/** The `RECURRENCE-ID` of the occurrence on `day` of a series starting `start`. */
export function occurrenceOn(start: PimTime, day: CalendarDate): PimTime {
  const split = splitPimTime(start);
  if (start.type === "date") return makePimTime(day, null, null);
  return makePimTime(day, split.time, split.tz);
}
