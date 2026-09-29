/**
 * Server data via TanStack Query. Every query goes through the `NailClient`.
 */
import { createContext, useContext } from "react";
import { useInfiniteQuery, useQuery } from "@tanstack/react-query";
import type { NailClient } from "../api/client";
import type {
  AccountId,
  FolderId,
  MessageId,
  ResolvedLayout,
  ResolvedTheme,
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
};

export interface AppConfig {
  theme: ResolvedTheme;
  layout: ResolvedLayout;
  locale: string | null;
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
