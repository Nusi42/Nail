/**
 * Widget `update-settings`: the running version and how it was installed,
 * automatic checks / downloads (of the newest release; no update channels,
 * ADR 0210), and the updater's state with "Check now", release notes,
 * progress and "Restart to install". When the updater is off (MSI managed
 * by IT, policy, portable or development copy) it says why and offers no
 * update buttons.
 *
 * Widget option: `show-installation` (default `#true`) — the version and
 * installer line at the top.
 */
import { useQueryClient } from "@tanstack/react-query";
import type { UpdateSettings as Settings } from "../../api/generated";
import { useLabel, useT } from "../../i18n/I18nProvider";
import { optionBool, type WidgetProps } from "../../layout/widgetRegistry";
import { useClient } from "../../state/queries";
import { installKindMessageId } from "./model";
import { systemKeys, useInstallation, useUpdateStatus } from "./queries";
import { QueryProblem } from "./QueryProblem";
import { UpdateStatusPanel } from "./UpdateStatus";
import { useBusyAction } from "../../state/busyAction";

/** Automatic checks / downloads; every change is saved at once. */
function UpdateOptions() {
  const t = useT();
  const client = useClient();
  const queryClient = useQueryClient();
  const status = useUpdateStatus();
  const { busy, problem, run } = useBusyAction();
  if (!status.data) return null;
  const { settings } = status.data;
  const off = status.data.state.state === "disabled";

  const save = async (next: Settings) => {
    await run(async () => {
      const answer = await client.request(
        { type: "SetUpdateSettings", settings: next },
        "UpdateStatus",
      );
      queryClient.setQueryData(systemKeys.updates, answer.status);
    });
  };

  return (
    <fieldset data-part="form.section" data-section="update-options" disabled={busy}>
      <legend data-part="form.section.title">{t("system-update-options")}</legend>
      <label data-part="form.check" data-field="auto-check">
        <input
          data-part="form.checkbox"
          type="checkbox"
          checked={settings.auto_check}
          disabled={off}
          onChange={(e) => void save({ ...settings, auto_check: e.target.checked })}
        />
        <span data-part="form.check.label">{t("system-update-auto-check")}</span>
      </label>
      <label data-part="form.check" data-field="auto-download">
        <input
          data-part="form.checkbox"
          type="checkbox"
          checked={settings.auto_download}
          disabled={off}
          onChange={(e) => void save({ ...settings, auto_download: e.target.checked })}
        />
        <span data-part="form.check.label">{t("system-update-auto-download")}</span>
      </label>
      {problem && (
        <p data-part="update-settings.error" role="alert">
          {t(problem.id, problem.args)}
        </p>
      )}
    </fieldset>
  );
}

/** "Nail 1.0.0 · Installed with Nail-setup.exe". */
function InstallationLine() {
  const t = useT();
  const installation = useInstallation();
  if (!installation.data) return null;
  return (
    <p data-part="update-settings.installation" data-kind={installation.data.kind}>
      {t("system-version-line", {
        version: installation.data.version,
        kind: t(installKindMessageId(installation.data.kind)),
      })}
    </p>
  );
}

/** The widget's content (also used by "About Nail"). */
export function UpdateSection({
  showInstallation = true,
  showOptions = true,
}: {
  showInstallation?: boolean;
  showOptions?: boolean;
}) {
  const status = useUpdateStatus();
  return (
    <>
      {showInstallation && <InstallationLine />}
      {status.isError && <QueryProblem error={status.error} part="update-settings.error" />}
      {status.data && (
        <>
          <UpdateStatusPanel status={status.data} />
          {showOptions && <UpdateOptions />}
        </>
      )}
    </>
  );
}

/** Widget `update-settings`. */
export function UpdateSettingsWidget({ node }: WidgetProps) {
  const t = useT();
  const label = useLabel();
  const title = node.label ? label(node.label) : t("system-updates-title");
  return (
    <section data-part="update-settings" data-node-id={node.id ?? undefined} aria-label={title}>
      <h3 data-part="update-settings.title">{t("system-updates-title")}</h3>
      <UpdateSection showInstallation={optionBool(node, "show-installation", true)} />
    </section>
  );
}
