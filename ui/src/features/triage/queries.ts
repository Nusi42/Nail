/**
 * Queries and writes of the triage feature: junk status per account,
 * notification settings, the snoozed list; marking junk and snoozing are
 * queued writes like any other (`state/mailOps.ts`).
 */
import { useQuery, type QueryClient } from "@tanstack/react-query";
import type {
  AccountId,
  FolderRole,
  Folder,
  MessageId,
  NotificationSettings,
} from "../../api/generated";
import { queryKeys, useClient } from "../../state/queries";
import { queued, removeFromLists, type WriteContext } from "../../state/mailOps";

export const triageKeys = {
  junk: (accountId: AccountId | null) => ["triage", "junk", accountId] as const,
  allJunk: ["triage", "junk"] as const,
  notifications: ["triage", "notifications"] as const,
  snoozed: ["triage", "snoozed"] as const,
};

export function useJunkStatus(accountId: AccountId | null) {
  const client = useClient();
  return useQuery({
    queryKey: triageKeys.junk(accountId),
    queryFn: async () =>
      (await client.request({ type: "GetJunkStatus", account_id: accountId ?? "" }, "JunkStatus"))
        .status,
    enabled: accountId !== null,
  });
}

export function useNotificationSettings() {
  const client = useClient();
  return useQuery({
    queryKey: triageKeys.notifications,
    queryFn: async (): Promise<NotificationSettings> =>
      (await client.request({ type: "GetNotificationSettings" }, "NotificationSettings")).settings,
  });
}

export function useSnoozed() {
  const client = useClient();
  return useQuery({
    queryKey: triageKeys.snoozed,
    queryFn: async () =>
      (await client.request({ type: "ListSnoozed", account_id: null }, "Snoozed")).messages,
  });
}

/** The role of the active folder, from the folder cache. */
export function activeFolderRole(ctx: WriteContext): FolderRole | null {
  const { activeAccountId, activeFolderId } = ctx.store.getState();
  const folders = ctx.queryClient.getQueryData<Folder[]>(queryKeys.folders(activeAccountId));
  return folders?.find((f) => f.id === activeFolderId)?.role ?? null;
}

/**
 * "Mark as junk / not junk": trains the classifier, sets `$Junk` /
 * `$NotJunk` and moves the mail (to Junk, or out of it) — one queued write.
 * The messages leave the list at once when they will leave its folder.
 */
export async function markJunk(
  ctx: WriteContext,
  ids: readonly MessageId[],
  junk: boolean,
): Promise<void> {
  if (ids.length === 0) return;
  const inJunk = activeFolderRole(ctx) === "junk";
  if (junk !== inJunk && ctx.store.getState().search === null) removeFromLists(ctx, ids);
  await queued(ctx, { type: "MarkJunk", message_ids: [...ids], junk });
  invalidateJunk(ctx.queryClient);
  // The reader shows the new verdict.
  for (const id of ids) void ctx.queryClient.invalidateQueries({ queryKey: queryKeys.message(id) });
}

/** Snoozes messages until a time; they leave every list until they wake. */
export async function snooze(
  ctx: WriteContext,
  ids: readonly MessageId[],
  until: string,
): Promise<void> {
  if (ids.length === 0) return;
  removeFromLists(ctx, ids);
  await queued(ctx, {
    type: "SnoozeMessages",
    message_ids: [...ids],
    until,
    wake_folder_id: null,
  });
  void ctx.queryClient.invalidateQueries({ queryKey: triageKeys.snoozed });
}

/** Brings snoozed messages back now. */
export async function unsnooze(ctx: WriteContext, ids: readonly MessageId[]): Promise<void> {
  if (ids.length === 0) return;
  await queued(ctx, { type: "UnsnoozeMessages", message_ids: [...ids] });
  void ctx.queryClient.invalidateQueries({ queryKey: triageKeys.snoozed });
}

export function invalidateJunk(queryClient: QueryClient): void {
  void queryClient.invalidateQueries({ queryKey: triageKeys.allJunk });
}
