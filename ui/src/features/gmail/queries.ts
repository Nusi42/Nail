/**
 * Data of the Gmail UI through `NailClient`: which account is a Gmail
 * account, its labels (with counts), label writes (queued by the core in
 * the label queue) and the label views / category filters (searches).
 */
import { useQuery, type QueryClient } from "@tanstack/react-query";
import type { NailClient } from "../../api/client";
import type {
  Account,
  AccountId,
  FolderId,
  GmailCategory,
  GmailLabel,
  GmailLabelId,
  GmailLabelSettings,
} from "../../api/generated";
import { queryKeys, useAccounts, useClient } from "../../state/queries";
import type { UiStore } from "../../state/uiStore";
import { categoryQuery, labelQuery } from "./model";

/** Query keys of this feature. */
export const gmailKeys = {
  all: ["gmail"] as const,
  labels: (accountId: AccountId) => ["gmail", "labels", accountId] as const,
};

/** The account if it is a Gmail account. */
export function useGmailAccount(accountId: AccountId | null): Account | null {
  const accounts = useAccounts();
  return accounts.data?.find((a) => a.id === accountId && a.kind === "gmail") ?? null;
}

/** Whether the cached accounts say `accountId` is a Gmail account. */
export function isGmailAccount(queryClient: QueryClient, accountId: AccountId | null): boolean {
  const accounts = queryClient.getQueryData<Account[]>(queryKeys.accounts);
  return accounts?.some((a) => a.id === accountId && a.kind === "gmail") ?? false;
}

export function useGmailLabels(accountId: AccountId | null) {
  const client = useClient();
  return useQuery({
    queryKey: gmailKeys.labels(accountId ?? ""),
    queryFn: async () =>
      (
        await client.request(
          { type: "ListGmailLabels", account_id: accountId ?? "" },
          "GmailLabels",
        )
      ).labels,
    enabled: accountId !== null,
  });
}

export interface GmailWriteContext {
  client: NailClient;
  queryClient: QueryClient;
}

function refresh(ctx: GmailWriteContext, accountId: AccountId): void {
  void ctx.queryClient.invalidateQueries({ queryKey: gmailKeys.labels(accountId) });
}

/** Creates (`labelId` `null`) or updates a label. */
export async function saveLabel(
  ctx: GmailWriteContext,
  accountId: AccountId,
  labelId: GmailLabelId | null,
  settings: GmailLabelSettings,
): Promise<GmailLabel> {
  const { label } = await ctx.client.request(
    labelId === null
      ? { type: "CreateGmailLabel", account_id: accountId, settings }
      : { type: "UpdateGmailLabel", account_id: accountId, label_id: labelId, settings },
    "GmailLabelSaved",
  );
  refresh(ctx, accountId);
  if (labelId !== null) {
    // A rename renames the tag on messages: lists show it.
    void ctx.queryClient.invalidateQueries({ queryKey: queryKeys.allMessages });
    void ctx.queryClient.invalidateQueries({ queryKey: queryKeys.allSearches });
  }
  return label;
}

export async function deleteLabel(
  ctx: GmailWriteContext,
  accountId: AccountId,
  labelId: GmailLabelId,
): Promise<void> {
  await ctx.client.request(
    { type: "DeleteGmailLabel", account_id: accountId, label_id: labelId },
    "Done",
  );
  refresh(ctx, accountId);
  void ctx.queryClient.invalidateQueries({ queryKey: queryKeys.allMessages });
  void ctx.queryClient.invalidateQueries({ queryKey: queryKeys.allSearches });
}

/** Shows a label's messages across folders (a search on its tag). */
export function showLabel(store: UiStore, accountId: AccountId, tag: string): void {
  const text = labelQuery(tag);
  const state = store.getState();
  state.setSearchInput(text);
  state.setSearch({ text, accountId, folderId: null, error: null });
}

/** Filters the inbox by a category (`null` = the whole inbox). */
export function showCategory(
  store: UiStore,
  accountId: AccountId,
  inbox: FolderId,
  category: GmailCategory | null,
): void {
  const state = store.getState();
  if (state.activeFolderId !== inbox) state.selectFolder(inbox);
  if (category === null) {
    state.setSearch(null);
    return;
  }
  const text = categoryQuery(category);
  state.setSearchInput(text);
  state.setSearch({ text, accountId, folderId: inbox, error: null });
}
