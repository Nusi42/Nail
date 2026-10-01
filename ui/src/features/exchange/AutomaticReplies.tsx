/**
 * Automatic replies (out of office) of Exchange accounts (ADR 0072,
 * `exchange::oof`; for Exchange they replace the Sieve vacation reply):
 *
 * - dialog `exchange-automatic-replies` / component
 *   `AutomaticRepliesEditor`: off / on / on for a schedule (start and end
 *   with the locale-following date and time picker, in the viewer's zone,
 *   stored as UTC), the reply inside the organization, who outside gets a
 *   reply (nobody / contacts only / everyone) and that reply (plain text).
 *   Saving is queued; "not on the server yet" is shown until it is.
 * - widget `automatic-replies-banner`: while replies are on (or a schedule
 *   is coming up) for an Exchange account, a banner with "Turn off" and
 *   "Edit…".
 */
import { useId, useState } from "react";
import { errorMessageOf } from "../../api/client";
import type {
  AccountId,
  AutomaticReplies,
  AutomaticRepliesSettings,
  AutomaticRepliesStatus,
  ExternalAudience,
  LocalizedMessage,
} from "../../api/generated";
import { useDateFormat } from "../../i18n/format";
import { useT } from "../../i18n/I18nProvider";
import type { DialogProps } from "../../layout/dialogRegistry";
import { optionBool, type WidgetProps } from "../../layout/widgetRegistry";
import { useNow } from "../../state/clock";
import { useUi, useUiStoreApi } from "../../state/uiStore";
import { DateTimeInput, type Clock } from "../pim/DateTimeInput";
import { formatInstant } from "../pim/fmt";
import {
  type CalendarDate,
  CalendarDateTime,
  instantIn,
  localZone,
  toCalendarDate,
  toZoned,
  utcString,
} from "../pim/time";
import { Problem, useWriteContext } from "./common";
import { automaticRepliesIssues, repliesActive, repliesUpcoming } from "./model";
import { AUTOMATIC_REPLIES_DIALOG } from "./names";
import { setAutomaticReplies, useAutomaticReplies, useExchangeAccounts } from "./queries";
import { exchangeStoreFor, useExchangeUi } from "./store";

const STATUSES: readonly AutomaticRepliesStatus[] = ["disabled", "always-enabled", "scheduled"];
const AUDIENCES: readonly ExternalAudience[] = ["none", "contacts-only", "all"];

/** A UTC instant as a day and a wall-clock time in `tz`. */
export function wallOf(utc: string, tz: string): { day: CalendarDate; time: Clock } {
  const zoned = instantIn(utc, tz);
  return { day: toCalendarDate(zoned), time: { hour: zoned.hour, minute: zoned.minute } };
}

/** A day and wall-clock time in `tz` as a UTC instant (`YYYY-MM-DDTHH:MM:SSZ`). */
export function utcOf(day: CalendarDate, time: Clock, tz: string): string {
  return utcString(
    toZoned(new CalendarDateTime(day.year, day.month, day.day, time.hour, time.minute), tz),
  );
}

/** A schedule to start from: the next full hour, for a week. */
export function defaultSchedule(now: number): { start: string; end: string } {
  const hour = 3_600_000;
  const start = Math.ceil(now / hour) * hour;
  return { start: utcString(start), end: utcString(start + 7 * 24 * hour) };
}

/** The editor for one account's automatic replies. */
export function AutomaticRepliesEditor({
  accountId,
  onSaved,
}: {
  accountId: AccountId;
  onSaved?: () => void;
}) {
  const t = useT();
  const replies = useAutomaticReplies(accountId);
  if (replies.isError) return <Problem message={errorMessageOf(replies.error)} />;
  if (!replies.data) {
    return (
      <p data-part="exchange.hint" aria-busy="true">
        {t("exchange-loading")}
      </p>
    );
  }
  return (
    <RepliesForm
      key={`${accountId}-${replies.data.fetched_at ?? ""}`}
      accountId={accountId}
      replies={replies.data}
      {...(onSaved ? { onSaved } : {})}
    />
  );
}

