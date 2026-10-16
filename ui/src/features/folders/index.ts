/**
 * Feature slot of folder management (ADR 0196): New folder, Rename, Move
 * and Delete in the folder pane's menus (`context:folder-tree.folder`,
 * `context:folder-tree.account`).
 *
 * - The actions act on the menu's folder (else the active folder); New
 *   folder on an account heading creates at the top of that account. They
 *   are off for Gmail (labels) and virtual accounts, and Rename / Move /
 *   Delete for system folders (Inbox, Sent, Trash, …).
 * - Delete moves the folder into the account's Trash at once; in Trash (or
 *   without one) it asks first (`folder-delete`) and deletes for good.
 * - The windows (`./FolderDialogs.tsx`) send the request and wait until the
 *   server confirmed (`runQueued`); a refusal shows the core's reason.
 * - The mock core answers through `./mock.ts`; the look is `./skin.ts`.
 *
 * Texts: `locales/{en,de}/folders.ftl` (the core's reasons in
 * `core-folders.ftl`). The central registry (`../registry.ts`) merges
 * these exports into the app; see `../README.md`.
 */
import type { ActionInfo } from "../../actions/palette";
import type { ActionContext } from "../../actions/registry";
import { CONFIRMATION_WINDOW_SPEC } from "../../state/windows";
import { runQueued } from "../../state/queuedOps";
import { contextTarget } from "../menus";
import type { Feature } from "../types";
import { FolderDeleteDialog, FolderMoveDialog, FolderNameDialog } from "./FolderDialogs";
import { accountKind, canManage, deleteMode, isSystemFolder, loadedFolders } from "./model";
import {
  FOLDER_DELETE_ACTION,
  FOLDER_DELETE_DIALOG,
  FOLDER_MOVE_ACTION,
  FOLDER_MOVE_DIALOG,
  FOLDER_NAME_DIALOG,
  FOLDER_NEW_ACTION,
  FOLDER_RENAME_ACTION,
} from "./names";
import { foldersStoreFor, type FolderTask } from "./store";

export * from "./names";
export { FOLDERS_SKIN_CSS } from "./skin";

/** The account a menu (or the active folder) is about. */
function targetAccount(ctx: ActionContext): string | null {
  const target = contextTarget(ctx.store);
  return target?.accountId ?? ctx.store.getState().activeAccountId;
}

/** A managed account's folder (the menu's, else the active one) that is not a system folder. */
function changeable(ctx: ActionContext) {
  const target = contextTarget(ctx.store);
  const folderId = target ? target.folderId : ctx.store.getState().activeFolderId;
  const accountId = targetAccount(ctx);
  if (folderId === null || !canManage(accountKind(ctx.queryClient, accountId))) return null;
  const folders = loadedFolders(ctx.queryClient, accountId);
  const folder = folders.find((f) => f.id === folderId);
  if (!folder || accountId === null || isSystemFolder(folder)) return null;
  return { folder, folders, accountId };
}

/** Opens `dialog` for the changeable folder of the menu. */
function openFor(ctx: ActionContext, kind: "rename" | "move" | "delete", dialog: string): void {
  const found = changeable(ctx);
  if (!found) return;
  open(ctx, { kind, accountId: found.accountId, folderId: found.folder.id }, dialog);
}

function open(ctx: ActionContext, task: FolderTask, dialog: string): void {
  foldersStoreFor(ctx.store).getState().setTask(task);
  ctx.store.getState().openWindow(dialog);
}

const isChangeable = (ctx: ActionContext) => changeable(ctx) !== null;

/** Action ids → handlers. */
export const actions: Feature["actions"] = {
  [FOLDER_NEW_ACTION]: {
    run: (ctx) => {
      const accountId = targetAccount(ctx);
      if (accountId === null) return;
      const parentId = contextTarget(ctx.store)?.folderId ?? null;
      open(ctx, { kind: "create", accountId, parentId }, FOLDER_NAME_DIALOG);
    },
    enabled: (ctx) => canManage(accountKind(ctx.queryClient, targetAccount(ctx))),
  },
  [FOLDER_RENAME_ACTION]: {
    run: (ctx) => {
      openFor(ctx, "rename", FOLDER_NAME_DIALOG);
    },
    enabled: isChangeable,
  },
  [FOLDER_MOVE_ACTION]: {
    run: (ctx) => {
      openFor(ctx, "move", FOLDER_MOVE_DIALOG);
    },
    enabled: isChangeable,
  },
  [FOLDER_DELETE_ACTION]: {
    run: async (ctx) => {
      const found = changeable(ctx);
      if (!found) return;
      if (deleteMode(found.folders, found.folder) === "permanent") {
        openFor(ctx, "delete", FOLDER_DELETE_DIALOG);
        return;
      }
      // Into Trash at once; a refusal shows as a failed write.
      await runQueued(ctx.client, {
        type: "DeleteFolder",
        folder_id: found.folder.id,
        permanent: false,
      });
    },
    enabled: isChangeable,
  },
};

/** Command-palette descriptions (`folders.ftl`). */
export const actionInfo: Readonly<Record<string, ActionInfo>> = {
  [FOLDER_NEW_ACTION]: {
    title: "folders-action-new",
    keywords: "folders-action-keywords",
    icon: "folder-plus",
  },
  [FOLDER_RENAME_ACTION]: {
    title: "folders-action-rename",
    keywords: "folders-action-keywords",
    icon: "pencil",
  },
  [FOLDER_MOVE_ACTION]: {
    title: "folders-action-move",
    keywords: "folders-action-keywords",
    icon: "folder-input",
  },
  [FOLDER_DELETE_ACTION]: {
    title: "folders-action-delete",
    keywords: "folders-action-keywords",
    icon: "trash",
  },
};

/** Dialogs by window kind. */
export const dialogs: NonNullable<Feature["dialogs"]> = {
  [FOLDER_NAME_DIALOG]: FolderNameDialog,
  [FOLDER_MOVE_DIALOG]: FolderMoveDialog,
  [FOLDER_DELETE_DIALOG]: FolderDeleteDialog,
};

/** The window model: small tool windows; deleting for good is a confirmation. */
export const windows: NonNullable<Feature["windows"]> = {
  // A name and two buttons: as small as a confirmation, but not modal.
  [FOLDER_NAME_DIALOG]: {
    size: CONFIRMATION_WINDOW_SPEC.size,
    min_size: CONFIRMATION_WINDOW_SPEC.min_size,
  },
  [FOLDER_MOVE_DIALOG]: {
    size: { width: 480, height: 560 },
    min_size: { width: 360, height: 360 },
  },
  [FOLDER_DELETE_DIALOG]: { confirmation: true },
};

/** Layout widget kinds → components (none). */
export const widgets: Feature["widgets"] = {};
