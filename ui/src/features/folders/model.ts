/**
 * What the folder actions may do (ADR 0196), from the folders and accounts
 * the UI has loaded. The core checks everything again; these rules only
 * keep impossible actions out of the menus.
 */
import type { QueryClient } from "@tanstack/react-query";
import type { Account, AccountKind, Folder } from "../../api/generated";
import { queryKeys } from "../../state/queries";

/** Accounts whose folders Nail manages (Gmail has labels; virtual accounts merge). */
export const MANAGED_KINDS: readonly AccountKind[] = ["imap", "pop3", "local", "exchange"];

export function canManage(kind: AccountKind | null): boolean {
  return kind !== null && MANAGED_KINDS.includes(kind);
}

/** Inbox, Sent, Trash, … cannot be renamed, moved or deleted. */
export function isSystemFolder(folder: Folder): boolean {
  return folder.role !== "other";
}

/** The account's kind, from the loaded account list. */
export function accountKind(
  queryClient: QueryClient,
  accountId: string | null,
): AccountKind | null {
  if (accountId === null) return null;
  const accounts = queryClient.getQueryData<Account[]>(queryKeys.accounts);
  return accounts?.find((a) => a.id === accountId)?.kind ?? null;
}

/** The account's loaded folders. */
export function loadedFolders(queryClient: QueryClient, accountId: string | null): Folder[] {
  if (accountId === null) return [];
  return queryClient.getQueryData<Folder[]>(queryKeys.folders(accountId)) ?? [];
}

/** Whether `folder` is `ancestorId` or below it. */
export function isInside(folders: readonly Folder[], folder: Folder, ancestorId: string): boolean {
  const byId = new Map(folders.map((f) => [f.id, f]));
  let current: Folder | undefined = folder;
  for (let depth = 0; current && depth <= folders.length; depth++) {
    if (current.id === ancestorId) return true;
    current = current.parent_id === null ? undefined : byId.get(current.parent_id);
  }
  return false;
}

/** The folder's subfolders at every depth. */
export function subfoldersOf(folders: readonly Folder[], folder: Folder): Folder[] {
  return folders.filter((f) => f.id !== folder.id && isInside(folders, f, folder.id));
}

/** Delete into the account's Trash, or only for good (no Trash, or in it already). */
export function deleteMode(folders: readonly Folder[], folder: Folder): "trash" | "permanent" {
  const trash = folders.find((f) => f.role === "trash");
  return trash && !isInside(folders, folder, trash.id) ? "trash" : "permanent";
}

/** Where a folder can move: not into itself or a subfolder. */
export function moveTargets(folders: readonly Folder[], folder: Folder): Folder[] {
  return folders.filter((f) => !isInside(folders, f, folder.id));
}
