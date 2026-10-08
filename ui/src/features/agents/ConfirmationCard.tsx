/**
 * One confirmation as the confirmation center shows it: who asks (always
 * "AI agent …" or "Plug-in …" next to the name), what kind of write, the
 * core's summary sentence, the affected items, before / after, the
 * accounts and times; while pending, approve / deny and — only for kinds
 * that may be relaxed — "always allow". Labels of affected items are
 * mailbox content and shown as plain text.
 */
import { useQueryClient } from "@tanstack/react-query";
import { useEffect, useId, useRef, useState } from "react";
import { errorMessageOf, NailError } from "../../api/client";
import type { Confirmation, ConfirmationDecision, LocalizedMessage } from "../../api/generated";
import { DATE_TIME, useDateFormat } from "../../i18n/format";
import { useT } from "../../i18n/I18nProvider";
import { useAccounts, useClient } from "../../state/queries";
import {
  attributionMessage,
  isDestructive,
  isOutgoing,
  kindMessageId,
  sourceMessageId,
  stateMessageId,
  targetMessageId,
} from "./model";
import { agentsKeys } from "./queries";

interface Props {
  confirmation: Confirmation;
  /** Bring it into view and focus its heading (opened from a notice). */
  focused?: boolean;
  /** Called after the user decided it. */
  onDecided?: (confirmation: Confirmation) => void;
}

/** `data-risk` of a kind: what the theme highlights. */
export function riskOf(
  confirmation: Pick<Confirmation, "kind">,
): "destructive" | "outgoing" | "change" {
  if (isDestructive(confirmation.kind)) return "destructive";
  return isOutgoing(confirmation.kind) ? "outgoing" : "change";
}

