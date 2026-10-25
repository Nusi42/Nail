/**
 * Server data via TanStack Query. Every query goes through the `NailClient`.
 */
import { createContext, useContext } from "react";
import { useInfiniteQuery, useQuery } from "@tanstack/react-query";
import { NailError, type NailClient } from "../api/client";
import type {
  AccountId,
  AccountStatus,
  FolderId,
  MessageId,
  PlatformInfo,
  ResolvedLayout,
  ResolvedTheme,
  SearchError,
  SearchHit,
  ThreadId,
} from "../api/generated";
import { normalizeLayout, normalizeTheme } from "../api/normalize";

export const ClientContext = createContext<NailClient | null>(null);

/** The app's `NailClient`. */
export function useClient(): NailClient {
  const client = useContext(ClientContext);
  if (!client) {
    throw new Error("useClient() must be used inside <ClientContext.Provider>");
  }
  return client;
}

/** Query keys, shared with event-driven invalidation. */
export const queryKeys = {
  config: ["config"] as const,
  accounts: ["accounts"] as const,
  folders: (accountId: AccountId | null) => ["folders", accountId] as const,
  allFolders: ["folders"] as const,
  messages: (folderId: FolderId | null, pageSize: number) =>
    ["messages", folderId, pageSize] as const,
  allMessages: ["messages"] as const,
  message: (id: MessageId | null) => ["message", id] as const,
  accountsConfig: ["accounts-config"] as const,
  identities: (accountId: AccountId | null) => ["identities", accountId] as const,
  /** Under "identities", so account changes refresh it too. */
  chosenIdentity: (
    accountId: AccountId | null,
    recipients: readonly string[],
    replyTo: MessageId | null,
  ) => ["identities", "chosen", accountId, recipients.join(","), replyTo] as const,
  allIdentities: ["identities"] as const,
  virtualAccounts: ["virtual-accounts"] as const,
  accountStatuses: ["account-statuses"] as const,
  threads: (folderId: FolderId | null, pageSize: number) =>
    ["threads", folderId, pageSize] as const,
  allThreads: ["threads"] as const,
  thread: (id: ThreadId | null) => ["thread", id] as const,
  /** P2: under the thread's key, so invalidating the thread refetches it. */
  conversation: (id: ThreadId | null, folderId: FolderId | null) =>
    ["thread", id, "conversation", folderId] as const,
  allThreadDetails: ["thread"] as const,
  search: (text: string, accountId: AccountId | null, folderId: FolderId | null, size: number) =>
    ["search", text, accountId, folderId, size] as const,
  allSearches: ["search"] as const,
  checkQuery: (text: string) => ["check-query", text] as const,
  outbox: ["outbox"] as const,
  drafts: ["drafts"] as const,
};

/** Audit entries per page (the MCP and remote-access audit logs). */
export const AUDIT_PAGE = 50;

/** Whether an error means "the core does not implement this yet". */
export function isUnsupported(error: unknown): boolean {
  return error instanceof NailError && error.code === "unsupported";
}

export interface AppConfig {
  theme: ResolvedTheme;
  layout: ResolvedLayout;
  locale: string | null;
  /** The system the core runs on and what it offers there (ADR 0202). */
  platform: PlatformInfo;
}

export function useConfig() {
  const client = useClient();
  return useQuery({
    queryKey: queryKeys.config,
    queryFn: async (): Promise<AppConfig> => {
      const r = await client.request({ type: "GetConfig" }, "Config");
      return {
        theme: normalizeTheme(r.theme),
        layout: normalizeLayout(r.layout),
        locale: r.locale,
        platform: r.platform,
      };
    },
    staleTime: Number.POSITIVE_INFINITY,
  });
}

export function useAccounts() {
  const client = useClient();
  return useQuery({
    queryKey: queryKeys.accounts,
    queryFn: async () => (await client.request({ type: "ListAccounts" }, "Accounts")).accounts,
  });
}

