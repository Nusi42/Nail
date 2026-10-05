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
import type { ActionTargets } from "../../actions/mailTargets";
import { runMailAction } from "../mail-actions/run";

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
 * "Mark as junk / not junk" on conversations (P3, ADR 0164): the selected
 * conversation's messages in the listed folder (or a card's message) in
 * one batch with one undo; the reader shows the new verdict.
 */
export async function markJunkTargets(
  ctx: WriteContext,
  targets: ActionTargets,
  junk: boolean,
): Promise<void> {
  const result = await runMailAction(ctx, targets, { type: "junk", junk });
  invalidateJunk(ctx.queryClient);
  for (const id of result?.message_ids ?? []) {
    void ctx.queryClient.invalidateQueries({ queryKey: queryKeys.message(id) });
  }
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
