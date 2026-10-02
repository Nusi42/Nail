/**
 * Widget `desktop-settings`: tray and startup — keep running in the tray
 * when the window closes, start with Windows (the per-user Run key; the
 * shell's value wins; locked with a note when the machine-wide Run key of
 * an MSI install turns it on for everyone), start in the tray when started
 * with Windows — and
 * the local endpoint `nail plugin dev` connects to (loopback only, with a
 * token; off by default). Every change is saved at once.
 *
 * Widget option: `show-dev-endpoint` (default `#true`).
 */
import { useQueryClient } from "@tanstack/react-query";
import { useId, useState } from "react";
import { errorMessageOf } from "../../api/client";
import type { DesktopSettings, LocalizedMessage } from "../../api/generated";
import { useLabel, useT } from "../../i18n/I18nProvider";
import { optionBool, type WidgetProps } from "../../layout/widgetRegistry";
import { useClient } from "../../state/queries";
import { systemKeys, useDesktopSettings } from "./queries";

/** Saves desktop settings; the answer replaces the cached ones. */
export function useSaveDesktopSettings() {
  const client = useClient();
  const queryClient = useQueryClient();
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState<LocalizedMessage | null>(null);
  const save = async (settings: DesktopSettings): Promise<boolean> => {
    setBusy(true);
    setProblem(null);
    try {
      const answer = await client.request(
        { type: "SetDesktopSettings", settings },
        "DesktopSettings",
      );
      queryClient.setQueryData(systemKeys.desktop, {
        settings: answer.settings,
        devEndpointUrl: answer.dev_endpoint_url,
        startWithWindowsManaged: answer.start_with_windows_managed,
      });
      return true;
    } catch (error) {
      setProblem(errorMessageOf(error));
      return false;
    } finally {
      setBusy(false);
    }
  };
  return { busy, problem, save };
}

/** One checkbox with its hint. */
function Check({
  field,
  checked,
  disabled = false,
  label,
  hint,
  onChange,
}: {
  field: string;
  checked: boolean;
  disabled?: boolean;
  label: string;
  hint: string;
  onChange(checked: boolean): void;
}) {
  const hintId = useId();
  return (
    <div data-part="form.field" data-field={field}>
      <label data-part="form.check">
        <input
          data-part="form.checkbox"
          type="checkbox"
          checked={checked}
          disabled={disabled}
          aria-describedby={hintId}
          onChange={(e) => onChange(e.target.checked)}
        />
        <span data-part="form.check.label">{label}</span>
      </label>
      <p data-part="form.hint" id={hintId}>
        {hint}
      </p>
    </div>
  );
}

/** Widget `desktop-settings`. */
export function DesktopSettingsWidget({ node }: WidgetProps) {
  const t = useT();
  const label = useLabel();
  const query = useDesktopSettings();
  const { busy, problem, save } = useSaveDesktopSettings();
  const showDev = optionBool(node, "show-dev-endpoint", true);
  const title = node.label ? label(node.label) : t("system-desktop-title");
  const data = query.data;
  const change = (patch: Partial<DesktopSettings>) => {
    if (data) void save({ ...data.settings, ...patch });
  };
  return (
    <section
      data-part="desktop-settings"
      data-node-id={node.id ?? undefined}
      aria-label={title}
      aria-busy={busy}
    >
      <h3 data-part="desktop-settings.title">{t("system-desktop-title")}</h3>
      {query.isError && (
        <p data-part="desktop-settings.error" role="alert">
          {t(errorMessageOf(query.error).id, errorMessageOf(query.error).args)}
        </p>
      )}
      {problem && (
        <p data-part="desktop-settings.error" role="alert">
          {t(problem.id, problem.args)}
        </p>
      )}
      {data && (
        <>
          <fieldset data-part="form.section" data-section="tray" disabled={busy}>
            <legend data-part="form.section.title">{t("system-desktop-tray")}</legend>
            <Check
              field="close-to-tray"
              checked={data.settings.close_to_tray}
              label={t("system-desktop-close-to-tray")}
              hint={t("system-desktop-close-to-tray-hint")}
              onChange={(close_to_tray) => change({ close_to_tray })}
            />
          </fieldset>
          <fieldset data-part="form.section" data-section="startup" disabled={busy}>
            <legend data-part="form.section.title">{t("system-desktop-startup")}</legend>
            <Check
              field="start-with-windows"
              checked={data.settings.start_with_windows}
              disabled={data.startWithWindowsManaged}
              label={t("system-desktop-start-with-windows")}
              hint={t(
                data.startWithWindowsManaged
                  ? "system-desktop-start-with-windows-managed"
                  : "system-desktop-start-with-windows-hint",
              )}
              onChange={(start_with_windows) => change({ start_with_windows })}
            />
            <Check
              field="start-hidden"
              checked={data.settings.start_hidden}
              disabled={!data.settings.start_with_windows}
              label={t("system-desktop-start-hidden")}
              hint={t("system-desktop-start-hidden-hint")}
              onChange={(start_hidden) => change({ start_hidden })}
            />
          </fieldset>
          {showDev && (
            <fieldset data-part="form.section" data-section="developer" disabled={busy}>
              <legend data-part="form.section.title">{t("system-desktop-developer")}</legend>
              <Check
                field="dev-endpoint"
                checked={data.settings.plugin_dev_endpoint}
                label={t("system-desktop-dev-endpoint")}
                hint={t("system-desktop-dev-endpoint-hint")}
                onChange={(plugin_dev_endpoint) => change({ plugin_dev_endpoint })}
              />
              {data.devEndpointUrl !== null && (
                <p data-part="desktop-settings.endpoint" role="status">
                  {t("system-desktop-dev-endpoint-running", { url: data.devEndpointUrl })}
                </p>
              )}
            </fieldset>
          )}
        </>
      )}
    </section>
  );
}
