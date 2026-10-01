/**
 * Where calendars, contacts and tasks come from:
 *
 * - the `calendar-list` widget: calendars and task lists by source, each
 *   with its colour and a switch to show or hide it in the views; rename,
 *   recolour, add a local calendar or task list.
 * - the `pim-sources` dialog: the sources (this computer, ICS / webcal
 *   subscriptions, CalDAV and CardDAV servers) with their state; sync now,
 *   remove, and add a subscription or server (tested first). A DAV source
 *   can use a mail account's login (`account=` in `accounts.kdl`).
 */
import { useEffect, useId, useState, type FormEvent } from "react";
import { errorMessageId } from "../../api/client";
import type {
  Calendar,
  CalendarComponent,
  LocalizedMessage,
  PimSource,
  PimSourceConfig,
  PimSourceTestResult,
} from "../../api/generated";
import { useDateFormat } from "../../i18n/format";
import { useT } from "../../i18n/I18nProvider";
import type { DialogProps } from "../../layout/dialogRegistry";
import { useAccounts } from "../../state/queries";
import { Problem, Swatch, geomId, problemOf, useChangeContext } from "./common";
import { formatInstant } from "./fmt";
import { IMPORT_DIALOG, SOURCES_DIALOG } from "./names";
import { usePim, usePimStore } from "./store";
import { pimWrite, useCalendars, usePimSources } from "./queries";
import { localZone } from "./time";

export const SOURCE_KINDS = ["ics", "caldav", "carddav"] as const;
export type NewSourceKind = (typeof SOURCE_KINDS)[number];

/** A source id from a name (`Work calendars` → `work-calendars`), unique among `taken`. */
export function sourceIdFor(name: string, taken: readonly string[]): string {
  const base =
    name
      .toLowerCase()
      .normalize("NFKD")
      .replace(/[^a-z0-9]+/g, "-")
      .replace(/^-+|-+$/g, "")
      .slice(0, 40) || "source";
  let id = base === "local" ? "local-1" : base;
  for (let n = 2; taken.includes(id); n++) id = `${base}-${n}`;
  return id;
}

function sourceName(t: (id: string) => string, source: PimSource | undefined, id: string): string {
  if (!source) return id;
  return source.kind === "local" ? t("pim-source-local") : source.name || source.id;
}

// ---------------------------------------------------------------------------
// Calendar list widget

