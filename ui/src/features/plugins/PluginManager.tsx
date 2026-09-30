/**
 * The plug-in manager (layout widget `plugin-manager`, also the dialog
 * `plugin-manager`; ADR 0050): the installed plug-ins with their state,
 * turning them on and off, their permissions (grants), settings, log,
 * layout placements and removal, plus installing a package and loading a
 * development folder (both through the install dialog).
 */
import { useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { errorMessageOf } from "../../api/client";
import type { LocalizedMessage, PluginGrant, PluginInfo } from "../../api/generated";
import { useLabel, useT } from "../../i18n/I18nProvider";
import type { WidgetProps } from "../../layout/widgetRegistry";
import { useAccounts, useClient } from "../../state/queries";
import { useUi, useUiStoreApi } from "../../state/uiStore";
import { grantsValid, orderedGrants } from "./grants";
import { INSTALL_DIALOG } from "./InstallDialog";
import { Logs } from "./Logs";
import { Permissions } from "./Permissions";
import { Placements } from "./Placements";
import { pluginKeys, usePlugins } from "./queries";
import { Settings } from "./Settings";
import { pluginsStoreFor, type InstallMode } from "./store";

/** Dialog name. */
export const MANAGER_DIALOG = "plugin-manager";

type Section = "permissions" | "settings" | "log" | "layout";
const SECTIONS: readonly Section[] = ["permissions", "settings", "log", "layout"];

function sourceText(plugin: PluginInfo): { id: string; args: Record<string, string> } {
  switch (plugin.source.kind) {
    case "file":
      return { id: "plugin-manager-source-file", args: { path: plugin.source.path } };
    case "url":
      return { id: "plugin-manager-source-url", args: { url: plugin.source.url } };
    case "dev":
      return { id: "plugin-manager-source-dev", args: { folder: plugin.source.folder } };
  }
}

function GrantsEditor({ plugin }: { plugin: PluginInfo }) {
  const t = useT();
  const client = useClient();
  const queryClient = useQueryClient();
  const accounts = useAccounts();
  const [grants, setGrants] = useState<PluginGrant[]>(plugin.grants);
  const [problem, setProblem] = useState<LocalizedMessage | null>(null);
  const [saved, setSaved] = useState(false);
  const permissions = plugin.manifest.permissions ?? [];
  const next = orderedGrants(permissions, grants);
  const dirty = JSON.stringify(next) !== JSON.stringify(orderedGrants(permissions, plugin.grants));
  const save = async () => {
    setProblem(null);
    setSaved(false);
    try {
      await client.request(
        { type: "UpdatePluginGrants", plugin_id: plugin.id, grants: next },
        "Plugin",
      );
      setSaved(true);
      void queryClient.invalidateQueries({ queryKey: pluginKeys.all });
    } catch (error) {
      setProblem(errorMessageOf(error));
    }
  };
  return (
    <div data-part="plugin-manager.grants">
      {problem && (
        <p data-part="plugin-manager.error" role="alert">
          {t(problem.id, problem.args)}
        </p>
      )}
      {saved && (
        <p data-part="plugin-manager.status" role="status">
          {t("plugin-manager-grants-saved")}
        </p>
      )}
      <Permissions
        permissions={permissions}
        grants={grants}
        accounts={accounts.data ?? []}
        onChange={(g) => {
          setSaved(false);
          setGrants(g);
        }}
      />
      {permissions.length > 0 && (
        <button
          type="button"
          data-part="form.button"
          data-action="save-grants"
          disabled={!dirty || !grantsValid(grants)}
          onClick={() => void save()}
        >
          {t("plugin-manager-grants-save")}
        </button>
      )}
    </div>
  );
}

function PluginRow({ plugin }: { plugin: PluginInfo }) {
  const t = useT();
  const client = useClient();
  const queryClient = useQueryClient();
  const [open, setOpen] = useState<Section | null>(null);
  const [confirming, setConfirming] = useState(false);
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState<LocalizedMessage | null>(null);
  const hasSettings = (plugin.manifest.contributes?.settings?.length ?? 0) > 0;
  const sections = SECTIONS.filter((s) => s !== "settings" || hasSettings);
  const source = sourceText(plugin);
  const sectionId = `plugin-${plugin.id}-section`;

  const run = async (work: () => Promise<unknown>) => {
    setBusy(true);
    setProblem(null);
    try {
      await work();
      void queryClient.invalidateQueries({ queryKey: pluginKeys.all });
    } catch (error) {
      setProblem(errorMessageOf(error));
    } finally {
      setBusy(false);
    }
  };

  const toggle = () =>
    run(() =>
      client.request(
        { type: plugin.enabled ? "DisablePlugin" : "EnablePlugin", plugin_id: plugin.id },
        "Plugin",
      ),
    );
  const uninstall = () =>
    run(() => client.request({ type: "UninstallPlugin", plugin_id: plugin.id }, "Done"));

  return (
    <li
      data-part="plugin-manager.plugin"
      data-plugin={plugin.id}
      data-state={plugin.state}
      aria-busy={busy}
    >
      <div data-part="plugin-manager.header">
        <h3 data-part="plugin-manager.name">
          {t("plugin-manager-name", {
            name: plugin.manifest.name,
            version: plugin.manifest.version,
          })}
        </h3>
        <span data-part="plugin-manager.state" data-state={plugin.state}>
          {t(`plugin-state-${plugin.state}`)}
        </span>
        <label data-part="plugin-manager.enabled">
          <input
            type="checkbox"
            role="switch"
            data-action="toggle"
            checked={plugin.enabled}
            disabled={busy}
            onChange={() => void toggle()}
          />
          {t("plugin-manager-enabled", { name: plugin.manifest.name })}
        </label>
      </div>
      {plugin.manifest.description !== undefined && (
        <p data-part="plugin-manager.description">{plugin.manifest.description}</p>
      )}
      <p data-part="plugin-manager.source" data-source={plugin.source.kind}>
        {t(source.id, source.args)}
      </p>
      {(plugin.localized || plugin.error) && (
        <p data-part="plugin-manager.crash" role="alert">
          {plugin.localized
            ? t(plugin.localized.id, { plugin: plugin.manifest.name, ...plugin.localized.args })
            : t("plugin-manager-crashed", { error: plugin.error ?? "" })}
        </p>
      )}
      {problem && (
        <p data-part="plugin-manager.error" role="alert">
          {t(problem.id, problem.args)}
        </p>
      )}
      <div data-part="plugin-manager.actions">
        {sections.map((section) => (
          <button
            key={section}
            type="button"
            data-part="form.button"
            data-action={section}
            aria-expanded={open === section}
            aria-controls={open === section ? sectionId : undefined}
            onClick={() => setOpen(open === section ? null : section)}
          >
            {t(`plugin-manager-show-${section}`)}
          </button>
        ))}
        <button
          type="button"
          data-part="form.button"
          data-action="uninstall"
          data-variant="danger"
          disabled={busy}
          onClick={() => setConfirming(true)}
        >
          {t("plugin-manager-uninstall")}
        </button>
      </div>
      {confirming && (
        <div
          data-part="plugin-manager.confirm"
          role="group"
          aria-label={t("plugin-manager-uninstall")}
        >
          <p data-part="plugin-manager.confirm.text">
            {t("plugin-manager-uninstall-confirm", { name: plugin.manifest.name })}
          </p>
          <button
            type="button"
            data-part="form.button"
            data-action="confirm-uninstall"
            data-variant="danger"
            disabled={busy}
            onClick={() => void uninstall()}
          >
            {t("plugin-manager-uninstall-yes")}
          </button>
          <button
            type="button"
            data-part="form.button"
            data-action="cancel-uninstall"
            onClick={() => setConfirming(false)}
          >
            {t("plugin-manager-uninstall-no")}
          </button>
        </div>
      )}
      {open !== null && (
        <div data-part="plugin-manager.section" data-section={open} id={sectionId}>
          {open === "permissions" && <GrantsEditor plugin={plugin} />}
          {open === "settings" && <Settings pluginId={plugin.id} />}
          {open === "log" && <Logs pluginId={plugin.id} />}
          {open === "layout" && <Placements pluginId={plugin.id} onDone={() => setOpen(null)} />}
        </div>
      )}
    </li>
  );
}

function ManagerContent() {
  const t = useT();
  const store = useUiStoreApi();
  const openWindow = useUi((s) => s.openWindow);
  const plugins = usePlugins();
  const install = (mode: InstallMode) => {
    pluginsStoreFor(store).getState().setInstallMode(mode);
    openWindow(INSTALL_DIALOG);
  };
  const list = plugins.data ?? [];
  return (
    <>
      <div data-part="plugin-manager.toolbar">
        <button
          type="button"
          data-part="form.button"
          data-action="install"
          onClick={() => install("package")}
        >
          {t("plugin-manager-install")}
        </button>
        <button
          type="button"
          data-part="form.button"
          data-action="load-dev"
          onClick={() => install("dev")}
        >
          {t("plugin-manager-load-dev")}
        </button>
      </div>
      {plugins.isError && (
        <p data-part="plugin-manager.error" role="alert">
          {t(errorMessageOf(plugins.error).id)}
        </p>
      )}
      {plugins.isSuccess && list.length === 0 && (
        <p data-part="plugin-manager.empty">{t("plugin-manager-empty")}</p>
      )}
      {list.length > 0 && (
        <ul data-part="plugin-manager.list">
          {list.map((plugin) => (
            <PluginRow key={plugin.id} plugin={plugin} />
          ))}
        </ul>
      )}
    </>
  );
}

/** Widget `plugin-manager`. */
export function PluginManager({ node }: WidgetProps) {
  const t = useT();
  const label = useLabel();
  return (
    <section
      data-part="plugin-manager"
      data-node-id={node.id ?? undefined}
      aria-label={node.label ? label(node.label) : t("plugin-manager-label")}
    >
      <ManagerContent />
    </section>
  );
}

/** Dialog `plugin-manager`. */
export function PluginManagerDialog() {
  return (
    <div data-part="plugin-manager">
      <ManagerContent />
    </div>
  );
}