export function useFolders(accountId: AccountId | null) {
  const client = useClient();
  return useQuery({
    queryKey: queryKeys.folders(accountId),
    queryFn: async () =>
      (await client.request({ type: "ListFolders", account_id: accountId ?? "" }, "Folders"))
        .folders,
    enabled: accountId !== null,
  });
}

export function useMessages(folderId: FolderId | null, pageSize: number) {
  const client = useClient();
  return useInfiniteQuery({
    queryKey: queryKeys.messages(folderId, pageSize),
    queryFn: ({ pageParam }) =>
      client.request(
        { type: "ListMessages", folder_id: folderId ?? "", offset: pageParam, limit: pageSize },
        "Messages",
      ),
    initialPageParam: 0,
    getNextPageParam: (last) => {
      const next = last.offset + last.messages.length;
      return next < last.total && last.messages.length > 0 ? next : undefined;
    },
    enabled: folderId !== null,
  });
}

export function useMessage(id: MessageId | null) {
  const client = useClient();
  return useQuery({
    queryKey: queryKeys.message(id),
    queryFn: async () =>
      (await client.request({ type: "GetMessage", id: id ?? "" }, "Message")).message,
    enabled: id !== null,
  });
}

export function useAccountsConfig() {
  const client = useClient();
  return useQuery({
    queryKey: queryKeys.accountsConfig,
    queryFn: async () =>
      (await client.request({ type: "GetAccountsConfig" }, "AccountsConfig")).config,
  });
}

/** Identities of an account (real or virtual), or of all accounts for `null`. */
export function useIdentities(accountId: AccountId | null, enabled = true) {
  const client = useClient();
  return useQuery({
    queryKey: queryKeys.identities(accountId),
    queryFn: async () =>
      (await client.request({ type: "ListIdentities", account_id: accountId }, "Identities"))
        .identities,
    enabled,
  });
}

/**
 * The identity the core would send from (virtual-account rules, the
 * identity a reply goes out from, the default).
 */
export function useChosenIdentity(
  accountId: AccountId | null,
  recipients: readonly string[],
  replyTo: MessageId | null,
  enabled = true,
) {
  const client = useClient();
  return useQuery({
    queryKey: queryKeys.chosenIdentity(accountId, recipients, replyTo),
    queryFn: async () =>
      (
        await client.request(
          {
            type: "ChooseIdentity",
            account_id: accountId,
            recipients: [...recipients],
            reply_to: replyTo,
          },
          "IdentityChosen",
        )
      ).identity_id,
    enabled,
  });
}

export function useVirtualAccounts() {
  const client = useClient();
  return useQuery({
    queryKey: queryKeys.virtualAccounts,
    queryFn: async () =>
      (await client.request({ type: "ListVirtualAccounts" }, "VirtualAccounts")).virtual_accounts,
  });
}

/**
 * Connection states by account id. A core without status support answers
 * `unsupported`; that is an empty map, not an error.
 */
export function useAccountStatuses() {
  const client = useClient();
  return useQuery({
    queryKey: queryKeys.accountStatuses,
    queryFn: async (): Promise<AccountStatus[]> => {
      try {
        return (await client.request({ type: "ListAccountStatuses" }, "AccountStatuses")).statuses;
      } catch (error) {
        if (isUnsupported(error)) return [];
        throw error;
      }
    },
    staleTime: Number.POSITIVE_INFINITY,
  });
}

export function useThreads(folderId: FolderId | null, pageSize: number, enabled = true) {
  const client = useClient();
  return useInfiniteQuery({
    queryKey: queryKeys.threads(folderId, pageSize),
    queryFn: ({ pageParam }) =>
      client.request(
        { type: "ListThreads", folder_id: folderId ?? "", offset: pageParam, limit: pageSize },
        "Threads",
      ),
    initialPageParam: 0,
    getNextPageParam: (last) => {
      const next = last.offset + last.threads.length;
      return next < last.total && last.threads.length > 0 ? next : undefined;
    },
    enabled: enabled && folderId !== null,
  });
}

