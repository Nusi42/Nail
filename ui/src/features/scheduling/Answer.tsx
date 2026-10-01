/**
 * Pieces shared by the invitation card and the event editor's meeting
 * panel: the answer buttons with an optional comment and "send a response"
 * switch (and a confirmation step when the scheduling settings ask for
 * one), the "propose a new time" form with the PIM date / time picker (UI
 * locale), the "forward invitation" form, and the "series or one
 * occurrence" choice for recurring invitations.
 */
import type { CalendarDate } from "@internationalized/date";
import { useId, useState, type FormEvent } from "react";
import type {
  InvitationResponse,
  LocalizedMessage,
  ParticipationStatus,
  PimTime,
} from "../../api/generated";
import { useT } from "../../i18n/I18nProvider";
import { Problem } from "../pim/common";
import { DateTimeInput, type Clock } from "../pim/DateTimeInput";
import { makePimTime, pimTimeIn, splitPimTime, toCalendarDate } from "../pim/time";
import { RESPONSES, responseOfStatus, splitAddresses, plausibleEmail } from "./model";

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
  onAnswer(response: InvitationResponse, comment: string | null, send: boolean): void;
}

/** Accept / Maybe / Decline with a comment and the "send a response" switch. */
export function AnswerForm({
  current,
  available,
  organizer,
  confirm,
  busy,
  onAnswer,
}: AnswerFormProps) {
  const t = useT();
  const ids = { comment: useId(), send: useId() };
  const [comment, setComment] = useState("");
  const [send, setSend] = useState(true);
  const [pending, setPending] = useState<InvitationResponse | null>(null);
  const pressed = responseOfStatus(current);
  const answer = (response: InvitationResponse) => {
    if (confirm && send) setPending(response);
    else onAnswer(response, comment.trim() || null, send);
  };

  if (pending !== null) {
    return (
      <div data-part="scheduling.answer.confirm" role="group" aria-labelledby={ids.send}>
        <p id={ids.send} data-part="scheduling.answer.confirm.text">
          {t("scheduling-answer-confirm", {
            answer: t(`scheduling-answer-${pending}`),
            organizer: organizer ?? t("scheduling-organizer-unknown"),
          })}
        </p>
        <div data-part="scheduling.answer.buttons">
          <button
            type="button"
            data-part="form.button"
            data-variant="primary"
            disabled={busy}
            onClick={() => {
              setPending(null);
              onAnswer(pending, comment.trim() || null, true);
            }}
          >
            {t("scheduling-answer-confirm-send")}
          </button>
          <button
            type="button"
            data-part="form.button"
            disabled={busy}
            onClick={() => {
              setPending(null);
              onAnswer(pending, comment.trim() || null, false);
            }}
          >
            {t("scheduling-answer-confirm-no-send")}
          </button>
          <button type="button" data-part="form.button" onClick={() => setPending(null)}>
            {t("scheduling-back")}
          </button>
        </div>
      </div>
    );
  }

  return (
    <div data-part="scheduling.answer">
      <div data-part="form.field" data-field="comment">
        <label data-part="form.label" htmlFor={ids.comment}>
          {t("scheduling-answer-comment")}
        </label>
        <textarea
          id={ids.comment}
          data-part="form.textarea"
          value={comment}
          disabled={!send}
          onChange={(e) => setComment(e.target.value)}
        />
      </div>
      <label data-part="form.check" data-field="send">
        <input type="checkbox" checked={send} onChange={(e) => setSend(e.target.checked)} />
        <span>{t("scheduling-answer-send")}</span>
      </label>
      <div
        data-part="scheduling.answer.buttons"
        role="group"
        aria-label={t("scheduling-answer-group")}
      >
        {RESPONSES.filter((r) => available.includes(r)).map((response) => (
          <button
            key={response}
            type="button"
            data-part="form.button"
            data-action={response}
            data-variant={pressed === response ? "primary" : undefined}
            aria-pressed={pressed === response}
            disabled={busy}
            onClick={() => answer(response)}
          >
            {t(`scheduling-answer-${response}`)}
          </button>
        ))}
      </div>
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
  onSubmit(start: PimTime, end: PimTime, comment: string | null): void;
  onCancel(): void;
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

  const submit = (event: FormEvent) => {
    event.preventDefault();
    if (!invalid) onSubmit(startTime, endTime, comment.trim() || null);
  };

  return (
    <form
      data-part="scheduling.propose"
      aria-label={t("scheduling-propose-title")}
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
        onChange={(day, time) => setTo({ day, time })}
      />
      {invalid && (
        <p id={ids.problem} data-part="scheduling.problem" role="alert">
          {t("scheduling-propose-end-before-start")}
        </p>
      )}
      <div data-part="form.field" data-field="propose-comment">
        <label data-part="form.label" htmlFor={ids.comment}>
          {t("scheduling-propose-comment")}
        </label>
        <textarea
          id={ids.comment}
          data-part="form.textarea"
          value={comment}
          onChange={(e) => setComment(e.target.value)}
        />
      </div>
      <Problem message={problem} part="scheduling.problem" />
      <div data-part="scheduling.propose.actions">
        <button
          type="submit"
          data-part="form.button"
          data-variant="primary"
          disabled={busy || invalid}
        >
          {t("scheduling-propose-send")}
        </button>
        <button type="button" data-part="form.button" onClick={onCancel}>
          {t("scheduling-cancel")}
        </button>
      </div>
    </form>
  );
}

export interface ForwardFormProps {
  busy: boolean;
  problem: LocalizedMessage | null;
  onSubmit(to: string[], comment: string | null): void;
  onCancel(): void;
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
          onChange={(e) => setTo(e.target.value)}
        />
      </div>
      {touched && invalid && (
        <p id={ids.problem} data-part="scheduling.problem" role="alert">
          {t("scheduling-forward-invalid")}
        </p>
      )}
      <div data-part="form.field" data-field="forward-comment">
        <label data-part="form.label" htmlFor={ids.comment}>
          {t("scheduling-forward-comment")}
        </label>
        <textarea
          id={ids.comment}
          data-part="form.textarea"
          value={comment}
          onChange={(e) => setComment(e.target.value)}
        />
      </div>
      <Problem message={problem} part="scheduling.problem" />
      <div data-part="scheduling.forward.actions">
        <button type="submit" data-part="form.button" data-variant="primary" disabled={busy}>
          {t("scheduling-forward-send")}
        </button>
        <button type="button" data-part="form.button" onClick={onCancel}>
          {t("scheduling-cancel")}
        </button>
      </div>
    </form>
  );
}

