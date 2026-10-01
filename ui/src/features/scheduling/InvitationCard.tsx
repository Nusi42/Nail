/**
 * The invitation card in the reader (contract ADR 0065, `Invitation`): what
 * the iTIP message is (invitation, update, cancellation, reply,
 * counter-proposal, …) and how it relates to the user's calendars, the
 * event (time, repetition, place, organizer, attendees with their answers),
 * problems (spoofing warnings first), conflicts with the user's calendars,
 * and the actions the core offers: Accept / Maybe / Decline with a comment
 * (optionally without sending, for the series or one occurrence), Propose
 * a new time, add / update / remove from the calendar, apply a reply,
 * accept or decline a counter-proposal, answer a refresh, ask for the
 * latest version, forward, open the event.
 *
 * Mount points: `InvitationCard` (the reader passes `MessageBody.invitation`
 * and the message id) and the `invitation-card` widget (the selected
 * message). Every write is a scheduling request; the core queues the
 * calendar change and the iMIP mail.
 */
import { useQueryClient } from "@tanstack/react-query";
import { useState, type ReactNode } from "react";
import type {
  Invitation,
  InvitationAction,
  InvitationResponse,
  InvitationTarget,
  LocalizedMessage,
  MessageId,
  PimTime,
} from "../../api/generated";
import { errorMessageOf } from "../../api/client";
import { useDateFormat } from "../../i18n/format";
import { useT } from "../../i18n/I18nProvider";
import { optionBool, optionString, type WidgetProps } from "../../layout/widgetRegistry";
import { isUnsupported, useClient } from "../../state/queries";
import { useUi, useUiStoreApi } from "../../state/uiStore";
import { Icon } from "../../widgets/Icon";
import { Problem } from "../pim/common";
import { EVENT_DIALOG } from "../pim/names";
import { pimStoreFor } from "../pim/store";
import { localZone, splitPimTime, todayIn } from "../pim/time";
import {
  AnswerForm,
  ForwardForm,
  occurrenceOn,
  ProposeTimeForm,
  ScopeChoice,
  type AnswerScope,
} from "./Answer";
import { instantRangeText, recurrenceText, timeRangeText } from "./fmt";
import { answerCounts, canRespond, OTHER_ACTIONS, RESPONSES } from "./model";
import {
  scheduleWrite,
  useInvitation,
  useSchedulingSettings,
  type SchedulingWrite,
} from "./queries";

/** Issues that warn about a forged sender (shown first, as alerts). */
const SPOOFING = new Set(["sender-not-organizer", "sender-not-attendee"]);

export interface InvitationCardProps {
  messageId: MessageId;
  /** The card from `MessageBody.invitation`, when the reader has it. */
  invitation?: Invitation | null;
  /** Show the attendee list open. */
  attendeesOpen?: boolean;
  /** Show conflicts with the user's calendars. */
  showConflicts?: boolean;
}

/** The card of one message; renders nothing for messages without an invitation. */
export function InvitationCard({
  messageId,
  invitation: initial,
  attendeesOpen = false,
  showConflicts = true,
}: InvitationCardProps) {
  const t = useT();
  const query = useInvitation(messageId, initial);
  if (query.isError) {
    if (isUnsupported(query.error)) return null;
    return (
      <section data-part="invitation" data-state="error" aria-label={t("invitation-label-error")}>
        <Problem message={errorMessageOf(query.error)} part="invitation.problem" />
      </section>
    );
  }
  const invitation = query.data;
  if (!invitation) return null;
  return (
    <InvitationView
      key={`${invitation.message_id}-${invitation.sequence}`}
      invitation={invitation}
      attendeesOpen={attendeesOpen}
      showConflicts={showConflicts}
    />
  );
}

type Mode = "idle" | "propose" | "forward";

