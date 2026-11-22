/**
 * The plug-in manager (layout widget `plugin-manager`, also the dialog
 * `plugin-manager`; ADR 0050): the installed plug-ins with their state,
 * turning them on and off, their permissions (grants), settings, log,
 * layout placements and removal, plus installing a package and loading a
 * development folder (both through the install dialog). M4a: the
 * confirmation policy of plug-ins that may write, and the activity (hooks,
 * background jobs, CPU time, what the plug-in adds).
 *
 * Remove… asks first in the modal confirmation `plugin-uninstall` (a
 * destructive `ConfirmDialog`, R6 / ADR 0239). On / off, the permissions,
 * the confirmation policy and a plug-in's settings are settings drafts:
 * nothing is written before the save bar's Save (the settings section's,
 * or the dialog's own).
 */
import { useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { errorMessageOf } from "../../api/client";
import type { PluginGrant, PluginInfo } from "../../api/generated";
import { useLabel, useT } from "../../i18n/I18nProvider";
import type { DialogProps } from "../../layout/dialogRegistry";
import type { WidgetProps } from "../../layout/widgetRegistry";
import { useAccounts, useClient } from "../../state/queries";
import { sameJson } from "../../text/json";
import { useFormNotice, useSettingsForm, useSettingsGuard } from "../settings/draft";
import { useUi, useUiStoreApi, type UiStore } from "../../state/uiStore";
import { PolicyEditor } from "../agents/PolicyEditor";
import { Activity } from "./Activity";
import { isWritePermission } from "./extensions";
import { grantsValid, orderedGrants } from "./grants";
import { INSTALL_DIALOG } from "./InstallDialog";
import { Logs } from "./Logs";
import { Permissions } from "./Permissions";
import { Placements } from "./Placements";
import { pluginKeys, usePlugins } from "./queries";
import { Settings } from "./Settings";
import { pluginsStoreFor, type InstallMode } from "./store";
import { hasText } from "../../text/empty";
import { useBusyAction } from "../../state/busyAction";
import {
  askInWindow,
  ConfirmDialog,
  pendingSlot,
  SettingsSaveBar,
  usePendingQuestion,
} from "../../widgets/dialog";

/** Dialog name. */
export const MANAGER_DIALOG = "plugin-manager";
/** The confirmation before a plug-in is removed. */
export const UNINSTALL_DIALOG = "plugin-uninstall";

/** The plug-in the open removal question is about (per UI store). */
interface PendingUninstall {
  id: string;
  name: string;
}

const pendingUninstall = pendingSlot<PendingUninstall>();

/** Asks before removing `plugin` (`plugin-uninstall`). */
export function askUninstall(store: UiStore, plugin: PluginInfo): void {
  askInWindow(pendingUninstall, store, UNINSTALL_DIALOG, {
    id: plugin.id,
    name: plugin.manifest.name,
  });
}

/** Dialog `plugin-uninstall`: "Remove “X”?" — Remove plug-in (red), Cancel. */
export function UninstallDialog({ close }: DialogProps) {
  const t = useT();
  const store = useUiStoreApi();
  const client = useClient();
  const queryClient = useQueryClient();
  // However the question ends (Cancel, ×, Esc, the removal), it is forgotten.
  const what = usePendingQuestion(pendingUninstall, store);
  const { busy, problem, run } = useBusyAction();
  const uninstall = async () => {
    if (what === null) return;
    await run(async () => {
      await client.request({ type: "UninstallPlugin", plugin_id: what.id }, "Done");
      void queryClient.invalidateQueries({ queryKey: pluginKeys.all });
      close();
    });
  };
  return (
    <ConfirmDialog
      kind="uninstall-plugin"
      risk="destructive"
      question={t("plugin-manager-uninstall-confirm", { name: what?.name ?? "" })}
      detail={<p data-part="dialog.note">{t("plugin-manager-uninstall-note")}</p>}
      action={t("plugin-manager-uninstall")}
      onConfirm={() => void uninstall()}
      canConfirm={what !== null}
      cancel={{ onClick: close }}
      busy={busy}
      error={problem ? t(problem.id, problem.args) : null}
    />
  );
}

type Section = "permissions" | "confirmations" | "activity" | "settings" | "log" | "layout";
export const MANAGER_SECTIONS: readonly Section[] = [
  "permissions",
  "confirmations",
  "activity",
  "settings",
  "log",
  "layout",
];

/** The sections a plug-in's row offers (confirmations only for plug-ins that may write). */
export function sectionsOf(plugin: PluginInfo): Section[] {
  const hasSettings = (plugin.manifest.contributes?.settings?.length ?? 0) > 0;
  const writes = (plugin.manifest.permissions ?? []).some(isWritePermission);
  return MANAGER_SECTIONS.filter(
    (s) => (s !== "settings" || hasSettings) && (s !== "confirmations" || writes),
  );
}

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

/**
 * A plug-in's permissions (R6, ADR 0239): a settings draft, written with the
 * section's Save (`UpdatePluginGrants`); Discard drops the edits.
 */
function GrantsEditor({ plugin }: { plugin: PluginInfo }) {
  const t = useT();
  const client = useClient();
  const queryClient = useQueryClient();
  const accounts = useAccounts();
  const permissions = plugin.manifest.permissions ?? [];
  const { notice, outcome, clear, problems } = useFormNotice();
  const form = useSettingsForm<PluginGrant[]>(plugin.grants, {
    equal: (a, b) => sameJson(orderedGrants(permissions, a), orderedGrants(permissions, b)),
    canSave: grantsValid,
    save: async (grants) => {
      await client.request(
        {
          type: "UpdatePluginGrants",
          plugin_id: plugin.id,
          grants: orderedGrants(permissions, grants),
        },
        "Plugin",
      );
      await queryClient.invalidateQueries({ queryKey: pluginKeys.all });
    },
    ...outcome("plugin-manager-grants-saved"),
  });
  const failure = problems([])[0];
  return (
    <div data-part="plugin-manager.grants">
      {failure && (
        <p data-part="plugin-manager.error" role="alert">
          {t(failure.id, failure.args)}
        </p>
      )}
      {notice !== null && !form.dirty && (
        <p data-part="plugin-manager.status" role="status">
          {t(notice)}
        </p>
      )}
      <Permissions
        permissions={permissions}
        grants={form.value}
        accounts={accounts.data ?? []}
        onChange={(g) => {
          clear();
          form.set(g);
        }}
      />
    </div>
  );
}

function PluginRow({ plugin }: { plugin: PluginInfo }) {
  const t = useT();
  const client = useClient();
  const queryClient = useQueryClient();
  const store = useUiStoreApi();
  const [open, setOpen] = useState<Section | null>(null);
  const busyAction = useBusyAction();
  const { busy, problem } = busyAction;
  const sections = sectionsOf(plugin);
  const source = sourceText(plugin);
  const sectionId = `plugin-${plugin.id}-section`;

  // On / off is a settings draft too (R6): written with the section's Save.
  const enabled = useSettingsForm(plugin.enabled, {
    save: (on) =>
      busyAction.run(async () => {
        await client.request(
          { type: on ? "EnablePlugin" : "DisablePlugin", plugin_id: plugin.id },
          "Plugin",
        );
        await queryClient.invalidateQueries({ queryKey: pluginKeys.all });
      }),
  });

  return (
    <li
      data-part="plugin-manager.plugin"
      data-plugin={plugin.id}
      data-state={plugin.state}
      aria-busy={busy}
    >
      <div data-part="plugin-manager.header">
        <h3 data-part="plugin-manager.name">
          {t("plugin-install-name", {
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
            checked={enabled.value}
            disabled={busy}
            onChange={(event) => {
              enabled.set(event.target.checked);
            }}
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
      {(plugin.localized !== undefined || hasText(plugin.error)) && (
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
            onClick={() => {
              setOpen(open === section ? null : section);
            }}
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
          onClick={() => {
            askUninstall(store, plugin);
          }}
        >
          {t("plugin-manager-remove")}
        </button>
      </div>
      {open !== null && (
        <div data-part="plugin-manager.section" data-section={open} id={sectionId}>
          {open === "permissions" && <GrantsEditor plugin={plugin} />}
          {open === "confirmations" && (
            <PolicyEditor
              key={plugin.id}
              actor={{ kind: "plugin", plugin_id: plugin.id }}
              name={plugin.manifest.name}
            />
          )}
          {open === "activity" && <Activity plugin={plugin} />}
          {open === "settings" && <Settings pluginId={plugin.id} />}
          {open === "log" && <Logs pluginId={plugin.id} />}
          {open === "layout" && (
            <Placements
              pluginId={plugin.id}
              onDone={() => {
                setOpen(null);
              }}
            />
          )}
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
          onClick={() => {
            install("package");
          }}
        >
          {t("plugin-manager-install")}
        </button>
        <button
          type="button"
          data-part="form.button"
          data-action="load-dev"
          onClick={() => {
            install("dev");
          }}
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

/**
 * Dialog `plugin-manager`: the same drafts as the settings section (R6), so
 * the window has the section's save bar and asks before it closes with
 * unsaved changes.
 */
export function PluginManagerDialog() {
  const t = useT();
  const name = t("plugin-manager-label");
  useSettingsGuard(name);
  return (
    <div data-part="plugin-manager">
      <ManagerContent />
      <SettingsSaveBar name={name} />
    </div>
  );
}
