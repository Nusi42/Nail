/**
 * The log viewer of the plug-in manager: a plug-in's own lines, the host's
 * diagnostics and its API calls (`ListPluginLogs`, live through
 * `PluginLogLine`), filterable by level, clearable.
 */
import { PLUGIN_LOG_LEVEL_VALUES } from "../../api/generated";
import { useQueryClient } from "@tanstack/react-query";
import { useId, useState } from "react";
import { errorMessageOf } from "../../api/client";
import type { LocalizedMessage, PluginLogEntry, PluginLogLevel } from "../../api/generated";
import { SHORT_DATE_TIME_SECONDS, formatDateTime } from "../../i18n/format";
import { useLocale, useT } from "../../i18n/I18nProvider";
import { useClient } from "../../state/queries";
import { pluginKeys, usePluginLogs } from "./queries";
import { RefreshButton } from "../../widgets/RefreshButton";

export const LOG_LEVELS: readonly PluginLogLevel[] = PLUGIN_LOG_LEVEL_VALUES;

/** Whether a line is shown at the chosen minimum level. */
export function atLeast(entry: PluginLogEntry, level: PluginLogLevel): boolean {
  return LOG_LEVELS.indexOf(entry.level) >= LOG_LEVELS.indexOf(level);
}

function time(iso: string, locale: string): string {
  return formatDateTime(iso, locale, SHORT_DATE_TIME_SECONDS);
}

export function Logs({ pluginId }: { pluginId: string }) {
  const t = useT();
  const locale = useLocale();
  const client = useClient();
  const queryClient = useQueryClient();
  const logs = usePluginLogs(pluginId);
  const levelId = useId();
  const [level, setLevel] = useState<PluginLogLevel>("debug");
  const [problem, setProblem] = useState<LocalizedMessage | null>(null);
  const entries = (logs.data ?? []).filter((e) => atLeast(e, level));

  const clear = async () => {
    setProblem(null);
    try {
      await client.request({ type: "ClearPluginLogs", plugin_id: pluginId }, "Done");
      queryClient.setQueryData(pluginKeys.logs(pluginId), []);
    } catch (error) {
      setProblem(errorMessageOf(error));
    }
  };

  return (
    <section data-part="plugin-logs" aria-label={t("plugin-logs-label")}>
      <div data-part="plugin-logs.toolbar">
        <label data-part="form.label" htmlFor={levelId}>
          {t("plugin-logs-level")}
        </label>
        <select
          id={levelId}
          data-part="form.select"
          value={level}
          onChange={(event) => {
            setLevel(event.target.value as PluginLogLevel);
          }}
        >
          {LOG_LEVELS.map((l) => (
            <option key={l} value={l}>
              {t(`plugin-log-level-${l}`)}
            </option>
          ))}
        </select>
        <RefreshButton refetch={logs.refetch} />
        <button
          type="button"
          data-part="form.button"
          data-action="clear"
          disabled={(logs.data ?? []).length === 0}
          onClick={() => void clear()}
        >
          {t("plugin-logs-clear")}
        </button>
      </div>
      {problem && (
        <p data-part="plugin-logs.error" role="alert">
          {t(problem.id, problem.args)}
        </p>
      )}
      {logs.isError && (
        <p data-part="plugin-logs.error" role="alert">
          {t(errorMessageOf(logs.error).id)}
        </p>
      )}
      {logs.isSuccess && entries.length === 0 && (
        <p data-part="plugin-logs.empty">{t("plugin-logs-empty")}</p>
      )}
      {entries.length > 0 && (
        <ol data-part="plugin-logs.list">
          {entries.map((entry) => (
            <li
              key={entry.seq}
              data-part="plugin-logs.entry"
              data-level={entry.level}
              data-source={entry.source}
            >
              <time data-part="plugin-logs.time" dateTime={entry.at}>
                {time(entry.at, locale)}
              </time>
              <span data-part="plugin-logs.level">{t(`plugin-log-level-${entry.level}`)}</span>
              <span data-part="plugin-logs.source">{t(`plugin-log-source-${entry.source}`)}</span>
              <span data-part="plugin-logs.message">
                {entry.localized ? t(entry.localized.id, entry.localized.args) : entry.message}
              </span>
            </li>
          ))}
        </ol>
      )}
    </section>
  );
}
