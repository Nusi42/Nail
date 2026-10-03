/**
 * Feature slot of M2d WP62 for plug-ins (contract ADR 0050; package ADR
 * 0052-plugin-ui): the renderer of plug-in component trees (dialogs,
 * panels), plug-in commands in toolbars / menus / keys, notifications, the
 * install dialog with its permissions, the plug-in manager (enable /
 * disable, grants, settings, logs, uninstall, development folders) and
 * placements proposed through the merge flow.
 *
 * M4a WP104 (contract ADR 0079) adds the extension points' UI. Mount
 * points for the integrator (components exported below):
 * - reader: `PluginBanners` (above the body), `PluginBadges` (header),
 *   `AttachmentPreviewPane` (attachment preview), `AttachmentActionMenu`
 *   (each attachment), `LinkActionMenu` (where a link is offered);
 * - composer: `ComposerExtensionBar` (toolbar), `useComposerEdits` +
 *   `applyComposerEdit` (apply edits of plug-ins / agents),
 *   `ComposerEditNotices` (who edited), `PluginMarkdownBlock` (Markdown
 *   preview of `<plugin>.<block>` fences), `sendDraftChecked` (instead of
 *   `SendDraft` → `SendQueued`; the veto dialog is the effect
 *   `SendVetoDialog`);
 * - mock core: `answerSendDraft` (route `SendDraft` through it),
 *   `installDemoExtensions` (demo plug-ins).
 * The plug-in manager gains the confirmation policy of writing plug-ins
 * and the activity (hooks, jobs, CPU, what it adds); the install dialog
 * lists every contribution and highlights write permissions.
 *
 * Only WP62 / WP104 edit the files in this directory (and `locales/{en,de}/
 * plugins.ftl`). The central registry (`../registry.ts`) merges these
 * exports into the app; see `../README.md`.
 */
import { webNotificationsAvailable } from "../triage/Notifications";
import type { Feature, FeatureEventHandler } from "../types";
import { RUN_COMMAND, runCommandHandler } from "./commands";
import { isForThisUi } from "./context";
import { INSTALL_DIALOG, InstallDialog } from "./InstallDialog";
import { PluginDialogs } from "./PluginDialogs";
import { MANAGER_DIALOG, PluginManager, PluginManagerDialog } from "./PluginManager";
import { PluginNotifications } from "./PluginNotifications";
import { PluginPanel } from "./PluginPanel";
import { appendLogLine, pluginKeys } from "./queries";
import { SendVetoDialog } from "./SendVeto";
import { pluginsStoreFor } from "./store";
import type { PluginInfo, PluginLogEntry, PluginUiNode } from "../../api/generated";

export { pluginKeys } from "./queries";
export { RUN_COMMAND, usePluginLayout, withPluginActions } from "./commands";
export { PLUGIN_BASE_CSS, PLUGIN_PARTS, pluginPartSelector } from "./parts";
export { INSTALL_DIALOG, MANAGER_DIALOG };
export { PluginBadges, PluginBanners } from "./MessageAnnotations";
export {
  AttachmentPreviewPane,
  previewImageSrc,
  useHasAttachmentPreview,
} from "./AttachmentPreview";
export { AttachmentActionMenu, LinkActionMenu, runPluginAction } from "./ActionMenus";
export {
  ComposerEditNotices,
  ComposerExtensionBar,
  PluginMarkdownBlock,
  useComposerEdits,
} from "./ComposerExtensions";
export { SendVetoDialog, sendDraftChecked } from "./SendVeto";
export {
  applyComposerEdit,
  attachmentActionsFor,
  composerActionsFor,
  linkActionsFor,
  markdownBlockOwner,
  markdownBlocksFor,
  previewRenderersFor,
} from "./extensions";
export { answerSendDraft, checkSendVeto } from "./extensionsMock";
export { installDemoExtensions } from "./extensionsSeed";
export type { PendingEdit } from "./store";

/** Layout widget kinds → components. */
export const widgets: Feature["widgets"] = {
  "plugin-panel": PluginPanel,
  "plugin-manager": PluginManager,
};