export function CalendarList() {
  const t = useT();
  const ctx = useChangeContext();
  const calendars = useCalendars();
  const sources = usePimSources();
  const [problem, setProblem] = useState<LocalizedMessage | null>(null);
  const [editing, setEditing] = useState<string | null>(null);
  const ids = { name: useId(), kind: useId() };
  const [newName, setNewName] = useState("");
  const [newKind, setNewKind] = useState<CalendarComponent>("event");

  const save = async (calendar: Calendar, settings: Calendar["settings"]) => {
    setProblem(null);
    try {
      await pimWrite(
        ctx,
        { type: "UpdateCalendar", calendar_id: calendar.id, settings },
        "CalendarSaved",
      );
    } catch (error) {
      setProblem(problemOf(error));
    }
  };

  const create = async (event: FormEvent) => {
    event.preventDefault();
    if (!newName.trim()) {
      setProblem({ id: "pim-issue-missing-name" });
      return;
    }
    setProblem(null);
    try {
      await pimWrite(
        ctx,
        {
          type: "CreateCalendar",
          source_id: "local",
          settings: {
            name: newName.trim(),
            description: null,
            color: null,
            visible: true,
            reminders: true,
            default_alarms: [],
            order: (calendars.data ?? []).length,
          },
          components: [newKind],
        },
        "CalendarSaved",
      );
      setNewName("");
    } catch (error) {
      setProblem(problemOf(error));
    }
  };

  const bySource = new Map<string, Calendar[]>();
  for (const calendar of calendars.data ?? []) {
    bySource.set(calendar.source_id, [...(bySource.get(calendar.source_id) ?? []), calendar]);
  }

  return (
    <section data-part="calendar-list" aria-label={t("pim-calendars-label")}>
      {[...bySource.entries()].map(([sourceId, list]) => {
        const label = sourceName(
          t,
          sources.data?.find((s) => s.id === sourceId),
          sourceId,
        );
        return (
          <section key={sourceId} data-part="calendar-list.source" aria-label={label}>
            <h3 data-part="calendar-list.source.name">{label}</h3>
            <ul data-part="calendar-list.items">
              {list.map((calendar) => (
                <li
                  key={calendar.id}
                  data-part="calendar-list.item"
                  data-calendar={calendar.id}
                  data-state={
                    [
                      calendar.settings.visible ? null : "hidden",
                      calendar.read_only ? "read-only" : null,
                    ]
                      .filter(Boolean)
                      .join(" ") || undefined
                  }
                >
                  <label data-part="form.check">
                    <input
                      type="checkbox"
                      checked={calendar.settings.visible}
                      onChange={(e) =>
                        void save(calendar, { ...calendar.settings, visible: e.target.checked })
                      }
                    />
                    <Swatch geom={geomId("swatch", calendar.id)} color={calendar.settings.color} />
                    <span data-part="calendar-list.name">{calendar.settings.name}</span>
                    {calendar.components.includes("task") &&
                      !calendar.components.includes("event") && (
                        <span data-part="calendar-list.kind">{t("pim-calendar-kind-tasks")}</span>
                      )}
                  </label>
                  <button
                    type="button"
                    data-part="form.button"
                    aria-expanded={editing === calendar.id}
                    aria-label={t("pim-calendar-settings-of", { name: calendar.settings.name })}
                    onClick={() => setEditing((e) => (e === calendar.id ? null : calendar.id))}
                  >
                    {t("pim-calendar-settings")}
                  </button>
                  {editing === calendar.id && (
                    <CalendarSettingsForm
                      calendar={calendar}
                      onSave={(s) => void save(calendar, s)}
                    />
                  )}
                </li>
              ))}
            </ul>
          </section>
        );
      })}
      <form data-part="calendar-list.new" onSubmit={(e) => void create(e)}>
        <h3 data-part="form.section.title">{t("pim-calendar-new")}</h3>
        <div data-part="form.field" data-field="name">
          <label data-part="form.label" htmlFor={ids.name}>
            {t("pim-calendar-name")}
          </label>
          <input
            id={ids.name}
            data-part="form.input"
            value={newName}
            onChange={(e) => setNewName(e.target.value)}
          />
        </div>
        <div data-part="form.field" data-field="kind">
          <label data-part="form.label" htmlFor={ids.kind}>
            {t("pim-calendar-holds")}
          </label>
          <select
            id={ids.kind}
            data-part="form.select"
            value={newKind}
            onChange={(e) => setNewKind(e.target.value as CalendarComponent)}
          >
            <option value="event">{t("pim-calendar-holds-event")}</option>
            <option value="task">{t("pim-calendar-holds-task")}</option>
          </select>
        </div>
        <button type="submit" data-part="form.button">
          {t("pim-calendar-create")}
        </button>
      </form>
      <Problem message={problem} />
      <div data-part="calendar-list.actions">
        <button
          type="button"
          data-part="form.button"
          onClick={() => ctx.store.getState().openWindow(SOURCES_DIALOG)}
        >
          {t("pim-sources-open")}
        </button>
        <button
          type="button"
          data-part="form.button"
          onClick={() => ctx.store.getState().openWindow(IMPORT_DIALOG)}
        >
          {t("pim-import-open")}
        </button>
      </div>
    </section>
  );
}

