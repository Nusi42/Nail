/**
 * The updater's state with its buttons — shared by the `update-settings`
 * widget and "About Nail": check now, the release found (version, size,
 * release notes as plain text), download progress, "Restart to install",
 * failures, and the policy-disabled state (no buttons then).
 */
import { useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { errorMessageOf } from "../../api/client";
import type { LocalizedMessage, UpdateInfo, UpdateStatus as Status } from "../../api/generated";
import { useLocale, useT } from "../../i18n/I18nProvider";
import { DATE_TIME, useDateFormat } from "../../i18n/format";
import { useClient } from "../../state/queries";
import {
  canCheck,
  canDownload,
  canInstall,
  disabledReasonMessageId,
  failureMessageId,
  isBusy,
  parseReleaseNotes,
  progressPercent,
  updateOf,
} from "./model";
import { systemKeys } from "./queries";

/** A byte count in MB (one decimal) in the UI locale. */
export function formatMegabytes(bytes: number, locale: string): string {
  return new Intl.NumberFormat(locale, {
    style: "unit",
    unit: "megabyte",
    maximumFractionDigits: 1,
  }).format(bytes / 1_000_000);
}

/** Release notes: untrusted Markdown shown as text (never HTML). */
export function ReleaseNotes({ update }: { update: UpdateInfo }) {
  const t = useT();
  if (update.notes === null || update.notes.trim() === "") return null;
  const blocks = parseReleaseNotes(update.notes);
  return (
    <section
      data-part="release-notes"
      aria-label={t("system-release-notes-label", { version: update.version })}
    >
      {blocks.map((block, index) =>
        block.kind === "list" ? (
          <ul key={index} data-part="release-notes.list">
            {block.items.map((item, i) => (
              <li key={i} data-part="release-notes.item">
                {item}
              </li>
            ))}
          </ul>
        ) : (
          <p
            key={index}
            data-part={block.kind === "heading" ? "release-notes.heading" : "release-notes.text"}
          >
            {block.text}
          </p>
        ),
      )}
    </section>
  );
}

type Command = "CheckForUpdates" | "DownloadUpdate" | "InstallUpdate";

/** Sends an update request; the answer (or the event) refreshes the status. */
function useUpdateCommand() {
  const client = useClient();
  const queryClient = useQueryClient();
  const [busy, setBusy] = useState<Command | null>(null);
  const [problem, setProblem] = useState<LocalizedMessage | null>(null);
  const run = async (type: Command) => {
    setBusy(type);
    setProblem(null);
    try {
      if (type === "InstallUpdate") {
        // Answered before Nail quits to install (the event shows "installing").
        await client.request({ type }, "Done");
      } else {
        await client.request({ type }, "UpdateStatus");
        // Not the answer itself: the host's events may already have moved
        // past it (a fast check is ready before the answer arrives), and an
        // older status must not overwrite a newer one. Refetch instead.
        await queryClient.invalidateQueries({ queryKey: systemKeys.updates });
      }
    } catch (error) {
      setProblem(errorMessageOf(error));
    } finally {
      setBusy(null);
    }
  };
  return { busy, problem, run };
}

function StateText({ status }: { status: Status }) {
  const t = useT();
  const locale = useLocale();
  const state = status.state;
  switch (state.state) {
    case "idle":
      return (
        <p data-part="update-status.text">
          {t(status.settings.auto_check ? "system-update-idle-auto" : "system-update-idle-manual")}
        </p>
      );
    case "checking":
      return <p data-part="update-status.text">{t("system-update-checking")}</p>;
    case "up-to-date":
      return (
        <p data-part="update-status.text">
          {t("system-update-up-to-date", { version: status.current_version })}
        </p>
      );
    case "available":
      return (
        <p data-part="update-status.text">
          {state.update.size_bytes === null
            ? t("system-update-available", { version: state.update.version })
            : t("system-update-available-size", {
                version: state.update.version,
                size: formatMegabytes(state.update.size_bytes, locale),
              })}
        </p>
      );
    case "downloading": {
      const percent = progressPercent(state);
      return (
        <>
          <p data-part="update-status.text">
            {t("system-update-downloading", { version: state.update.version })}
          </p>
          <progress
            data-part="update-status.progress"
            aria-label={t("system-update-progress-label")}
            max={100}
            {...(percent === null ? {} : { value: percent })}
          />
          <p data-part="update-status.progress.text">
            {percent === null
              ? t("system-update-progress-bytes", {
                  done: formatMegabytes(state.downloaded_bytes, locale),
                })
              : t("system-update-progress-percent", { percent })}
          </p>
        </>
      );
    }
    case "ready-to-install":
      return (
        <p data-part="update-status.text">
          {t("system-update-ready", { version: state.update.version })}
        </p>
      );
    case "installing":
      return (
        <p data-part="update-status.text">
          {t("system-update-installing", { version: state.update.version })}
        </p>
      );
    case "failed":
      return (
        <p data-part="update-status.text" role="alert">
          {t(failureMessageId(state.failure))}
        </p>
      );
    case "disabled":
      return (
        <p data-part="update-status.text" data-tone="policy">
          {t(disabledReasonMessageId(state.reason))}
        </p>
      );
  }
}

/** The updater's state, release notes and buttons. */
export function UpdateStatusPanel({ status }: { status: Status }) {
  const t = useT();
  const format = useDateFormat();
  const { busy, problem, run } = useUpdateCommand();
  const state = status.state;
  const update = updateOf(state);
  const disabled = state.state === "disabled";
  return (
    <div
      data-part="update-status"
      data-state={state.state}
      aria-busy={isBusy(state) || busy !== null}
    >
      <div data-part="update-status.live" role="status" aria-live="polite">
        <StateText status={status} />
      </div>
      {problem && (
        <p data-part="update-status.error" role="alert">
          {t(problem.id, problem.args)}
        </p>
      )}
      {status.last_checked_at !== null && !disabled && (
        <p data-part="update-status.checked">
          {t("system-update-last-checked", {
            when: format.dateTime(status.last_checked_at, DATE_TIME),
          })}
        </p>
      )}
      {update !== null && state.state !== "failed" && <ReleaseNotes update={update} />}
      {!disabled && (
        <div data-part="update-status.buttons">
          {canInstall(state) && (
            <button
              type="button"
              data-part="form.button"
              data-variant="primary"
              data-action="install"
              disabled={busy !== null}
              onClick={() => void run("InstallUpdate")}
            >
              {t("system-update-restart")}
            </button>
          )}
          {canDownload(state) && (
            <button
              type="button"
              data-part="form.button"
              data-variant="primary"
              data-action="download"
              disabled={busy !== null}
              onClick={() => void run("DownloadUpdate")}
            >
              {t("system-update-download")}
            </button>
          )}
          <button
            type="button"
            data-part="form.button"
            data-action="check"
            disabled={busy !== null || !canCheck(state)}
            onClick={() => void run("CheckForUpdates")}
          >
            {t(state.state === "failed" ? "system-update-retry" : "system-update-check")}
          </button>
        </div>
      )}
    </div>
  );
}