/** Whether an answer goes to the whole series or one occurrence. */
export type AnswerScope = "series" | "occurrence";

export interface ScopeChoiceProps {
  scope: AnswerScope;
  onScope(scope: AnswerScope): void;
  /** The occurrence's day. */
  day: CalendarDate;
  onDay(day: CalendarDate): void;
  tz: string;
}

/** "All occurrences" or "only the occurrence on …" for a recurring invitation. */
export function ScopeChoice({ scope, onScope, day, onDay, tz }: ScopeChoiceProps) {
  const t = useT();
  const name = useId();
  return (
    <fieldset data-part="scheduling.scope">
      <legend data-part="form.section.title">{t("scheduling-scope-legend")}</legend>
      {(["series", "occurrence"] as const).map((value) => (
        <label key={value} data-part="form.check" data-scope={value}>
          <input
            type="radio"
            name={name}
            value={value}
            checked={scope === value}
            onChange={() => onScope(value)}
          />
          <span>{t(`scheduling-scope-${value}`)}</span>
        </label>
      ))}
      {scope === "occurrence" && (
        <DateTimeInput
          label={t("scheduling-scope-day")}
          field="occurrence"
          day={day}
          time={null}
          tz={tz}
          onChange={(picked) => onDay(picked)}
        />
      )}
    </fieldset>
  );
}

/** The `RECURRENCE-ID` of the occurrence on `day` of a series starting `start`. */
export function occurrenceOn(start: PimTime, day: CalendarDate): PimTime {
  const split = splitPimTime(start);
  if (start.type === "date") return makePimTime(day, null, null);
  return makePimTime(day, split.time, split.tz);
}