/** Action ids → handlers. */
export const actions: Feature["actions"] = {
  /** Runs a plug-in command (`plugin.<id>.<command>` are aliases of it). */
  [RUN_COMMAND]: runCommandHandler,
  /** Opens the install dialog for a package. */
  "plugins.install": {
    run: (ctx) => {
      pluginsStoreFor(ctx.store).getState().setInstallMode("package");
      ctx.store.getState().openWindow(INSTALL_DIALOG);
    },
  },
  /** Opens the install dialog for a development folder. */
  "plugins.load-dev": {
    run: (ctx) => {
      pluginsStoreFor(ctx.store).getState().setInstallMode("dev");
      ctx.store.getState().openWindow(INSTALL_DIALOG);
    },
  },
  /** Opens the plug-in manager. */
  "plugins.manage": { run: (ctx) => ctx.store.getState().openWindow(MANAGER_DIALOG) },
};

/** Dialog names → components. */
export const dialogs: Feature["dialogs"] = {
  [INSTALL_DIALOG]: InstallDialog,
  [MANAGER_DIALOG]: PluginManagerDialog,
};

/** Crashes that are worth telling the user about. */
const CRASHES = ["error", "cpu-budget", "memory-limit"];

/** The core's plug-in events: queries, dialogs, notifications, panels, logs. */
export const handlePluginsEvent: FeatureEventHandler = (event, { client, queryClient, store }) => {
  const plugins = pluginsStoreFor(store).getState();
  switch (event.type) {
    case "PluginsChanged":
      void queryClient.invalidateQueries({ queryKey: pluginKeys.all });
      break;
    case "PluginActivityChanged":
      void queryClient.invalidateQueries({ queryKey: pluginKeys.activity(event.plugin_id) });
      break;
    case "ComposerEditRequested":
      if (isForThisUi(client, event.client_id)) {
        plugins.pushEdit({ draftId: event.draft_id, by: event.by, edit: event.edit });
      }
      break;
    case "PluginDialogOpened":
      if (isForThisUi(client, event.dialog.client_id)) plugins.openDialog(event.dialog);
      break;
    case "PluginDialogClosed":
      plugins.closeDialog(event.dialog_id);
      break;
    case "PluginNotification": {
      const n = event.notification;
      if (!isForThisUi(client, n.client_id)) break;
      plugins.pushToast({ kind: "plugin", id: n.id, notification: n });
      if (n.spec.system && webNotificationsAvailable() && Notification.permission === "granted") {
        try {
          new Notification(n.spec.title, n.spec.body === undefined ? {} : { body: n.spec.body });
        } catch (error) {
          console.warn("nail: cannot show a notification", error);
        }
      }
      break;
    }
    case "PluginPanelChanged":
      queryClient.setQueriesData<PluginUiNode[]>(
        { queryKey: [...pluginKeys.panels, event.panel] },
        () => event.tree,
      );
      break;
    case "PluginLogLine":
      queryClient.setQueryData<PluginLogEntry[]>(pluginKeys.logs(event.entry.plugin_id), (old) =>
        appendLogLine(old, event.entry),
      );
      break;
    case "PluginStopped": {
      plugins.closePluginDialogs(event.plugin_id);
      void queryClient.invalidateQueries({ queryKey: pluginKeys.list });
      if (!CRASHES.includes(event.reason)) break;
      const name =
        queryClient
          .getQueryData<PluginInfo[]>(pluginKeys.list)
          ?.find((p) => p.id === event.plugin_id)?.manifest.name ?? event.plugin_id;
      plugins.pushToast({
        kind: "stopped",
        id: `stopped-${event.plugin_id}-${Date.now()}`,
        pluginId: event.plugin_id,
        level: "error",
        message: {
          id: event.localized?.id ?? `plugin-stopped-${event.reason}`,
          args: { ...event.localized?.args, plugin: name },
        },
      });
      break;
    }
  }
};

/** Handlers for the core's events. */
export const events: Feature["events"] = [handlePluginsEvent];
/** Background components: plug-in dialogs, notifications and the send veto of this UI. */
export const effects: Feature["effects"] = [PluginDialogs, PluginNotifications, SendVetoDialog];

/**
 * Settings labels per widget kind, for the settings search (M5b WP163, ADR
 * 0109): Fluent ids of the setting labels each widget shows.
 */
export const settings: NonNullable<Feature["settings"]> = {
  "plugin-manager": {
    labels: [
      "plugin-manager-tab",
      "plugin-manager-label",
      "plugin-manager-install",
      "plugin-manager-load-dev",
      "plugin-manager-show-permissions",
      "plugin-manager-show-settings",
      "plugin-manager-show-confirmations",
      "plugin-manager-show-activity",
      "plugin-manager-show-log",
      "plugin-manager-show-layout",
    ],
    keywords: "settings-keywords-plugin-manager",
  },
};