export function useThread(id: ThreadId | null) {
  const client = useClient();
  return useQuery({
    queryKey: queryKeys.thread(id),
    queryFn: () => client.request({ type: "GetThread", thread_id: id ?? "" }, "Thread"),
    enabled: id !== null,
  });
}

/**
 * The conversation of a thread as the list shows it in `folderId` (P2, ADR
 * 0148): oldest first, the user's replies from Sent marked `from_me`.
 * Without a folder, every folder but Drafts, Trash and Junk.
 */
export function useConversation(id: ThreadId | null, folderId: FolderId | null) {
  const client = useClient();
  return useQuery({
    queryKey: queryKeys.conversation(id, folderId),
    queryFn: () =>
      client.request(
        { type: "GetConversation", thread_id: id ?? "", folder_id: folderId },
        "Conversation",
      ),
    enabled: id !== null,
  });
}

export interface SearchScope {
  text: string;
  accountId: AccountId | null;
  folderId: FolderId | null;
}

/** Search hits page by page; an invalid query resolves to its error. */
/** One loaded page of search results. */
export interface SearchPage {
  offset: number;
  total: number;
  hits: SearchHit[];
  error: SearchError | null;
}

/**
 * The offset of the next search page, `undefined` at the end. Hits found
 * only by the server search (`from_server`, first page) come on top of the
 * page: the offset and the total count local hits only.
 */
export function nextSearchOffset(last: SearchPage): number | undefined {
  const local = last.hits.filter((h) => h.from_server !== true).length;
  const serverOnly = last.hits.length - local;
  const next = last.offset + local;
  return last.error === null && local > 0 && next < last.total - serverOnly ? next : undefined;
}

export function useSearch(scope: SearchScope | null, pageSize: number) {
  const client = useClient();
  return useInfiniteQuery({
    queryKey: queryKeys.search(
      scope?.text ?? "",
      scope?.accountId ?? null,
      scope?.folderId ?? null,
      pageSize,
    ),
    queryFn: async ({ pageParam }) => {
      const response = await client.call({
        type: "Search",
        query: {
          text: scope?.text ?? "",
          account_id: scope?.accountId ?? null,
          folder_id: scope?.folderId ?? null,
          offset: pageParam,
          limit: pageSize,
          sort: "date",
        },
      });
      if (response.type === "Error") throw new NailError(response.code, response.message);
      if (response.type === "InvalidQuery") {
        return { offset: pageParam, total: 0, hits: [], error: response.error };
      }
      if (response.type !== "SearchResults") {
        throw new NailError("internal", `unexpected ${response.type} to Search`);
      }
      return { offset: response.offset, total: response.total, hits: response.hits, error: null };
    },
    initialPageParam: 0,
    getNextPageParam: nextSearchOffset,
    enabled: scope !== null && scope.text.trim() !== "",
  });
}

/** Live syntax check of a query (`null` = valid or not checked). */
export function useCheckQuery(text: string) {
  const client = useClient();
  return useQuery({
    queryKey: queryKeys.checkQuery(text),
    queryFn: async () => {
      try {
        return (await client.request({ type: "CheckQuery", text }, "QueryChecked")).error;
      } catch (error) {
        if (isUnsupported(error)) return null;
        throw error;
      }
    },
    enabled: text.trim() !== "",
    staleTime: Number.POSITIVE_INFINITY,
  });
}

/** Saved drafts (not in the outbox), fetched afresh whenever shown. */
export function useDrafts() {
  const client = useClient();
  return useQuery({
    queryKey: queryKeys.drafts,
    queryFn: async () => (await client.request({ type: "ListDrafts" }, "Drafts")).drafts,
    staleTime: 0,
  });
}

export function useOutbox(enabled = true) {
  const client = useClient();
  return useQuery({
    queryKey: queryKeys.outbox,
    queryFn: async () => (await client.request({ type: "ListOutbox" }, "Outbox")).entries,
    enabled,
  });
}