function RepliesForm({
  accountId,
  replies,
  onSaved,
}: {
  accountId: AccountId;
  replies: AutomaticReplies;
  onSaved?: () => void;
}) {
  const t = useT();
  const format = useDateFormat();
  const ctx = useWriteContext();
  const now = useNow();
  const tz = localZone();
  const ids = {
    status: useId(),
    internal: useId(),
    external: useId(),
    audience: useId(),
    problem: useId(),
  };
  const [draft, setDraft] = useState<AutomaticRepliesSettings>(() => {
    const s = replies.settings;
    if (s.start !== null && s.end !== null) return s;
    return { ...s, ...defaultSchedule(now) };
  });
  const [problem, setProblem] = useState<LocalizedMessage | null>(null);
  const [saved, setSaved] = useState<AutomaticReplies | null>(null);
  const [busy, setBusy] = useState(false);
  const current = saved ?? replies;
  const change = (patch: Partial<AutomaticRepliesSettings>) => {
    setDraft({ ...draft, ...patch });
    setSaved(null);
  };
  const scheduled = draft.status === "scheduled";
  const timeProblem =
    problem?.id === "exchange-oof-issue-end-before-start" ||
    problem?.id === "exchange-oof-issue-missing-schedule" ||
    problem?.id === "exchange-oof-issue-invalid-time";

  const save = async () => {
    // The schedule is only sent with `scheduled`.
    const settings: AutomaticRepliesSettings = scheduled
      ? draft
      : { ...draft, start: null, end: null };
    const issue = automaticRepliesIssues(settings)[0];
    if (issue) {
      setProblem({ id: `exchange-oof-issue-${issue}` });
      return;
    }
    setBusy(true);
    setProblem(null);
    try {
      setSaved(await setAutomaticReplies(ctx, accountId, settings));
      onSaved?.();
    } catch (error) {
      setProblem(errorMessageOf(error));
    } finally {
      setBusy(false);
    }
  };

  const timeField = (key: "start" | "end") => {
    const value = draft[key] ?? defaultSchedule(now)[key];
    const wall = wallOf(value, tz);
    return (
      <DateTimeInput
        label={t(`exchange-oof-${key}`)}
        field={`oof-${key}`}
        day={wall.day}
        time={wall.time}
        tz={tz}
        invalid={timeProblem}
        describedBy={timeProblem ? ids.problem : undefined}
        onChange={(day, time) => change({ [key]: utcOf(day, time ?? wall.time, tz) })}
      />
    );
  };

  return (
    <form
      data-part="exchange.oof"
      data-status={draft.status}
      aria-busy={busy}
      onSubmit={(e) => {
        e.preventDefault();
        void save();
      }}
    >
      <fieldset data-part="form.section" data-section="status">
        <legend data-part="form.section.title">{t("exchange-oof-status")}</legend>
        {STATUSES.map((status) => (
          <label key={status} data-part="form.check" data-field={`status-${status}`}>
            <input
              type="radio"
              name={ids.status}
              checked={draft.status === status}
              onChange={() => change({ status })}
            />
            <span data-part="form.check.label">{t(`exchange-oof-status-${status}`)}</span>
          </label>
        ))}
      </fieldset>
      {scheduled && (
        <fieldset data-part="exchange.oof.schedule">
          <legend data-part="form.section.title">{t("exchange-oof-schedule")}</legend>
          {timeField("start")}
          {timeField("end")}
          <p data-part="form.hint">{t("exchange-oof-zone", { zone: tz.replaceAll("_", " ") })}</p>
        </fieldset>
      )}
      <div data-part="form.field" data-field="internal-reply">
        <label data-part="form.label" htmlFor={ids.internal}>
          {t("exchange-oof-internal")}
        </label>
        <textarea
          id={ids.internal}
          data-part="form.textarea"
          value={draft.internal_reply}
          aria-invalid={problem?.id === "exchange-oof-issue-empty-internal-reply"}
          onChange={(e) => change({ internal_reply: e.target.value })}
        />
      </div>
      <div data-part="form.field" data-field="external-audience">
        <label data-part="form.label" htmlFor={ids.audience}>
          {t("exchange-oof-audience")}
        </label>
        <select
          id={ids.audience}
          data-part="form.select"
          value={draft.external_audience}
          onChange={(e) => change({ external_audience: e.target.value as ExternalAudience })}
        >
          {AUDIENCES.map((a) => (
            <option key={a} value={a}>
              {t(`exchange-oof-audience-${a}`)}
            </option>
          ))}
        </select>
      </div>
      {draft.external_audience !== "none" && (
        <div data-part="form.field" data-field="external-reply">
          <label data-part="form.label" htmlFor={ids.external}>
            {t("exchange-oof-external")}
          </label>
          <textarea
            id={ids.external}
            data-part="form.textarea"
            value={draft.external_reply}
            aria-invalid={problem?.id === "exchange-oof-issue-empty-external-reply"}
            onChange={(e) => change({ external_reply: e.target.value })}
          />
        </div>
      )}
      <p data-part="form.hint">{t("exchange-oof-plain-text")}</p>
      <div id={ids.problem}>
        <Problem message={problem} />
      </div>
      {current.sync === "pending" ? (
        <p data-part="exchange.oof.sync" data-sync="pending" role="status">
          {t("exchange-oof-pending")}
        </p>
      ) : (
        current.fetched_at && (
          <p data-part="exchange.oof.sync" data-sync="synced">
            {t("exchange-oof-fetched", {
              time: formatInstant(format, current.fetched_at, tz, {
                dateStyle: "medium",
                timeStyle: "short",
              }),
            })}
          </p>
        )
      )}
      {saved && (
        <p data-part="exchange.notice" role="status">
          {t("exchange-oof-saved")}
        </p>
      )}
      <div data-part="exchange.actions">
        <button type="submit" data-part="form.button" data-variant="primary" disabled={busy}>
          {t("exchange-save")}
        </button>
      </div>
    </form>
  );
}

