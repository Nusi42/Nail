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
import type { MessageId } from "../../api/generated";
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

/** Action ids → handlers. */
export const actions: Feature["actions"] = {
  /** Marks the selected message as junk (trains the classifier, moves it to Junk). */
  "junk.mark": { run: (ctx) => markJunk(ctx, targets(ctx), true), enabled: hasTarget },
  /** Marks it as not junk (trains the classifier, moves it out of Junk). */
  "junk.not-junk": { run: (ctx) => markJunk(ctx, targets(ctx), false), enabled: hasTarget },
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
