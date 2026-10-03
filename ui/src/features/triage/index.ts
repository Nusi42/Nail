/**
 * Feature slot of M2c WP54 for junk mail, snooze and notifications
 * (contract ADR 0043): "Mark as junk / not junk" and the classifier's
 * settings and status, snoozing and the snoozed list, new-mail
 * notification settings (per account / folder, quiet hours) and showing
 * `NotificationRequested` as a Web Notification in the remote UI (the
 * desktop shell shows OS notifications itself).
 *
 * Only WP54 edits the files in this directory (plus
 * `crates/nail-config/src/widgets/triage.rs` for widget kinds and
 * `locales/{en,de}/triage.ftl`). The central registry (`../registry.ts`)
 * merges these exports into the app; see `../README.md`.
 */
import type { MessageBody, MessageId } from "../../api/generated";
import type { ActionContext } from "../../actions/registry";
import { queryKeys } from "../../state/queries";
import { contextTarget } from "../menus";
import type { Feature, FeatureEventHandler } from "../types";
import { JunkSettingsWidget, JunkVerdictBar } from "./Junk";
import { NotificationSettingsWidget, WebNotifications } from "./Notifications";
import { invalidateJunk, markJunk, triageKeys } from "./queries";
import { SNOOZED_WINDOW, SNOOZE_DIALOG, SnoozeDialog, SnoozeList, SnoozedDialog } from "./Snooze";
import { triageStoreFor } from "./store";

export { SNOOZED_WINDOW, SNOOZE_DIALOG };

/** Layout widget kinds → components. */
export const widgets: Feature["widgets"] = {
  "junk-settings": JunkSettingsWidget,
  "junk-verdict": JunkVerdictBar,
  "snooze-list": SnoozeList,
  "notification-settings": NotificationSettingsWidget,
};

/** The messages an action is about: the context menu's, else the selection. */
function targets(ctx: ActionContext): MessageId[] {
  const fromMenu = contextTarget(ctx.store)?.messageId ?? null;
  const id = fromMenu ?? ctx.store.getState().selectedMessageId;
  return id === null ? [] : [id];
}

const hasTarget = (ctx: ActionContext) => targets(ctx).length > 0;

/** Whether the list shows a Junk folder (of one account or All accounts). */
function inJunkFolder(ctx: ActionContext): boolean {
  const { activeAccountId, activeFolderId } = ctx.store.getState();
  if (activeFolderId === null) return false;
  const folders = ctx.queryClient.getQueryData<{ id: string; role: string }[]>(
    queryKeys.folders(activeAccountId),
  );
  return folders?.find((f) => f.id === activeFolderId)?.role === "junk";
}

/** Whether the target is junk: shown in a Junk folder, or the classifier says so. */
function looksJunk(ctx: ActionContext): boolean {
  if (inJunkFolder(ctx)) return true;
  const id = targets(ctx)[0];
  if (id === undefined) return false;
  const body = ctx.queryClient.getQueryData<MessageBody>(queryKeys.message(id));
  return body?.junk?.junk === true;
}

/** Action ids → handlers. */
export const actions: Feature["actions"] = {
  /** Marks the selected message as junk (trains the classifier, moves it to Junk). */
  "junk.mark": {
    run: (ctx) => markJunk(ctx, targets(ctx), true),
    enabled: (ctx) => hasTarget(ctx) && !inJunkFolder(ctx),
  },
  /** Marks it as not junk (trains the classifier, moves it out of Junk). */
  "junk.not-junk": {
    run: (ctx) => markJunk(ctx, targets(ctx), false),
    // Only for mail in Junk or flagged as junk (M5b review: hidden otherwise).
    enabled: (ctx) => hasTarget(ctx) && looksJunk(ctx),
  },
  /** Opens the snooze picker for the selected message. */
  "snooze.open": {
    run: (ctx) => {
      triageStoreFor(ctx.store).getState().setSnoozing(targets(ctx));
      ctx.store.getState().openWindow(SNOOZE_DIALOG);
    },
    enabled: hasTarget,
  },
  /** Opens the list of snoozed messages. */
  "snooze.list": { run: (ctx) => ctx.store.getState().openWindow(SNOOZED_WINDOW) },
};

/** Dialog names → components (the layout's `snoozed` window wins when it has one). */
export const dialogs: Feature["dialogs"] = {
  [SNOOZE_DIALOG]: SnoozeDialog,
  [SNOOZED_WINDOW]: SnoozedDialog,
};

/**
 * The window model of this feature's windows (M5c, ADR 0116): tool windows
 * are non-modal with their default and minimum sizes; only confirmations
 * (`confirmation: true`) are modal. The layout's `window` properties win.
 */
export const windows: NonNullable<Feature["windows"]> = {
  [SNOOZE_DIALOG]: { size: { width: 440, height: 480 }, min_size: { width: 360, height: 380 } },
  [SNOOZED_WINDOW]: { size: { width: 720, height: 520 }, min_size: { width: 420, height: 300 } },
};

/** Keeps junk status, snoozed list and notification settings current. */
export const handleTriageEvent: FeatureEventHandler = (event, { queryClient }) => {
  switch (event.type) {
    case "JunkStatusChanged":
      invalidateJunk(queryClient);
      break;
    case "SnoozeChanged":
      void queryClient.invalidateQueries({ queryKey: triageKeys.snoozed });
      void queryClient.invalidateQueries({ queryKey: queryKeys.allMessages });
      void queryClient.invalidateQueries({ queryKey: queryKeys.allFolders });
      break;
    case "NotificationSettingsChanged":
      void queryClient.invalidateQueries({ queryKey: triageKeys.notifications });
      break;
  }
};

/** Handlers for the core's events. */
export const events: Feature["events"] = [handleTriageEvent];
/** Background components. */
export const effects: Feature["effects"] = [WebNotifications];

/**
 * Settings labels per widget kind, for the settings search (M5b WP163, ADR
 * 0109): Fluent ids of the setting labels each widget shows.
 */
export const settings: NonNullable<Feature["settings"]> = {
  "junk-settings": {
    labels: [
      "junk-settings-tab",
      "junk-settings-label",
      "junk-settings-classifier",
      "junk-settings-enabled",
      "junk-settings-honor-server",
      "junk-settings-move",
      "junk-settings-threshold",
      "junk-settings-min-training",
      "junk-status-label",
      "junk-reset",
    ],
    keywords: "settings-keywords-junk-settings",
  },
  "notification-settings": {
    labels: [
      "notification-settings-tab",
      "notification-settings-label",
      "notification-general-title",
      "notification-enabled",
      "notification-show-preview",
      "notification-sound",
      "notification-folders",
      "notification-quiet-title",
      "notification-quiet-enabled",
      "notification-quiet-start",
      "notification-quiet-end",
      "notification-overrides-title",
      "notification-browser-allow",
      "notification-test",
    ],
    keywords: "settings-keywords-notification-settings",
  },
};