/** Dialog `exchange-automatic-replies`: the editor for a chosen Exchange account. */
export function AutomaticRepliesDialog({ close }: DialogProps) {
  const t = useT();
  const store = useUiStoreApi();
  const chosen = useExchangeUi((s) => s.repliesAccount);
  const activeAccountId = useUi((s) => s.activeAccountId);
  const accounts = useExchangeAccounts();
  const [picked, setPicked] = useState<string | null>(null);
  const ids = { account: useId() };
  const account =
    accounts.find((a) => a.id === (picked ?? chosen)) ??
    accounts.find((a) => a.id === activeAccountId) ??
    accounts[0];
  const finish = () => {
    exchangeStoreFor(store).getState().editReplies(null);
    close();
  };
  if (!account) {
    return (
      <div data-part="exchange.dialog" data-state="empty">
        <p data-part="exchange.hint">{t("exchange-no-accounts")}</p>
        <button type="button" data-part="form.button" onClick={close}>
          {t("exchange-close")}
        </button>
      </div>
    );
  }
  return (
    <div data-part="exchange.dialog" data-dialog="automatic-replies">
      <p data-part="exchange.intro">{t("exchange-oof-intro")}</p>
      {accounts.length > 1 && (
        <div data-part="form.field" data-field="account">
          <label data-part="form.label" htmlFor={ids.account}>
            {t("exchange-account")}
          </label>
          <select
            id={ids.account}
            data-part="form.select"
            value={account.id}
            onChange={(e) => setPicked(e.target.value)}
          >
            {accounts.map((a) => (
              <option key={a.id} value={a.id}>
                {a.name}
              </option>
            ))}
          </select>
        </div>
      )}
      <AutomaticRepliesEditor key={account.id} accountId={account.id} />
      <div data-part="exchange.actions">
        <button type="button" data-part="form.button" onClick={finish}>
          {t("exchange-close")}
        </button>
      </div>
    </div>
  );
}

function Banner({ accountId, name }: { accountId: AccountId; name: string }) {
  const t = useT();
  const format = useDateFormat();
  const store = useUiStoreApi();
  const ctx = useWriteContext();
  const now = useNow();
  const replies = useAutomaticReplies(accountId);
  const [problem, setProblem] = useState<LocalizedMessage | null>(null);
  const settings = replies.data?.settings;
  if (!settings) return null;
  const active = repliesActive(settings, now);
  const upcoming = repliesUpcoming(settings, now);
  if (!active && !upcoming) return null;
  const tz = localZone();
  const when = (utc: string | null) =>
    utc === null ? "" : formatInstant(format, utc, tz, { dateStyle: "medium", timeStyle: "short" });
  const text = upcoming
    ? t("exchange-oof-banner-upcoming", { account: name, start: when(settings.start) })
    : settings.status === "scheduled"
      ? t("exchange-oof-banner-until", { account: name, end: when(settings.end) })
      : t("exchange-oof-banner-on", { account: name });

  const turnOff = async () => {
    setProblem(null);
    try {
      await setAutomaticReplies(ctx, accountId, { ...settings, status: "disabled" });
    } catch (error) {
      setProblem(errorMessageOf(error));
    }
  };

  return (
    <div
      data-part="automatic-replies-banner.item"
      data-state={upcoming ? "upcoming" : "on"}
      data-sync={replies.data?.sync}
    >
      <p data-part="automatic-replies-banner.text" role="status">
        {text}
      </p>
      <button
        type="button"
        data-part="form.button"
        data-action="turn-off"
        onClick={() => void turnOff()}
      >
        {t("exchange-oof-turn-off")}
      </button>
      <button
        type="button"
        data-part="form.button"
        data-action="edit"
        onClick={() => {
          exchangeStoreFor(store).getState().editReplies(accountId);
          store.getState().openWindow(AUTOMATIC_REPLIES_DIALOG);
        }}
      >
        {t("exchange-oof-edit")}
      </button>
      <Problem message={problem} />
    </div>
  );
}

/** Widget `automatic-replies-banner`. Renders nothing while no replies are on. */
export function AutomaticRepliesBanner({ node }: WidgetProps) {
  const accounts = useExchangeAccounts();
  const activeAccountId = useUi((s) => s.activeAccountId);
  const onlyActive = optionBool(node, "only-active-account", false);
  const shown = onlyActive ? accounts.filter((a) => a.id === activeAccountId) : accounts;
  if (shown.length === 0) return null;
  // No landmark: the banner is empty most of the time.
  return (
    <div data-part="automatic-replies-banner" data-node-id={node.id ?? undefined}>
      {shown.map((a) => (
        <Banner key={a.id} accountId={a.id} name={a.name} />
      ))}
    </div>
  );
}
