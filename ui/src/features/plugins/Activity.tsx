/**
 * The plug-in manager's "Activity" section (M4a, ADR 0079): what the
 * plug-in does in the background — its mail hooks (calls, failures, time
 * outs, vetoes, time spent), its background jobs (schedule, events, last
 * run and how it ended, next run, "Run now") and its CPU time against the
 * hourly quota — plus everything it adds (extensions, MCP tools).
 */
import { useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { errorMessageOf } from "../../api/client";
import type {
  LocalizedMessage,
  PluginHookStats,
  PluginInfo,
  PluginJobStatus,
} from "../../api/generated";
import { DATE_TIME, useDateFormat } from "../../i18n/format";
import { useT } from "../../i18n/I18nProvider";
import { useClient } from "../../state/queries";
import { contributionItems, hookMessageId, isRunning } from "./extensions";
import { pluginKeys, usePluginActivity } from "./queries";
import { ContributionList } from "./InstallDialog";

/** CPU use as a share of the quota, 0–100 (whole percent). */
export function cpuPercent(usedMs: number, quotaMs: number): number {
  if (quotaMs <= 0) return 0;
  return Math.min(100, Math.round((usedMs / quotaMs) * 100));
}

/** `data-level` of the CPU meter: what the theme highlights. */
export function cpuLevel(percent: number): "ok" | "high" | "exhausted" {
  if (percent >= 100) return "exhausted";
  return percent >= 75 ? "high" : "ok";
}

function Hooks({ hooks }: { hooks: readonly PluginHookStats[] }) {
  const t = useT();
  const dates = useDateFormat();
  return (
    <table data-part="plugin-activity.hooks">
      <caption data-part="plugin-activity.caption">{t("plugin-activity-hooks")}</caption>
      <thead>
        <tr>
          <th scope="col">{t("plugin-activity-hook")}</th>
          <th scope="col">{t("plugin-activity-calls")}</th>
          <th scope="col">{t("common-failed")}</th>
          <th scope="col">{t("plugin-activity-timeouts")}</th>
          <th scope="col">{t("plugin-activity-vetoes")}</th>
          <th scope="col">{t("plugin-activity-time")}</th>
          <th scope="col">{t("plugin-activity-last")}</th>
        </tr>
      </thead>
      <tbody>
        {hooks.map((h) => (
          <tr
            key={h.hook}
            data-part="plugin-activity.hook"
            data-hook={h.hook}
            data-state={h.failures + h.timeouts > 0 ? "problems" : "ok"}
          >
            <th scope="row">{t(hookMessageId(h.hook))}</th>
            <td>{t("plugin-activity-count", { count: h.calls })}</td>
            <td>{t("plugin-activity-count", { count: h.failures })}</td>
            <td>{t("plugin-activity-count", { count: h.timeouts })}</td>
            <td>{t("plugin-activity-count", { count: h.vetoes })}</td>
            <td>
              {t("plugin-activity-time-value", {
                average: h.calls === 0 ? 0 : Math.round(h.total_ms / h.calls),
                max: h.max_ms,
              })}
            </td>
            <td>
              {h.last_at === null ? t("common-never") : dates.dateTime(h.last_at, DATE_TIME)}
              {h.last_error !== null && (
                <span data-part="plugin-activity.last-error">
                  {t("plugin-activity-last-error", { error: h.last_error })}
                </span>
              )}
            </td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

function Job({ plugin, job }: { plugin: PluginInfo; job: PluginJobStatus }) {
  const t = useT();
  const dates = useDateFormat();
  const client = useClient();
  const queryClient = useQueryClient();
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState<LocalizedMessage | null>(null);
  const run = async () => {
    setBusy(true);
    setProblem(null);
    try {
      await client.request({ type: "RunPluginJob", plugin_id: plugin.id, job: job.job }, "Done");
    } catch (error) {
      setProblem(errorMessageOf(error));
    } finally {
      setBusy(false);
      void queryClient.invalidateQueries({ queryKey: pluginKeys.activity(plugin.id) });
    }
  };
  const state = job.running ? "running" : (job.last_outcome ?? "never");
  return (
    <li data-part="plugin-activity.job" data-job={job.job} data-state={state} aria-busy={busy}>
      <h4 data-part="plugin-activity.job.title">{job.title ?? job.job}</h4>
      <dl data-part="plugin-activity.job.facts">
        {job.schedule !== null && (
          <>
            <dt>{t("common-schedule")}</dt>
            <dd>
              <code data-part="plugin-activity.schedule">{job.schedule}</code>
            </dd>
          </>
        )}
        {job.events.length > 0 && (
          <>
            <dt>{t("plugin-activity-events")}</dt>
            <dd>
              {job.events.map((e) => (
                <code key={e} data-part="plugin-activity.event">
                  {e}
                </code>
              ))}
            </dd>
          </>
        )}
        <dt>{t("plugin-activity-last-run")}</dt>
        <dd>
          {job.running
            ? t("plugin-activity-running")
            : job.last_run_at === null
              ? t("common-never")
              : t("plugin-activity-last-run-value", {
                  time: dates.dateTime(job.last_run_at, DATE_TIME),
                  outcome: t(`plugin-job-outcome-${job.last_outcome ?? "ok"}`),
                })}
        </dd>
        <dt>{t("plugin-activity-next-run")}</dt>
        <dd>
          {job.next_run_at === null
            ? t("plugin-activity-no-next-run")
            : dates.dateTime(job.next_run_at, DATE_TIME)}
        </dd>
        <dt>{t("plugin-activity-runs")}</dt>
        <dd>
          {t("plugin-activity-runs-value", { runs: job.run_count, failures: job.failure_count })}
        </dd>
      </dl>
      {job.last_error !== null && (
        <p data-part="plugin-activity.last-error">
          {t("plugin-activity-last-error", { error: job.last_error })}
        </p>
      )}
      {problem && (
        <p data-part="plugin-activity.error" role="alert">
          {t(problem.id, problem.args)}
        </p>
      )}
      <button
        type="button"
        data-part="form.button"
        data-action="run-job"
        disabled={busy || job.running || !isRunning(plugin)}
        onClick={() => void run()}
      >
        {t("plugin-activity-run-now", { job: job.title ?? job.job })}
      </button>
    </li>
  );
}

function Adds({ plugin }: { plugin: PluginInfo }) {
  const t = useT();
  const items = contributionItems(plugin.manifest);
  if (items.length === 0) return null;
  return (
    <div data-part="plugin-activity.adds">
      <h4 data-part="plugin-activity.heading">{t("plugin-install-adds")}</h4>
      <ContributionList items={items} />
    </div>
  );
}

/** The "Activity" section of a plug-in in the manager. */
export function Activity({ plugin }: { plugin: PluginInfo }) {
  const t = useT();
  const activity = usePluginActivity(plugin.id);
  const a = activity.data;
  const percent = a ? cpuPercent(a.cpu_ms_last_hour, a.cpu_quota_ms_per_hour) : 0;
  return (
    <div data-part="plugin-activity" aria-busy={activity.isFetching}>
      {activity.isError && (
        <p data-part="plugin-activity.error" role="alert">
          {t(errorMessageOf(activity.error).id)}
        </p>
      )}
      {a && (
        <>
          <div data-part="plugin-activity.cpu" data-level={cpuLevel(percent)}>
            <span data-part="plugin-activity.cpu.label" id={`cpu-${plugin.id}`}>
              {t("plugin-activity-cpu")}
            </span>
            <meter
              data-part="plugin-activity.cpu.meter"
              min={0}
              max={a.cpu_quota_ms_per_hour}
              high={a.cpu_quota_ms_per_hour * 0.75}
              value={Math.min(a.cpu_ms_last_hour, a.cpu_quota_ms_per_hour)}
              aria-labelledby={`cpu-${plugin.id}`}
              aria-valuetext={t("plugin-activity-cpu-value", {
                used: (a.cpu_ms_last_hour / 1000).toFixed(1),
                quota: Math.round(a.cpu_quota_ms_per_hour / 1000),
                percent,
              })}
            />
            <span data-part="plugin-activity.cpu.value">
              {t("plugin-activity-cpu-value", {
                used: (a.cpu_ms_last_hour / 1000).toFixed(1),
                quota: Math.round(a.cpu_quota_ms_per_hour / 1000),
                percent,
              })}
            </span>
            {percent >= 100 && (
              <span data-part="plugin-activity.cpu.exhausted" role="note">
                {t("plugin-activity-cpu-exhausted")}
              </span>
            )}
          </div>
          {a.hooks.length > 0 && <Hooks hooks={a.hooks} />}
          {a.jobs.length > 0 && (
            <section data-part="plugin-activity.jobs" aria-label={t("plugin-activity-jobs")}>
              <h4 data-part="plugin-activity.heading">{t("plugin-activity-jobs")}</h4>
              <ul data-part="plugin-activity.job-list">
                {a.jobs.map((job) => (
                  <Job key={job.job} plugin={plugin} job={job} />
                ))}
              </ul>
            </section>
          )}
          {a.hooks.length === 0 && a.jobs.length === 0 && (
            <p data-part="plugin-activity.none">{t("plugin-activity-none")}</p>
          )}
        </>
      )}
      <Adds plugin={plugin} />
    </div>
  );
}
