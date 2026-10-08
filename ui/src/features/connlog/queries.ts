/**
 * Server data of the connection log feature (TanStack Query through
 * `NailClient`), everything under `["connlog"]`. The core's events keep it
 * fresh (`handleConnlogEvent`): `ConnectionLogChanged` fetches only the
 * entries after the last one held; `SyncErrorsChanged` refetches the
 * account's problems.
 */
import { useQuery, type QueryClient } from "@tanstack/react-query";
import type { NailClient } from "../../api/client";
import type {
  AccountId,
  ConnectionLog,
  ConnectionLogEntry,
  ConnectionLogScope,
  ConnectionLogSettings,
  Event as NailEvent,
} from "../../api/generated";
import { useClient } from "../../state/queries";
import type { FeatureEventContext } from "../types";
import { mergeLog, scopeKey } from "./model";

/** Query keys of this feature. */
export const connlogKeys = {
  all: ["connlog"] as const,
  log: (scope: ConnectionLogScope) => ["connlog", "log", scopeKey(scope)] as const,
  settings: ["connlog", "settings"] as const,
  syncErrors: (accountId: AccountId | null) => ["connlog", "errors", accountId ?? "*"] as const,
  appLog: ["connlog", "application-log"] as const,
};

/** What a viewer holds of a scope's log. */
export interface HeldLog {
  entries: ConnectionLogEntry[];
  lastSeq: number;
  keepOnDisk: boolean;
}

function held(previous: HeldLog | undefined, page: ConnectionLog): HeldLog {
  return {
    entries: mergeLog(previous?.entries ?? [], page),
    lastSeq: page.last_seq,
    keepOnDisk: page.keep_on_disk,
  };
}

async function fetchPage(
  client: NailClient,
  scope: ConnectionLogScope,
  after: number | null,
): Promise<ConnectionLog> {
  return (
    await client.request(
      { type: "GetConnectionLog", scope, ...(after === null ? {} : { after }) },
      "ConnectionLog",
    )
  ).log;
}

/** The entries of a scope (fetched once, then extended by events). */
export function useConnectionLog(scope: ConnectionLogScope) {
  const client = useClient();
  return useQuery({
    queryKey: connlogKeys.log(scope),
    retry: false,
    staleTime: Number.POSITIVE_INFINITY,
    queryFn: async () => held(undefined, await fetchPage(client, scope, null)),
  });
}

/**
 * Fetches what is new in a scope's log the UI holds (nothing when no viewer
 * holds it): the entries after the last one held; entries the core dropped
 * or cleared go.
 */
export async function refreshConnectionLog(
  client: NailClient,
  queryClient: QueryClient,
  scope: ConnectionLogScope,
): Promise<void> {
  const key = connlogKeys.log(scope);
  const current = queryClient.getQueryData<HeldLog>(key);
  if (!current) return;
  const after = current.entries.at(-1)?.seq ?? null;
  const page = await fetchPage(client, scope, after);
  queryClient.setQueryData<HeldLog>(key, (previous) => held(previous, page));
}

/** The connection log settings. */
export function useConnectionLogSettings() {
  const client = useClient();
  return useQuery({
    queryKey: connlogKeys.settings,
    retry: false,
    queryFn: async () =>
      (await client.request({ type: "GetConnectionLogSettings" }, "ConnectionLogSettings"))
        .settings,
  });
}

/** Stores the settings and puts the answer in the cache. */
export async function saveConnectionLogSettings(
  client: NailClient,
  queryClient: QueryClient,
  settings: ConnectionLogSettings,
): Promise<void> {
  const answer = await client.request(
    { type: "SetConnectionLogSettings", settings },
    "ConnectionLogSettings",
  );
  queryClient.setQueryData(connlogKeys.settings, answer.settings);
}

/** The stored last errors and skipped messages of an account. */
export function useSyncErrors(accountId: AccountId) {
  const client = useClient();
  return useQuery({
    queryKey: connlogKeys.syncErrors(accountId),
    retry: false,
    queryFn: async () => {
      const answer = await client.request(
        { type: "ListSyncErrors", account_id: accountId },
        "SyncErrors",
      );
      return { errors: answer.errors, skipped: answer.skipped };
    },
  });
}

/** Where the application log is written and its recent lines. */
export function useAppLog(enabled = true) {
  const client = useClient();
  return useQuery({
    queryKey: connlogKeys.appLog,
    enabled,
    retry: false,
    queryFn: async () => (await client.request({ type: "GetAppLog" }, "AppLog")).log,
  });
}

/** Keeps the feature's data fresh from the core's events. */
export function handleConnlogEvent(event: NailEvent, ctx: FeatureEventContext): void {
  switch (event.type) {
    case "ConnectionLogChanged":
      refreshConnectionLog(ctx.client, ctx.queryClient, event.scope).catch(() => {
        // The next change (or reopening the viewer) fetches again.
      });
      break;
    case "SyncErrorsChanged":
      void ctx.queryClient.invalidateQueries({
        queryKey: connlogKeys.syncErrors(event.account_id),
      });
      void ctx.queryClient.invalidateQueries({ queryKey: connlogKeys.syncErrors(null) });
      break;
  }
}
