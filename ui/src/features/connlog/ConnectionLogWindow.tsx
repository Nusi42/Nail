/**
 * The connection log viewer (window `connection-log:<scope>`, ADR 0190):
 * the redacted protocol lines of one account (or account setup) as the
 * core recorded them, newest last and followed live; filters by protocol
 * and level, Pause, Copy / Export (the text Rust formats:
 * `ExportConnectionLog`), Clear. No message content or credential ever
 * reaches the UI — the core redacted every line before storing it.
 */
import { useEffect, useMemo, useRef, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { errorMessageIdOf } from "../../api/client";
import type { ConnectionLogScope, WireLevel, WireProtocol } from "../../api/generated";
import { useLocale, useT } from "../../i18n/I18nProvider";
import { TIME_MILLIS, formatDateTime } from "../../i18n/format";
import type { DialogProps } from "../../layout/dialogRegistry";
import { useAccounts, useClient } from "../../state/queries";
import { useUiStoreApi } from "../../state/uiStore";
import { SelectField } from "../../widgets/forms";
import { downloadText, saveThroughPicker } from "../../widgets/attachmentSave";
import {
  ALL_ENTRIES,
  WIRE_LEVELS,
  directionId,
  entryAt,
  exportName,
  filterEntries,
  levelId,
  protocolId,
  protocolsIn,
  type LogFilter,
} from "./model";
import { VIEWER_PART } from "./names";
import { refreshConnectionLog, useConnectionLog } from "./queries";
import { useConnlogState, viewerTarget } from "./store";

/** `PathFilter` of the Save dialog. */
const TEXT_FILTERS = [{ label_id: "connlog-filter-text", extensions: ["txt"] }];

/** Value of the "every protocol" choice. */
const ANY_PROTOCOL = "";

/** The scope's heading: the account's name, or account setup. */
function useScopeTitle(scope: ConnectionLogScope): string {
  const t = useT();
  const accounts = useAccounts();
  switch (scope.kind) {
    case "account": {
      const name = accounts.data?.find((a) => a.id === scope.account_id)?.name ?? scope.account_id;
      return t("connlog-viewer-title-account", { account: name });
    }
    case "pim-source":
      return t("connlog-viewer-title-source", { source: scope.source_id });
    case "setup":
      return t("connlog-viewer-title-setup");
  }
}

type Notice = { id: string; tone: "done" | "error" } | null;

/** The viewer of one scope. */
export function ConnectionLogViewer({
  scope,
  highlight,
}: {
  scope: ConnectionLogScope;
  highlight: number | null;
}) {
  const t = useT();
  const locale = useLocale();
  const client = useClient();
  const queryClient = useQueryClient();
  const title = useScopeTitle(scope);
  const log = useConnectionLog(scope);
  const [filter, setFilter] = useState<LogFilter>(ALL_ENTRIES);
  const [pausedAt, setPausedAt] = useState<number | null>(null);
  const [notice, setNotice] = useState<Notice>(null);
  const [busy, setBusy] = useState(false);
  const list = useRef<HTMLOListElement>(null);
  const entries = useMemo(() => log.data?.entries ?? [], [log.data]);
  const shown = useMemo(
    () => filterEntries(entries, filter, pausedAt),
    [entries, filter, pausedAt],
  );
  const pointed = entryAt(entries, highlight);
  const newest = shown.at(-1)?.seq;

  // Follow new entries unless paused; point at the error's entry once.
  const pointedOnce = useRef(false);
  useEffect(() => {
    const element = list.current;
    if (!element) return;
    if (pointed !== null && !pointedOnce.current) {
      const target = element.querySelector<HTMLElement>(`[data-seq="${String(pointed)}"]`);
      if (target) {
        pointedOnce.current = true;
        target.scrollIntoView({ block: "center" });
        return;
      }
    }
    if (pausedAt === null) element.scrollTop = element.scrollHeight;
  }, [newest, pausedAt, pointed]);

  const run = async (work: () => Promise<Notice>) => {
    setBusy(true);
    setNotice(null);
    try {
      setNotice(await work());
    } catch (error) {
      setNotice({ id: errorMessageIdOf(error), tone: "error" });
    } finally {
      setBusy(false);
    }
  };

  const text = async () =>
    (await client.request({ type: "ExportConnectionLog", scope }, "ConnectionLogExport")).text ??
    "";

  const copy = () =>
    run(async () => {
      await navigator.clipboard.writeText(await text());
      return { id: "connlog-viewer-copied", tone: "done" };
    });

  const exportLog = () =>
    run(async () => {
      const name = exportName(scope);
      const outcome = await saveThroughPicker(
        client,
        { titleId: "connlog-export-title", name, filters: TEXT_FILTERS },
        (path) => client.call({ type: "ExportConnectionLog", scope, path }),
        async () => downloadText(name, await text()),
      );
      switch (outcome) {
        case "saved":
          return { id: "connlog-viewer-saved", tone: "done" };
        case "downloaded":
          return { id: "connlog-viewer-downloaded", tone: "done" };
        case "cancelled":
          return null;
        case "unsupported":
          return { id: "connlog-viewer-export-unsupported", tone: "error" };
      }
    });

  const clear = () =>
    run(async () => {
      await client.request({ type: "ClearConnectionLog", scope }, "Done");
      await refreshConnectionLog(client, queryClient, scope);
      return { id: "connlog-viewer-cleared", tone: "done" };
    });

  const protocols = protocolsIn(entries);
  return (
    <div
      data-part={VIEWER_PART}
      data-scope={scope.kind}
      data-state={pausedAt === null ? "live" : "paused"}
    >
      <header data-part="connection-log.header">
        <h2 data-part="connection-log.title">{title}</h2>
        <p data-part="connection-log.note">
          {t("connlog-viewer-note")}{" "}
          {log.data?.keepOnDisk ? t("connlog-viewer-on-disk") : t("connlog-viewer-in-memory")}
        </p>
      </header>
      <div data-part="connection-log.controls">
        <div
          data-part="connection-log.filters"
          role="group"
          aria-label={t("connlog-viewer-filters")}
        >
          <SelectField
            labelId="connlog-viewer-protocol"
            name="protocol"
            width="content"
            value={filter.protocol ?? ANY_PROTOCOL}
            choices={[
              { value: ANY_PROTOCOL, label: "connlog-viewer-protocol-all" },
              ...protocols.map((p) => ({ value: p, label: protocolId(p) })),
            ]}
            onChange={(value) => {
              setFilter({
                ...filter,
                protocol: value === ANY_PROTOCOL ? null : (value as WireProtocol),
              });
            }}
          />
          <SelectField
            labelId="connlog-viewer-level"
            name="level"
            width="content"
            value={filter.level}
            choices={WIRE_LEVELS.map((l) => ({
              value: l,
              label: `connlog-viewer-level-from-${l}`,
            }))}
            onChange={(value) => {
              setFilter({ ...filter, level: value as WireLevel });
            }}
          />
        </div>
        <div
          data-part="connection-log.actions"
          role="toolbar"
          aria-label={t("connlog-viewer-actions")}
        >
          <button
            type="button"
            data-part="form.button"
            data-action="pause"
            aria-pressed={pausedAt !== null}
            onClick={() => {
              setPausedAt(pausedAt === null ? (entries.at(-1)?.seq ?? 0) : null);
            }}
          >
            {t("connlog-viewer-pause")}
          </button>
          <button
            type="button"
            data-part="form.button"
            data-action="copy"
            disabled={busy || entries.length === 0}
            onClick={() => void copy()}
          >
            {t("connlog-viewer-copy")}
          </button>
          <button
            type="button"
            data-part="form.button"
            data-action="export"
            disabled={busy || entries.length === 0}
            onClick={() => void exportLog()}
          >
            {t("connlog-viewer-export")}
          </button>
          <button
            type="button"
            data-part="form.button"
            data-action="clear"
            data-variant="danger"
            disabled={busy || entries.length === 0}
            onClick={() => void clear()}
          >
            {t("connlog-viewer-clear")}
          </button>
        </div>
      </div>
      <p data-part="connection-log.status" role="status" data-tone={notice?.tone}>
        {notice
          ? t(notice.id)
          : pausedAt !== null
            ? t("connlog-viewer-paused", { count: entries.length - shown.length })
            : t("connlog-viewer-count", { shown: shown.length, total: entries.length })}
      </p>
      {log.isError && (
        <p data-part="connection-log.error" role="alert">
          {t(errorMessageIdOf(log.error))}
        </p>
      )}
      {log.isSuccess && shown.length === 0 ? (
        <p data-part="connection-log.empty">
          {entries.length === 0 ? t("connlog-viewer-empty") : t("connlog-viewer-filtered")}
        </p>
      ) : (
        <ol
          ref={list}
          data-part="connection-log.list"
          aria-label={t("connlog-viewer-list")}
          // eslint-disable-next-line jsx-a11y/no-noninteractive-tabindex -- the list scrolls: keyboard users must be able to reach it (axe scrollable-region-focusable).
          tabIndex={0}
        >
          {shown.map((entry) => (
            <li
              key={entry.seq}
              data-part="connection-log.entry"
              data-seq={entry.seq}
              data-level={entry.level}
              data-direction={entry.direction}
              data-protocol={entry.protocol}
              data-highlight={entry.seq === pointed ? "" : undefined}
            >
              <span data-part="connection-log.entry.time">
                {formatDateTime(entry.at, locale, TIME_MILLIS)}
              </span>
              <span data-part="connection-log.entry.protocol">{t(protocolId(entry.protocol))}</span>
              <span data-part="connection-log.entry.connection">
                {t("connlog-viewer-connection", { id: entry.connection })}
              </span>
              <span data-part="connection-log.entry.direction">
                {t(directionId(entry.direction))}
              </span>
              {entry.level !== "info" && (
                <span data-part="connection-log.entry.level">{t(levelId(entry.level))}</span>
              )}
              <code data-part="connection-log.entry.text">{entry.text}</code>
            </li>
          ))}
        </ol>
      )}
    </div>
  );
}

/** The viewer window: its scope comes from the store or its id. */
export function ConnectionLogWindow({ name }: DialogProps) {
  const t = useT();
  const ui = useUiStoreApi();
  // Re-render when the store learns the window's scope.
  useConnlogState((s) => s.viewers[name]);
  const target = viewerTarget(ui, name);
  if (!target) {
    return (
      <div data-part={VIEWER_PART}>
        <p data-part="connection-log.empty">{t("connlog-viewer-unknown")}</p>
      </div>
    );
  }
  return <ConnectionLogViewer scope={target.scope} highlight={target.highlight} />;
}
