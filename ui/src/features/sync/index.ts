/**
 * Feature slot of M2c WP54 for per-folder sync policies (full / headers
 * only / recent N days) and bodies downloaded on demand (contract
 * ADR 0043): the folder's sync settings (settings tab and the folder
 * menu's dialog), the "not downloaded" marker in the list and the reader,
 * "Download full message".
 *
 * Only WP54 edits the files in this directory (plus
 * `crates/nail-config/src/widgets/sync.rs` for widget kinds and
 * `locales/{en,de}/sync.ftl`). The central registry (`../registry.ts`)
 * merges these exports into the app; see `../README.md`.
 */
import type { MessageId } from "../../api/generated";
import type { ActionContext } from "../../actions/registry";
import { findSummary } from "../../state/mailOps";
import { queryKeys } from "../../state/queries";
import { contextTarget } from "../menus";
import type { Feature, FeatureEventHandler } from "../types";
import { syncStoreFor } from "./store";
import {
  MessageBodyBar,
  SYNC_FOLDER_DIALOG,
  SyncFolderDialog,
  SyncPoliciesWidget,
  syncKeys,
} from "./Sync";

export { SYNC_FOLDER_DIALOG };

/** Layout widget kinds → components. */
export const widgets: Feature["widgets"] = {
  "sync-folder-policies": SyncPoliciesWidget,
  "sync-message-body": MessageBodyBar,
};

function targetMessages(ctx: ActionContext): MessageId[] {
  const id = contextTarget(ctx.store)?.messageId ?? ctx.store.getState().selectedMessageId;
  return id === null ? [] : [id];
}

/**
 * Downloads the bodies of messages that only have their headers locally;
 * the reader shows them once they are stored.
 */
export async function downloadBodies(ctx: ActionContext, ids: MessageId[]): Promise<void> {
  if (ids.length === 0) return;
  const sync = syncStoreFor(ctx.store).getState();
  sync.started(ids);
  try {
    const answer = await ctx.client.request(
      { type: "FetchMessageBodies", message_ids: ids },
      "BodiesFetched",
    );
    sync.finished(answer.fetched, answer.failed);
  } catch (error) {
    sync.finished([], ids);
    throw error;
  } finally {
    for (const id of ids) {
      void ctx.queryClient.invalidateQueries({ queryKey: queryKeys.message(id) });
    }
    void ctx.queryClient.invalidateQueries({ queryKey: queryKeys.allMessages });
  }
}

/** Action ids → handlers. */
export const actions: Feature["actions"] = {
  /** Downloads the full message (bodies on demand). */
  "sync.download-message": {
    run: (ctx) => downloadBodies(ctx, targetMessages(ctx)),
    enabled: (ctx) =>
      targetMessages(ctx).some((id) => findSummary(ctx.queryClient, id)?.headers_only === true),
  },
  /** Opens the sync settings of the menu's folder (else the active folder). */
  "sync.folder-policy": {
    run: (ctx) => {
      const folderId = contextTarget(ctx.store)?.folderId ?? ctx.store.getState().activeFolderId;
      syncStoreFor(ctx.store).getState().setFolder(folderId);
      ctx.store.getState().openWindow(SYNC_FOLDER_DIALOG);
    },
    enabled: (ctx) =>
      (contextTarget(ctx.store)?.folderId ?? ctx.store.getState().activeFolderId) !== null,
  },
};

/** Dialog names → components. */
export const dialogs: NonNullable<Feature["dialogs"]> = {
  [SYNC_FOLDER_DIALOG]: SyncFolderDialog,
};

/**
 * The window model of this feature's windows (M5c, ADR 0116): tool windows
 * are non-modal with their default and minimum sizes; only confirmations
 * (`confirmation: true`) are modal. The layout's `window` properties win.
 */
export const windows: NonNullable<Feature["windows"]> = {
  [SYNC_FOLDER_DIALOG]: {
    size: { width: 560, height: 480 },
    min_size: { width: 420, height: 360 },
  },
};

/** Policies live in `accounts.kdl`: refetch them when it changes. */
export const handleSyncEvent: FeatureEventHandler = (event, { queryClient }) => {
  if (event.type === "AccountsChanged") {
    void queryClient.invalidateQueries({ queryKey: syncKeys.all });
  }
};

/** Handlers for the core's events. */
export const events: NonNullable<Feature["events"]> = [handleSyncEvent];

/**
 * Settings labels per widget kind, for the settings search (M5b WP163, ADR
 * 0109): Fluent ids of the setting labels each widget shows.
 */
export const settings: NonNullable<Feature["settings"]> = {
  "sync-folder-policies": {
    labels: [
      "sync-settings-tab",
      "sync-policies-label",
      "sync-enabled-label",
      "sync-idle-label",
      "sync-bodies-label",
      "sync-days",
    ],
    keywords: "settings-keywords-sync-folder-policies",
  },
};
