/**
 * Data of the Exchange UI through `NailClient`: which accounts are Exchange
 * accounts, their status, other mailboxes, categories, automatic replies,
 * rules and rooms, and the writes (queued by the core in the Exchange
 * operation queue, or the mail queue for Focused Inbox moves).
 */
import { useQuery, type QueryClient } from "@tanstack/react-query";
import type { NailClient } from "../../api/client";
import type {
  Account,
  AccountId,
  AutomaticReplies,
  AutomaticRepliesSettings,
  ExchangeArea,
  ExchangeCategory,
  ExchangeCategoryId,
  ExchangeCategorySettings,
  ExchangeMailbox,
  ExchangeMailboxSettings,
  ExchangeRulesStatus,
  InferenceClassification,
  MessageId,
} from "../../api/generated";
import { patchSummaries, queued } from "../../state/mailOps";
import { queryKeys, useAccounts, useClient } from "../../state/queries";
import type { UiStore } from "../../state/uiStore";
import { focusQuery, tagQuery } from "./model";

/** Query keys of this feature (`area` keys match `ExchangeChanged.areas`). */
export const exchangeKeys = {
  all: ["exchange"] as const,
  account: (accountId: string) => ["exchange", accountId] as const,
  area: (accountId: string, area: ExchangeArea) => ["exchange", accountId, area] as const,
  rooms: (accountId: string, list: string | null, from: string, to: string, min: number | null) =>
    ["exchange", accountId, "rooms", "find", list, from, to, min] as const,
};

/** The Exchange accounts. */
export function useExchangeAccounts(): Account[] {
  const accounts = useAccounts();
  return (accounts.data ?? []).filter((a) => a.kind === "exchange");
}

/** The account if it is an Exchange account. */
export function useExchangeAccount(accountId: AccountId | null): Account | null {
  const accounts = useAccounts();
  return accounts.data?.find((a) => a.id === accountId && a.kind === "exchange") ?? null;
}

/** Whether the cached accounts say `accountId` is an Exchange account. */
export function isExchangeAccount(queryClient: QueryClient, accountId: AccountId | null): boolean {
  const accounts = queryClient.getQueryData<Account[]>(queryKeys.accounts);
  return accounts?.some((a) => a.id === accountId && a.kind === "exchange") ?? false;
}

export function useExchangeStatus(accountId: AccountId | null) {
  const client = useClient();
  return useQuery({
    queryKey: exchangeKeys.area(accountId ?? "", "status"),
    queryFn: async () =>
      (
        await client.request(
          { type: "GetExchangeStatus", account_id: accountId ?? "" },
          "ExchangeStatus",
        )
      ).status,
    enabled: accountId !== null,
  });
}

export function useMailboxes(accountId: AccountId | null) {
  const client = useClient();
  return useQuery({
    queryKey: exchangeKeys.area(accountId ?? "", "mailboxes"),
    queryFn: async () =>
      (
        await client.request(
          { type: "ListExchangeMailboxes", account_id: accountId ?? "" },
          "ExchangeMailboxes",
        )
      ).mailboxes,
    enabled: accountId !== null,
  });
}

export function useCategories(accountId: AccountId | null) {
  const client = useClient();
  return useQuery({
    queryKey: exchangeKeys.area(accountId ?? "", "categories"),
    queryFn: async () =>
      (
        await client.request(
          { type: "ListExchangeCategories", account_id: accountId ?? "" },
          "ExchangeCategories",
        )
      ).categories,
    enabled: accountId !== null,
  });
}

export function useAutomaticReplies(accountId: AccountId | null) {
  const client = useClient();
  return useQuery({
    queryKey: exchangeKeys.area(accountId ?? "", "automatic-replies"),
    queryFn: async () =>
      (
        await client.request(
          { type: "GetAutomaticReplies", account_id: accountId ?? "" },
          "AutomaticReplies",
        )
      ).replies,
    enabled: accountId !== null,
  });
}

export function useExchangeRules(accountId: AccountId | null) {
  const client = useClient();
  return useQuery({
    queryKey: exchangeKeys.area(accountId ?? "", "rules"),
    queryFn: async () =>
      (
        await client.request(
          { type: "GetExchangeRules", account_id: accountId ?? "" },
          "ExchangeRules",
        )
      ).status,
    enabled: accountId !== null,
  });
}

export function useRoomLists(accountId: AccountId | null) {
  const client = useClient();
  return useQuery({
    queryKey: exchangeKeys.area(accountId ?? "", "rooms"),
    queryFn: async () =>
      (
        await client.request(
          { type: "ListRoomLists", account_id: accountId ?? "", refresh: false },
          "RoomLists",
        )
      ).lists,
    enabled: accountId !== null,
  });
}

/** Rooms with their availability in `[from, to)` (`enabled` = the range is valid). */
export function useRooms(
  accountId: AccountId | null,
  list: string | null,
  from: string,
  to: string,
  minCapacity: number | null,
  enabled: boolean,
) {
  const client = useClient();
  return useQuery({
    queryKey: exchangeKeys.rooms(accountId ?? "", list, from, to, minCapacity),
    queryFn: async () =>
      (
        await client.request(
          {
            type: "FindRooms",
            account_id: accountId ?? "",
            room_list: list,
            from,
            to,
            min_capacity: minCapacity,
            refresh: false,
          },
          "Rooms",
        )
      ).rooms,
    enabled: accountId !== null && enabled,
  });
}

export interface ExchangeWriteContext {
  client: NailClient;
  queryClient: QueryClient;
}

function refresh(ctx: ExchangeWriteContext, accountId: AccountId, area: ExchangeArea): void {
  void ctx.queryClient.invalidateQueries({ queryKey: exchangeKeys.area(accountId, area) });
}