function Changes({ confirmation }: { confirmation: Confirmation }) {
  const t = useT();
  if (confirmation.changes.length === 0) return null;
  return (
    <table data-part="confirmation.changes">
      <caption data-part="confirmation.changes.caption">{t("agents-changes")}</caption>
      <thead>
        <tr data-part="confirmation.changes.header">
          <th scope="col">{t("common-field")}</th>
          <th scope="col">{t("agents-change-before")}</th>
          <th scope="col">{t("agents-change-after")}</th>
        </tr>
      </thead>
      <tbody>
        {confirmation.changes.map((change, index) => (
          <tr key={index} data-part="confirmation.change">
            <th scope="row" data-part="confirmation.change.field">
              {t(change.field.id, change.field.args)}
            </th>
            <td
              data-part="confirmation.change.before"
              data-empty={change.before === null ? "" : undefined}
            >
              {change.before ?? t("agents-change-new")}
            </td>
            <td
              data-part="confirmation.change.after"
              data-empty={change.after === null ? "" : undefined}
            >
              {change.after ?? t("agents-change-removed")}
            </td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

function Targets({ confirmation }: { confirmation: Confirmation }) {
  const t = useT();
  if (confirmation.targets.length === 0) return null;
  return (
    <div data-part="confirmation.targets">
      <h4 data-part="confirmation.targets.title">{t("agents-targets")}</h4>
      <ul data-part="confirmation.targets.list">
        {confirmation.targets.map((target) => (
          <li
            key={`${target.kind}:${target.id}`}
            data-part="confirmation.target"
            data-kind={target.kind}
          >
            <span data-part="confirmation.target.kind">{t(targetMessageId(target.kind))}</span>
            <span data-part="confirmation.target.label">{target.label}</span>
          </li>
        ))}
      </ul>
      {confirmation.more_targets > 0 && (
        <p data-part="confirmation.targets.more">
          {t("agents-more-targets", { count: confirmation.more_targets })}
        </p>
      )}
    </div>
  );
}

/** A confirmation with its details and, while pending, the decision buttons. */
export function ConfirmationCard({ confirmation, focused = false, onDecided }: Props) {
  const t = useT();
  const dates = useDateFormat();
  const client = useClient();
  const queryClient = useQueryClient();
  const accounts = useAccounts();
  const headingId = useId();
  const headingRef = useRef<HTMLHeadingElement>(null);
  const [alwaysAllow, setAlwaysAllow] = useState(false);
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState<LocalizedMessage | null>(null);
  const pending = confirmation.state === "pending";
  const risk = riskOf(confirmation);
  const actor = attributionMessage(confirmation.requested_by);
  const kind = t(kindMessageId(confirmation.kind));
  const accountNames = confirmation.account_ids.map(
    (id) => accounts.data?.find((a) => a.id === id)?.name ?? id,
  );

  useEffect(() => {
    if (focused) headingRef.current?.focus();
  }, [focused]);

  const decide = async (decision: ConfirmationDecision) => {
    setBusy(true);
    setProblem(null);
    try {
      const { confirmation: decided } = await client.request(
        {
          type: "DecideConfirmation",
          confirmation_id: confirmation.id,
          decision,
          always_allow: decision === "approve" && alwaysAllow && confirmation.can_always_allow,
        },
        "ConfirmationDecided",
      );
      onDecided?.(decided);
    } catch (error) {
      setProblem(
        error instanceof NailError && error.code === "conflict"
          ? { id: "agents-decide-conflict" }
          : errorMessageOf(error),
      );
    } finally {
      setBusy(false);
      void queryClient.invalidateQueries({ queryKey: agentsKeys.confirmations });
      if (alwaysAllow) void queryClient.invalidateQueries({ queryKey: agentsKeys.all });
    }
  };

  return (
    <article
      data-part="confirmation"
      data-confirmation={confirmation.id}
      data-kind={confirmation.kind}
      data-risk={risk}
      data-state={confirmation.state}
      data-actor={confirmation.requested_by.actor.kind}
      data-focused={focused ? "" : undefined}
      aria-labelledby={headingId}
      aria-busy={busy}
    >
      <header data-part="confirmation.header">
        <span data-part="confirmation.actor" data-actor={confirmation.requested_by.actor.kind}>
          {t(actor.id, actor.args)}
        </span>
        {confirmation.requested_by.on_behalf_of && (
          <span data-part="confirmation.on-behalf-of">
            {t("agents-on-behalf-of", { client: confirmation.requested_by.on_behalf_of.name })}
          </span>
        )}
        <span data-part="confirmation.kind" data-risk={risk}>
          {kind}
        </span>
        {risk === "destructive" && (
          <span data-part="confirmation.risk" data-risk={risk}>
            {t("agents-risk-destructive")}
          </span>
        )}
        {risk === "outgoing" && (
          <span data-part="confirmation.risk" data-risk={risk}>
            {t("agents-risk-outgoing")}
          </span>
        )}
      </header>
      <h3
        data-part="confirmation.summary"
        id={headingId}
        tabIndex={focused ? -1 : undefined}
        ref={headingRef}
      >
        {t(confirmation.summary.id, confirmation.summary.args)}
      </h3>
      <p data-part="confirmation.operation">
        {t("agents-operation", { operation: confirmation.operation })}
      </p>
      <Targets confirmation={confirmation} />
      <Changes confirmation={confirmation} />
      {accountNames.length > 0 && (
        <p data-part="confirmation.accounts">
          {t("agents-accounts", { accounts: accountNames.join(", "), count: accountNames.length })}
        </p>
      )}
      <p data-part="confirmation.times">
        <span data-part="confirmation.requested">
          {t("agents-requested-at", { time: dates.dateTime(confirmation.created_at, DATE_TIME) })}
        </span>
        {pending && (
          <span data-part="confirmation.expires">
            {t("agents-expires-at", { time: dates.dateTime(confirmation.expires_at, DATE_TIME) })}
          </span>
        )}
      </p>
      {!pending && (
        <p data-part="confirmation.outcome" data-state={confirmation.state}>
          <span data-part="confirmation.state" data-state={confirmation.state}>
            {t(stateMessageId(confirmation.state))}
          </span>
          {confirmation.decided_at !== null && (
            <span data-part="confirmation.decided">
              {t("agents-decided-at", {
                time: dates.dateTime(confirmation.decided_at, DATE_TIME),
              })}
            </span>
          )}
          {confirmation.decided_via !== null && (
            <span data-part="confirmation.via">{t(sourceMessageId(confirmation.decided_via))}</span>
          )}
        </p>
      )}
      {confirmation.error && (
        <p data-part="confirmation.error" role="alert">
          {t(confirmation.error.id, confirmation.error.args)}
        </p>
      )}
      {problem && (
        <p data-part="confirmation.error" role="alert">
          {t(problem.id, problem.args)}
        </p>
      )}
      {pending && (
        <div data-part="confirmation.decision">
          {confirmation.can_always_allow ? (
            <label data-part="confirmation.always-allow">
              <input
                type="checkbox"
                data-part="confirmation.always-allow.input"
                checked={alwaysAllow}
                disabled={busy}
                onChange={(event) => {
                  setAlwaysAllow(event.target.checked);
                }}
              />
              <span data-part="confirmation.always-allow.label">
                {t("agents-always-allow", { actor: confirmation.requested_by.name, kind })}
              </span>
            </label>
          ) : (
            <p data-part="confirmation.always-asks">{t("agents-always-asks")}</p>
          )}
          <div data-part="confirmation.buttons">
            <button
              type="button"
              data-part="form.button"
              data-action="approve"
              data-variant={risk === "destructive" ? "danger" : "primary"}
              disabled={busy}
              onClick={() => void decide("approve")}
            >
              {t(risk === "destructive" ? "agents-approve-destructive" : "agents-approve")}
            </button>
            <button
              type="button"
              data-part="form.button"
              data-action="deny"
              disabled={busy}
              onClick={() => void decide("deny")}
            >
              {t("common-deny")}
            </button>
          </div>
        </div>
      )}
    </article>
  );
}