function CalendarSettingsForm({
  calendar,
  onSave,
}: {
  calendar: Calendar;
  onSave(settings: Calendar["settings"]): void;
}) {
  const t = useT();
  const ids = { name: useId(), color: useId(), reminders: useId() };
  const [name, setName] = useState(calendar.settings.name);
  const [color, setColor] = useState(calendar.settings.color ?? "");
  const [reminders, setReminders] = useState(calendar.settings.reminders);
  return (
    <form
      data-part="calendar-list.settings"
      onSubmit={(e) => {
        e.preventDefault();
        onSave({ ...calendar.settings, name: name.trim(), color: color.trim() || null, reminders });
      }}
    >
      <fieldset data-part="pim.editor.main" disabled={calendar.read_only}>
        <div data-part="form.field" data-field="name">
          <label data-part="form.label" htmlFor={ids.name}>
            {t("pim-calendar-name")}
          </label>
          <input
            id={ids.name}
            data-part="form.input"
            value={name}
            onChange={(e) => setName(e.target.value)}
          />
        </div>
        <div data-part="form.field" data-field="color">
          <label data-part="form.label" htmlFor={ids.color}>
            {t("pim-calendar-color")}
          </label>
          <input
            id={ids.color}
            data-part="form.input"
            value={color}
            onChange={(e) => setColor(e.target.value)}
          />
        </div>
        <label data-part="form.check" data-field="reminders">
          <input
            id={ids.reminders}
            type="checkbox"
            checked={reminders}
            onChange={(e) => setReminders(e.target.checked)}
          />
          <span>{t("pim-calendar-reminders")}</span>
        </label>
      </fieldset>
      {!calendar.read_only && (
        <button type="submit" data-part="form.button" data-variant="primary">
          {t("pim-save")}
        </button>
      )}
    </form>
  );
}

// ---------------------------------------------------------------------------
// Sources dialog

export function SourcesDialog({ close }: DialogProps) {
  const t = useT();
  const format = useDateFormat();
  const ctx = useChangeContext();
  const sources = usePimSources();
  const [problem, setProblem] = useState<LocalizedMessage | null>(null);
  const tz = localZone();

  const run = async (request: Parameters<typeof pimWrite>[1]) => {
    setProblem(null);
    try {
      await pimWrite(ctx, request, "Done");
    } catch (error) {
      setProblem(problemOf(error));
    }
  };

  return (
    <div data-part="pim.sources">
      <ul data-part="pim.sources.list" aria-label={t("pim-sources-label")}>
        {(sources.data ?? []).map((source) => (
          <li
            key={source.id}
            data-part="pim.sources.item"
            data-source={source.id}
            data-state={source.state}
          >
            <span data-part="pim.sources.name">{sourceName(t, source, source.id)}</span>
            <span data-part="pim.sources.kind">{t(`pim-source-kind-${source.kind}`)}</span>
            <span data-part="pim.sources.state">{t(`pim-source-state-${source.state}`)}</span>
            {source.last_sync_at && (
              <span data-part="pim.sources.synced">
                {t("pim-source-synced", {
                  time: formatInstant(format, source.last_sync_at, tz, {
                    dateStyle: "short",
                    timeStyle: "short",
                  }),
                })}
              </span>
            )}
            {source.localized && (
              <span data-part="pim.sources.error">
                {t(source.localized.id, source.localized.args)}
              </span>
            )}
            {source.kind !== "local" && (
              <>
                <button
                  type="button"
                  data-part="form.button"
                  aria-label={t("pim-source-sync-label", {
                    name: sourceName(t, source, source.id),
                  })}
                  onClick={() => void run({ type: "SyncPimSources", source_id: source.id })}
                >
                  {t("pim-source-sync")}
                </button>
                <button
                  type="button"
                  data-part="form.button"
                  data-variant="danger"
                  aria-label={t("pim-source-remove-label", {
                    name: sourceName(t, source, source.id),
                  })}
                  onClick={() => void run({ type: "RemovePimSource", source_id: source.id })}
                >
                  {t("pim-remove")}
                </button>
              </>
            )}
          </li>
        ))}
      </ul>
      <Problem message={problem} />
      <AddSourceForm taken={(sources.data ?? []).map((s) => s.id)} />
      <div data-part="pim.editor.actions">
        <button
          type="button"
          data-part="form.button"
          onClick={() => void run({ type: "SyncPimSources", source_id: null })}
        >
          {t("pim-sources-sync-all")}
        </button>
        <button type="button" data-part="form.button" onClick={close}>
          {t("pim-close")}
        </button>
      </div>
    </div>
  );
}

