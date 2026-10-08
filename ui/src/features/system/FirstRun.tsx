/**
 * The first-run screen (dialog `first-run`, action `system.first-run`)
 * and the effect that offers it once (`FirstRunGate`): in the desktop app,
 * until the user finished or skipped it (`GetFirstRun` /
 * `CompleteFirstRun`, stored by the core). It offers
 * - Settings → Default apps (Windows never lets an app make itself the
 *   default),
 * - "Start with Windows" (the Run key; Nail then starts in the tray),
 * - importing mail from another program — the import assistant arrives
 *   with M4c; until an action `import.open` exists the screen only says so.
 *
 * Not offered in remote-UI sessions or without a desktop host (`nail
 * serve`, the e2e harness): there `GetInstallation` answers `unsupported`.
 */
import { useQueryClient } from "@tanstack/react-query";
import { useEffect } from "react";
import { useActions } from "../../actions/ActionsProvider";
import { useT } from "../../i18n/I18nProvider";
import type { DialogProps } from "../../layout/dialogRegistry";
import { useClient } from "../../state/queries";
import { useUiStoreApi } from "../../state/uiStore";
import { remoteStore } from "../remote";
import { ChooseDefaultAppsButton } from "./DefaultApps";
import { useSaveDesktopSettings } from "./DesktopSettings";
import { defaultAppStateMessageId } from "./model";
import { FIRST_RUN_DIALOG, IMPORT_WIZARD_ACTION } from "./names";
import {
  systemKeys,
  useDefaultApps,
  useDesktopSettings,
  useFirstRun,
  useInstallation,
} from "./queries";
import { systemStoreFor } from "./store";
import { useBusyAction } from "../../state/busyAction";

function DefaultAppsStep() {
  const t = useT();
  const apps = useDefaultApps();
  const mailto = apps.data?.status.associations.find((a) => a.target === "mailto");
  return (
    <li data-part="first-run.step" data-step="default-apps">
      <h4 data-part="first-run.step.title">{t("system-first-run-default-apps")}</h4>
      <p data-part="first-run.step.text">{t("system-first-run-default-apps-text")}</p>
      {mailto && (
        <p data-part="first-run.step.state" data-state={mailto.state}>
          {t("system-first-run-mailto-state", { state: t(defaultAppStateMessageId(mailto.state)) })}
        </p>
      )}
      <ChooseDefaultAppsButton />
    </li>
  );
}

function StartupStep() {
  const t = useT();
  const query = useDesktopSettings();
  const { busy, problem, save } = useSaveDesktopSettings();
  const data = query.data;
  return (
    <li data-part="first-run.step" data-step="startup">
      <h4 data-part="first-run.step.title">{t("system-first-run-startup")}</h4>
      {data && (
        <label data-part="form.check" data-field="start-with-windows">
          <input
            data-part="form.checkbox"
            type="checkbox"
            checked={data.settings.start_with_windows}
            disabled={busy || data.startWithWindowsManaged}
            onChange={(e) =>
              void save({
                ...data.settings,
                start_with_windows: e.target.checked,
                // Started with Windows = in the tray, without a window.
                start_hidden: e.target.checked ? true : data.settings.start_hidden,
              })
            }
          />
          <span data-part="form.check.label">{t("system-first-run-start-with-windows")}</span>
        </label>
      )}
      {data?.startWithWindowsManaged && (
        <p data-part="first-run.step.text" data-note="managed">
          {t("system-desktop-start-with-windows-managed")}
        </p>
      )}
      <p data-part="first-run.step.text">{t("system-first-run-startup-text")}</p>
      {problem && (
        <p data-part="first-run.error" role="alert">
          {t(problem.id, problem.args)}
        </p>
      )}
    </li>
  );
}

function ImportStep({ onImport }: { onImport: () => void }) {
  const t = useT();
  const actions = useActions();
  const available = actions.isEnabled(IMPORT_WIZARD_ACTION);
  return (
    <li data-part="first-run.step" data-step="import">
      <h4 data-part="first-run.step.title">{t("system-first-run-import")}</h4>
      <p data-part="first-run.step.text">{t("system-first-run-import-text")}</p>
      {available ? (
        <button type="button" data-part="form.button" data-action="import" onClick={onImport}>
          {t("common-import-open")}
        </button>
      ) : (
        <p data-part="form.hint" data-field="import">
          {t("system-first-run-import-later")}
        </p>
      )}
    </li>
  );
}

/** The first-run screen's content. */
export function FirstRun({ close }: { close: () => void }) {
  const t = useT();
  const client = useClient();
  const queryClient = useQueryClient();
  const actions = useActions();
  const installation = useInstallation();
  const { busy, problem, run } = useBusyAction();
  const desktop = installation.isSuccess;

  const complete = async (then?: () => void) => {
    await run(async () => {
      const answer = await client.request({ type: "CompleteFirstRun" }, "FirstRun");
      queryClient.setQueryData(systemKeys.firstRun, answer.state);
      close();
      then?.();
    });
  };

  return (
    <div data-part="first-run">
      <h3 data-part="first-run.title">{t("system-first-run-title")}</h3>
      <p data-part="first-run.intro">{t("system-first-run-intro")}</p>
      <ul data-part="first-run.steps" aria-label={t("common-setup-steps")}>
        {desktop && <DefaultAppsStep />}
        {desktop && <StartupStep />}
        <ImportStep
          onImport={() => void complete(() => void actions.dispatch(IMPORT_WIZARD_ACTION))}
        />
      </ul>
      <p data-part="first-run.later">{t("system-first-run-later")}</p>
      {problem && (
        <p data-part="first-run.error" role="alert">
          {t(problem.id, problem.args)}
        </p>
      )}
      <div data-part="first-run.buttons">
        <button
          type="button"
          data-part="form.button"
          data-variant="primary"
          data-action="done"
          disabled={busy}
          onClick={() => void complete()}
        >
          {t("system-first-run-done")}
        </button>
        <button
          type="button"
          data-part="form.button"
          data-action="skip"
          disabled={busy}
          onClick={() => void complete()}
        >
          {t("system-first-run-skip")}
        </button>
      </div>
    </div>
  );
}

/** Dialog `first-run` (title `window-first-run`). */
export function FirstRunDialog({ close }: DialogProps) {
  return <FirstRun close={close} />;
}

/** Opens the first-run screen once, in the desktop app, until it was completed. */
export function FirstRunGate() {
  const ui = useUiStoreApi();
  const remote = remoteStore.getState().session !== null;
  const installation = useInstallation(!remote);
  const firstRun = useFirstRun(!remote && installation.isSuccess);
  const due = installation.isSuccess && firstRun.data !== undefined && !firstRun.data.completed;

  useEffect(() => {
    if (!due) return;
    const system = systemStoreFor(ui);
    if (system.getState().firstRunOffered) return;
    system.getState().setFirstRunOffered();
    ui.getState().openWindow(FIRST_RUN_DIALOG);
  }, [due, ui]);

  return null;
}