/** Opens another mailbox (`existing` = change what an open one shows). */
export async function saveMailbox(
  ctx: ExchangeWriteContext,
  accountId: AccountId,
  settings: ExchangeMailboxSettings,
  existing: boolean,
): Promise<ExchangeMailbox> {
  const { mailbox } = await ctx.client.request(
    existing
      ? { type: "UpdateExchangeMailbox", account_id: accountId, settings }
      : { type: "AddExchangeMailbox", account_id: accountId, settings },
    "ExchangeMailboxSaved",
  );
  refresh(ctx, accountId, "mailboxes");
  void ctx.queryClient.invalidateQueries({ queryKey: queryKeys.folders(accountId) });
  return mailbox;
}

export async function removeMailbox(
  ctx: ExchangeWriteContext,
  accountId: AccountId,
  address: string,
): Promise<void> {
  await ctx.client.request(
    { type: "RemoveExchangeMailbox", account_id: accountId, address },
    "Done",
  );
  refresh(ctx, accountId, "mailboxes");
  void ctx.queryClient.invalidateQueries({ queryKey: queryKeys.folders(accountId) });
}

/** Creates (`categoryId` `null`) or updates a category. */
export async function saveCategory(
  ctx: ExchangeWriteContext,
  accountId: AccountId,
  categoryId: ExchangeCategoryId | null,
  settings: ExchangeCategorySettings,
): Promise<ExchangeCategory> {
  const { category } = await ctx.client.request(
    categoryId === null
      ? { type: "CreateExchangeCategory", account_id: accountId, settings }
      : {
          type: "UpdateExchangeCategory",
          account_id: accountId,
          category_id: categoryId,
          settings,
        },
    "ExchangeCategorySaved",
  );
  refresh(ctx, accountId, "categories");
  if (categoryId !== null) {
    // A rename renames the tag on messages (mail queue): lists show it.
    void ctx.queryClient.invalidateQueries({ queryKey: queryKeys.allMessages });
    void ctx.queryClient.invalidateQueries({ queryKey: queryKeys.allSearches });
  }
  return category;
}

export async function deleteCategory(
  ctx: ExchangeWriteContext,
  accountId: AccountId,
  categoryId: ExchangeCategoryId,
): Promise<void> {
  await ctx.client.request(
    { type: "DeleteExchangeCategory", account_id: accountId, category_id: categoryId },
    "Queued",
  );
  refresh(ctx, accountId, "categories");
}

export async function setAutomaticReplies(
  ctx: ExchangeWriteContext,
  accountId: AccountId,
  settings: AutomaticRepliesSettings,
): Promise<AutomaticReplies> {
  const { replies } = await ctx.client.request(
    { type: "SetAutomaticReplies", account_id: accountId, settings },
    "AutomaticReplies",
  );
  ctx.queryClient.setQueryData(exchangeKeys.area(accountId, "automatic-replies"), replies);
  refresh(ctx, accountId, "status");
  return replies;
}

export async function syncRules(
  ctx: ExchangeWriteContext,
  accountId: AccountId,
): Promise<ExchangeRulesStatus> {
  const { status } = await ctx.client.request(
    { type: "SyncExchangeRules", account_id: accountId },
    "ExchangeRules",
  );
  ctx.queryClient.setQueryData(exchangeKeys.area(accountId, "rules"), status);
  return status;
}

export async function setServerRuleEnabled(
  ctx: ExchangeWriteContext,
  accountId: AccountId,
  serverRuleId: string,
  enabled: boolean,
): Promise<void> {
  await ctx.client.request(
    {
      type: "SetExchangeInboxRuleEnabled",
      account_id: accountId,
      server_rule_id: serverRuleId,
      enabled,
    },
    "Queued",
  );
  refresh(ctx, accountId, "rules");
}

export async function deleteServerRule(
  ctx: ExchangeWriteContext,
  accountId: AccountId,
  serverRuleId: string,
): Promise<void> {
  await ctx.client.request(
    { type: "DeleteExchangeInboxRule", account_id: accountId, server_rule_id: serverRuleId },
    "Queued",
  );
  refresh(ctx, accountId, "rules");
}

/**
 * Moves messages to Focused / Other (optimistic list patch, then the
 * queued write the status bar counts).
 */
export async function classifyFocused(
  ctx: ExchangeWriteContext & { store: UiStore },
  messageIds: readonly MessageId[],
  classification: InferenceClassification,
  always: boolean,
): Promise<void> {
  if (messageIds.length === 0) return;
  patchSummaries(ctx.queryClient, messageIds, (m) =>
    m.exchange ? { ...m, exchange: { ...m.exchange, inference: classification } } : m,
  );
  await queued(ctx, {
    type: "ClassifyFocused",
    message_ids: [...messageIds],
    classification,
    always,
  });
}

/** Filters the inbox by a Focused Inbox tab (`null` = all of it). */
export function showFocus(
  store: UiStore,
  accountId: AccountId,
  inbox: string,
  focus: InferenceClassification | null,
): void {
  const state = store.getState();
  if (state.activeFolderId !== inbox) state.selectFolder(inbox);
  if (focus === null) {
    state.setSearch(null);
    return;
  }
  const text = focusQuery(focus);
  state.setSearchInput(text);
  state.setSearch({ text, accountId, folderId: inbox, error: null });
}

/** Shows a category's messages across folders (a search on its tag). */
export function showCategory(store: UiStore, accountId: AccountId, name: string): void {
  const text = tagQuery(name);
  const state = store.getState();
  state.setSearchInput(text);
  state.setSearch({ text, accountId, folderId: null, error: null });
}