function AddSourceForm({ taken }: { taken: readonly string[] }) {
  const t = useT();
  const ctx = useChangeContext();
  const accounts = useAccounts();
  const ids = {
    kind: useId(),
    name: useId(),
    url: useId(),
    account: useId(),
    user: useId(),
    password: useId(),
    color: useId(),
    refresh: useId(),
  };
  // A `webcal:` link Nail was opened with fills in the subscription (taken
  // when the dialog opens).
  const pim = usePimStore();
  const takeLaunchUrl = usePim((s) => s.setSubscribeUrl);
  const [launchUrl] = useState(() => pim.getState().subscribeUrl);
  useEffect(() => {
    if (launchUrl !== null) takeLaunchUrl(null);
  }, [launchUrl, takeLaunchUrl]);
  const [kind, setKind] = useState<NewSourceKind>("ics");
  const [name, setName] = useState("");
  const [url, setUrl] = useState(launchUrl ?? "");
  const [account, setAccount] = useState("");
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [color, setColor] = useState("");
  const [refresh, setRefresh] = useState("");
  const [tested, setTested] = useState<PimSourceTestResult | null>(null);
  const [problem, setProblem] = useState<LocalizedMessage | null>(null);
  const [added, setAdded] = useState(false);
  const [busy, setBusy] = useState(false);
  const realAccounts = (accounts.data ?? []).filter((a) => a.kind !== "virtual");

  const config = (): PimSourceConfig => {
    const refreshMinutes =
      refresh.trim() === "" ? null : Math.max(1, Math.trunc(Number(refresh)) || 1);
    const auth =
      kind !== "ics" && !account && username.trim()
        ? { method: "password" as const, username: username.trim(), secret: null, mechanism: null }
        : null;
    return {
      id: sourceIdFor(name || url || kind, taken),
      name: name.trim() || null,
      enabled: true,
      settings:
        kind === "ics"
          ? {
              kind: "ics",
              url: url.trim(),
              color: color.trim() || null,
              alarms: false,
              refresh_minutes: refreshMinutes,
              auth: null,
            }
          : {
              kind,
              account: account || null,
              url: url.trim() || null,
              auth,
              refresh_minutes: refreshMinutes,
            },
    };
  };
  const secret = () =>
    kind !== "ics" && !account && password ? { kind: "password" as const, password } : null;

  const test = async () => {
    setBusy(true);
    setProblem(null);
    setTested(null);
    try {
      const answer = await ctx.client.request(
        { type: "TestPimSource", config: config(), secret: secret() },
        "PimSourceTested",
      );
      setTested(answer.result);
    } catch (error) {
      setProblem(problemOf(error));
    } finally {
      setBusy(false);
    }
  };

  const add = async (event: FormEvent) => {
    event.preventDefault();
    setBusy(true);
    setProblem(null);
    setAdded(false);
    try {
      await pimWrite(
        ctx,
        { type: "AddPimSource", config: config(), secret: secret() },
        "PimSourceSaved",
      );
      setAdded(true);
      setName("");
      setUrl("");
      setPassword("");
      setTested(null);
    } catch (error) {
      setProblem(problemOf(error));
    } finally {
      setBusy(false);
    }
  };

  return (
    <form data-part="pim.sources.add" onSubmit={(e) => void add(e)} aria-busy={busy}>
      <h3 data-part="form.section.title">{t("pim-source-add")}</h3>
      <div data-part="form.field" data-field="kind">
        <label data-part="form.label" htmlFor={ids.kind}>
          {t("pim-source-kind")}
        </label>
        <select
          id={ids.kind}
          data-part="form.select"
          value={kind}
          onChange={(e) => setKind(e.target.value as NewSourceKind)}
        >
          {SOURCE_KINDS.map((k) => (
            <option key={k} value={k}>
              {t(`pim-source-kind-${k}`)}
            </option>
          ))}
        </select>
      </div>
      <div data-part="form.field" data-field="name">
        <label data-part="form.label" htmlFor={ids.name}>
          {t("pim-source-name")}
        </label>
        <input
          id={ids.name}
          data-part="form.input"
          value={name}
          onChange={(e) => setName(e.target.value)}
        />
      </div>
      {kind !== "ics" && (
        <div data-part="form.field" data-field="account">
          <label data-part="form.label" htmlFor={ids.account}>
            {t("pim-source-account")}
          </label>
          <select
            id={ids.account}
            data-part="form.select"
            value={account}
            onChange={(e) => setAccount(e.target.value)}
          >
            <option value="">{t("pim-source-account-none")}</option>
            {realAccounts.map((a) => (
              <option key={a.id} value={a.id}>
                {a.name || a.email}
              </option>
            ))}
          </select>
        </div>
      )}
      <div data-part="form.field" data-field="url">
        <label data-part="form.label" htmlFor={ids.url}>
          {kind === "ics" ? t("pim-source-feed-url") : t("pim-source-server-url")}
        </label>
        <input
          id={ids.url}
          data-part="form.input"
          type="url"
          value={url}
          onChange={(e) => setUrl(e.target.value)}
        />
      </div>
      {kind !== "ics" && !account && (
        <>
          <div data-part="form.field" data-field="username">
            <label data-part="form.label" htmlFor={ids.user}>
              {t("pim-source-username")}
            </label>
            <input
              id={ids.user}
              data-part="form.input"
              autoComplete="username"
              value={username}
              onChange={(e) => setUsername(e.target.value)}
            />
          </div>
          <div data-part="form.field" data-field="password">
            <label data-part="form.label" htmlFor={ids.password}>
              {t("pim-source-password")}
            </label>
            <input
              id={ids.password}
              data-part="form.input"
              type="password"
              autoComplete="current-password"
              value={password}
              onChange={(e) => setPassword(e.target.value)}
            />
          </div>
        </>
      )}
      {kind === "ics" && (
        <div data-part="form.field" data-field="color">
          <label data-part="form.label" htmlFor={ids.color}>
            {t("pim-calendar-color")}
          </label>
          <input
            id={ids.color}
            data-part="form.input"
            value={color}
            onChange={(e) => setColor(e.target.value)}
          />
        </div>
      )}
      <div data-part="form.field" data-field="refresh">
        <label data-part="form.label" htmlFor={ids.refresh}>
          {t("pim-source-refresh")}
        </label>
        <input
          id={ids.refresh}
          data-part="form.input"
          type="number"
          inputMode="numeric"
          min={1}
          value={refresh}
          onChange={(e) => setRefresh(e.target.value)}
        />
      </div>
      {tested && (
        <div data-part="pim.sources.test" data-state={tested.ok ? "ok" : "error"} role="status">
          {tested.ok ? (
            <>
              <p data-part="pim.sources.test.text">
                {t("pim-source-test-ok", { count: tested.collections.length })}
              </p>
              <ul data-part="pim.sources.test.collections">
                {tested.collections.map((c) => (
                  <li key={c.url} data-part="pim.sources.test.collection">
                    {c.name}
                  </li>
                ))}
              </ul>
            </>
          ) : (
            <p data-part="pim.sources.test.text">
              {tested.localized
                ? t(tested.localized.id, tested.localized.args)
                : t(errorMessageId(tested.error ?? "internal"))}
            </p>
          )}
        </div>
      )}
      {added && (
        <p data-part="pim.sources.added" role="status">
          {t("pim-source-added")}
        </p>
      )}
      <Problem message={problem} />
      <div data-part="pim.editor.actions">
        <button type="button" data-part="form.button" disabled={busy} onClick={() => void test()}>
          {t("pim-source-test")}
        </button>
        <button
          type="submit"
          data-part="form.button"
          data-variant="primary"
          disabled={busy || (kind === "ics" && !url.trim())}
        >
          {t("pim-source-add-submit")}
        </button>
      </div>
    </form>
  );
}