function InvitationView({
  invitation,
  attendeesOpen,
  showConflicts,
}: {
  invitation: Invitation;
  attendeesOpen: boolean;
  showConflicts: boolean;
}) {
  const t = useT();
  const format = useDateFormat();
  const client = useClient();
  const queryClient = useQueryClient();
  const store = useUiStoreApi();
  const settings = useSchedulingSettings();
  const tz = localZone();
  const [mode, setMode] = useState<Mode>("idle");
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState<LocalizedMessage | null>(null);
  const [done, setDone] = useState<LocalizedMessage | null>(null);
  const fields = invitation.fields;
  const seriesStart = fields?.start ?? null;
  const recurring = invitation.recurrence !== null && invitation.recurrence_id === null;
  const [scope, setScope] = useState<AnswerScope>("series");
  const [day, setDay] = useState(() => (seriesStart ? splitPimTime(seriesStart).day : todayIn(tz)));
  const canPickOccurrence = recurring && invitation.event_id !== null && seriesStart !== null;

  const title = fields?.title || t("invitation-untitled");
  const has = (action: InvitationAction) => invitation.actions.includes(action);

  const run = async (request: SchedulingWrite, success: LocalizedMessage | null) => {
    setBusy(true);
    setProblem(null);
    setDone(null);
    try {
      await scheduleWrite({ client, queryClient }, request);
      setMode("idle");
      setDone(success);
    } catch (error) {
      setProblem(errorMessageOf(error));
    } finally {
      setBusy(false);
    }
  };

  /** The target of attendee-side requests: the card, or one occurrence of the event. */
  const target = (): InvitationTarget => {
    if (
      canPickOccurrence &&
      scope === "occurrence" &&
      invitation.event_id !== null &&
      seriesStart
    ) {
      return {
        type: "event",
        event_id: invitation.event_id,
        recurrence_id: occurrenceOn(seriesStart, day),
      };
    }
    return { type: "message", message_id: invitation.message_id };
  };

  const answer = (response: InvitationResponse, comment: string | null, send: boolean) =>
    void run(
      {
        type: "RespondToInvitation",
        target: target(),
        response,
        comment,
        send,
        calendar_id: null,
      },
      { id: send ? `invitation-done-${response}-sent` : `invitation-done-${response}` },
    );

  const other = (action: InvitationAction) => {
    switch (action) {
      case "add-to-calendar":
      case "update-calendar":
      case "remove-from-calendar":
      case "apply-reply":
      case "send-current":
        return run(
          { type: "ApplyInvitation", message_id: invitation.message_id, calendar_id: null },
          { id: `invitation-applied-${action}` },
        );
      case "accept-counter":
      case "decline-counter":
        return run(
          {
            type: "RespondToCounter",
            message_id: invitation.message_id,
            accept: action === "accept-counter",
            comment: null,
          },
          { id: `invitation-applied-${action}` },
        );
      case "refresh":
        return run(
          { type: "RefreshInvitation", target: target() },
          { id: "invitation-applied-refresh" },
        );
      case "forward":
        setMode("forward");
        return;
      case "open-event":
        if (invitation.event_id !== null) {
          pimStoreFor(store).getState().editEvent({
            mode: "edit",
            eventId: invitation.event_id,
            recurrenceId: invitation.recurrence_id,
            start: null,
            end: null,
          });
          store.getState().openWindow(EVENT_DIALOG);
        }
        return;
      default:
        return;
    }
  };

  const issues = [...invitation.issues].sort(
    (a, b) => Number(SPOOFING.has(b)) - Number(SPOOFING.has(a)),
  );
  const organizerText = invitation.organizer
    ? (invitation.organizer.name ?? invitation.organizer.email)
    : null;
  const counts = answerCounts(invitation.attendees.map((a) => a.status));
  const counter = invitation.counter;

  let details: ReactNode = null;
  if (fields) {
    details = (
      <dl data-part="invitation.details">
        <Detail field="when" label={t("invitation-when")}>
          {timeRangeText(t, format, fields.start, fields.end, tz)}
        </Detail>
        {invitation.recurrence && (
          <Detail field="repeats" label={t("invitation-repeats")}>
            {recurrenceText(t, format, invitation.recurrence, fields.start)}
          </Detail>
        )}
        {invitation.recurrence_id && (
          <Detail field="occurrence" label={t("invitation-occurrence")}>
            {timeRangeText(t, format, invitation.recurrence_id, invitation.recurrence_id, tz)}
          </Detail>
        )}
        {fields.location && (
          <Detail field="where" label={t("invitation-where")}>
            {fields.location}
          </Detail>
        )}
        {organizerText && (
          <Detail field="organizer" label={t("invitation-organizer")}>
            {invitation.organizer?.name
              ? t("invitation-person", {
                  name: invitation.organizer.name,
                  email: invitation.organizer.email,
                })
              : organizerText}
          </Detail>
        )}
        {invitation.my_status && invitation.role === "attendee" && (
          <Detail field="my-status" label={t("invitation-my-status")}>
            <span data-part="invitation.status" data-status={invitation.my_status}>
              {t(`pim-partstat-${invitation.my_status}`)}
            </span>
          </Detail>
        )}
        {fields.conferences[0] && (
          <Detail field="call" label={t("invitation-call")}>
            {fields.conferences[0].uri}
          </Detail>
        )}
      </dl>
    );
  }

  return (
    <section
      data-part="invitation"
      data-method={invitation.method}
      data-state={invitation.state}
      data-role={invitation.role}
      aria-label={t("invitation-label", { title })}
      aria-busy={busy}
    >
      <header data-part="invitation.header">
        <Icon name="calendar" part="invitation.icon" />
        <p data-part="invitation.kind">
          {t(`invitation-kind-${invitation.method}`, {
            state: invitation.state,
            role: invitation.role,
          })}
        </p>
        <h2 data-part="invitation.title">{title}</h2>
        <p data-part="invitation.state" data-state={invitation.state}>
          {t(`invitation-state-${invitation.state}`, { method: invitation.method })}
        </p>
      </header>

      {issues.length > 0 && (
        <ul data-part="invitation.issues" aria-label={t("invitation-issues")}>
          {issues.map((issue) => (
            <li
              key={issue}
              data-part="invitation.issue"
              data-issue={issue}
              data-severity={SPOOFING.has(issue) ? "danger" : "warning"}
            >
              <span
                data-part="invitation.issue.text"
                role={SPOOFING.has(issue) ? "alert" : undefined}
              >
                {t(`itip-issue-${issue}`)}
              </span>
            </li>
          ))}
        </ul>
      )}

      {details}

      {invitation.method === "reply" && invitation.from_attendee && (
        <p data-part="invitation.reply" data-status={invitation.from_attendee.status}>
          {t("invitation-reply-from", {
            name: invitation.from_attendee.name ?? invitation.from_attendee.email,
            status: invitation.from_attendee.status,
          })}
        </p>
      )}

      {counter && (
        <div data-part="invitation.counter">
          <p data-part="invitation.counter.intro">
            {t("invitation-counter-from", {
              name: invitation.from_attendee?.name ?? invitation.from_attendee?.email ?? "",
            })}
          </p>
          <p data-part="invitation.counter.time">
            {timeRangeText(t, format, counter.start, counter.end, tz)}
          </p>
          {counter.location && (
            <p data-part="invitation.counter.location">
              {t("invitation-counter-location", { location: counter.location })}
            </p>
          )}
          {counter.comment && (
            <blockquote data-part="invitation.counter.comment">{counter.comment}</blockquote>
          )}
        </div>
      )}

      {invitation.comment && (
        <figure data-part="invitation.comment">
          <figcaption data-part="invitation.comment.label">
            {t("invitation-comment-from", {
              name:
                invitation.from_attendee?.name ??
                invitation.organizer?.name ??
                invitation.organizer?.email ??
                "",
            })}
          </figcaption>
          <blockquote data-part="invitation.comment.text">{invitation.comment}</blockquote>
        </figure>
      )}

      {invitation.attendees.length > 0 && invitation.method !== "reply" && (
        <details data-part="invitation.attendees" open={attendeesOpen || undefined}>
          <summary data-part="invitation.attendees.summary">
            {t("invitation-attendees-summary", {
              count: invitation.attendees.length,
              accepted: counts.accepted,
              declined: counts.declined,
              tentative: counts.tentative,
              pending: counts["needs-action"],
            })}
          </summary>
          <ul data-part="invitation.attendee-list">
            {invitation.attendees.map((a) => (
              <li
                key={a.email}
                data-part="invitation.attendee"
                data-status={a.status}
                data-kind={a.kind}
                data-role={a.role}
              >
                <span data-part="invitation.attendee.name">
                  {a.name ? t("invitation-person", { name: a.name, email: a.email }) : a.email}
                </span>
                <span data-part="invitation.attendee.role">
                  {a.kind === "room" || a.kind === "resource"
                    ? t(`invitation-attendee-${a.kind}`)
                    : t(`pim-role-${a.role}`)}
                </span>
                <span data-part="invitation.attendee.status">{t(`pim-partstat-${a.status}`)}</span>
              </li>
            ))}
          </ul>
        </details>
      )}

      {showConflicts && invitation.role === "attendee" && fields && canRespond(invitation) && (
        <div data-part="invitation.conflicts">
          {invitation.conflicts.length === 0 ? (
            <p data-part="invitation.conflicts.none">{t("invitation-conflicts-none")}</p>
          ) : (
            <>
              <p data-part="invitation.conflicts.title">
                {t("invitation-conflicts-title", { count: invitation.conflicts.length })}
              </p>
              <ul data-part="invitation.conflict-list">
                {invitation.conflicts.map((c) => (
                  <li
                    key={`${c.event_id}-${c.start_utc}`}
                    data-part="invitation.conflict"
                    data-state={c.tentative ? "tentative" : undefined}
                  >
                    <span data-part="invitation.conflict.title">
                      {c.title || t("invitation-untitled")}
                    </span>
                    <span data-part="invitation.conflict.time">
                      {c.all_day
                        ? t("invitation-conflict-all-day")
                        : instantRangeText(t, format, c.start_utc, c.end_utc, tz)}
                    </span>
                    {c.tentative && (
                      <span data-part="invitation.conflict.tentative">
                        {t("invitation-conflict-tentative")}
                      </span>
                    )}
                  </li>
                ))}
              </ul>
            </>
          )}
        </div>
      )}

      {invitation.transport === "server" && canRespond(invitation) && (
        <p data-part="invitation.transport">{t("invitation-transport-server")}</p>
      )}

      {canRespond(invitation) && mode === "idle" && (
        <>
          {canPickOccurrence && (
            <ScopeChoice scope={scope} onScope={setScope} day={day} onDay={setDay} tz={tz} />
          )}
          <AnswerForm
            current={invitation.my_status}
            available={RESPONSES.filter((r) => has(r))}
            organizer={organizerText}
            confirm={settings.data?.confirm_replies ?? false}
            busy={busy}
            onAnswer={answer}
          />
        </>
      )}

      {mode === "propose" && fields && (
        <ProposeTimeForm
          start={fields.start}
          end={fields.end}
          tz={tz}
          busy={busy}
          problem={problem}
          onCancel={() => setMode("idle")}
          onSubmit={(start: PimTime, end: PimTime, comment) =>
            void run(
              { type: "ProposeNewTime", target: target(), start, end, comment },
              { id: "invitation-applied-propose" },
            )
          }
        />
      )}

      {mode === "forward" && (
        <ForwardForm
          busy={busy}
          problem={problem}
          onCancel={() => setMode("idle")}
          onSubmit={(to, comment) =>
            void run(
              {
                type: "ForwardInvitation",
                target: { type: "message", message_id: invitation.message_id },
                to: to.map((email) => ({ name: null, email })),
                comment,
              },
              { id: "invitation-applied-forward" },
            )
          }
        />
      )}

      {mode === "idle" && (
        <div data-part="invitation.actions">
          {has("propose-new-time") && fields && (
            <button
              type="button"
              data-part="form.button"
              data-action="propose-new-time"
              disabled={busy}
              onClick={() => setMode("propose")}
            >
              {t("invitation-action-propose-new-time")}
            </button>
          )}
          {OTHER_ACTIONS.filter(has).map((action) => (
            <button
              key={action}
              type="button"
              data-part="form.button"
              data-action={action}
              data-variant={
                action === "remove-from-calendar" || action === "decline-counter"
                  ? "danger"
                  : action === "accept-counter" || action === "apply-reply"
                    ? "primary"
                    : undefined
              }
              disabled={busy}
              onClick={() => void other(action)}
            >
              {t(`invitation-action-${action}`)}
            </button>
          ))}
        </div>
      )}

      {mode === "idle" && <Problem message={problem} part="invitation.problem" />}
      <p data-part="invitation.done" role="status">
        {done ? t(done.id, done.args) : ""}
      </p>
    </section>
  );
}

function Detail({ field, label, children }: { field: string; label: string; children: ReactNode }) {
  return (
    <div data-part="invitation.detail" data-field={field}>
      <dt data-part="invitation.detail.label">{label}</dt>
      <dd data-part="invitation.detail.value">{children}</dd>
    </div>
  );
}

/** Layout widget `invitation-card`: the card of the selected message. */
export function InvitationCardWidget({ node }: WidgetProps) {
  const messageId = useUi((s) => s.selectedMessageId);
  if (messageId === null) return null;
  return (
    <div data-part="invitation-card" data-node-id={node.id ?? undefined}>
      <InvitationCard
        key={messageId}
        messageId={messageId}
        attendeesOpen={optionString(node, "attendees", "collapsed") === "expanded"}
        showConflicts={optionBool(node, "conflicts", true)}
      />
    </div>
  );
}
