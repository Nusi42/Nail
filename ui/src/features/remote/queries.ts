/**
 * Server data of the remote-UI settings (TanStack Query through
 * `NailClient`). Everything lives under the `remote` key, so the events
 * refresh it with one invalidation (see `index.ts`).
 */
import { useInfiniteQuery, useQuery, type QueryClient } from "@tanstack/react-query";
import type {
  RemoteAuditEntry,
  RemoteUiSettings,
  RemoteUiStatus,
  RemoteUserId,
} from "../../api/generated";
import { useClient } from "../../state/queries";
import { AUDIT_PAGE } from "../../state/queries";

/** Audit entries per page. */

export const remoteKeys = {
  all: ["remote"] as const,
  ui: ["remote", "ui"] as const,
  access: ["remote", "access"] as const,
  users: ["remote", "access", "users"] as const,
  devices: ["remote", "access", "devices"] as const,
  sessions: (includeEnded: boolean) => ["remote", "access", "sessions", includeEnded] as const,
  pairing: ["remote", "access", "pairing"] as const,
  audit: (userId: RemoteUserId | null) => ["remote", "audit", userId] as const,
  allAudit: ["remote", "audit"] as const,
  allows: ["remote", "allows"] as const,
};

export interface RemoteUi {
  settings: RemoteUiSettings;
  status: RemoteUiStatus;
}

export function useRemoteUi() {
  const client = useClient();
  return useQuery({
    queryKey: remoteKeys.ui,
    queryFn: async (): Promise<RemoteUi> => {
      const { settings, status } = await client.request(
        { type: "GetRemoteUiSettings" },
        "RemoteUi",
      );
      return { settings, status };
    },
  });
}

/** Puts a pushed status into the cache. */
export function applyRemoteStatus(queryClient: QueryClient, status: RemoteUiStatus): void {
  queryClient.setQueryData<RemoteUi>(remoteKeys.ui, (old) =>
    old === undefined ? old : { ...old, status },
  );
}

export function useRemoteUsers() {
  const client = useClient();
  return useQuery({
    queryKey: remoteKeys.users,
    queryFn: async () => (await client.request({ type: "ListRemoteUsers" }, "RemoteUsers")).users,
  });
}

export function useRemoteDevices() {
  const client = useClient();
  return useQuery({
    queryKey: remoteKeys.devices,
    queryFn: async () =>
      (await client.request({ type: "ListRemoteDevices", user_id: null }, "RemoteDevices")).devices,
  });
}

export function useRemoteSessions(includeEnded: boolean) {
  const client = useClient();
  return useQuery({
    queryKey: remoteKeys.sessions(includeEnded),
    queryFn: async () =>
      (
        await client.request(
          {
            type: "ListRemoteSessions",
            filter: { user_id: null, device_id: null, include_ended: includeEnded },
          },
          "RemoteSessions",
        )
      ).sessions,
  });
}

export function usePairingCodes() {
  const client = useClient();
  return useQuery({
    queryKey: remoteKeys.pairing,
    queryFn: async () => (await client.request({ type: "ListPairingCodes" }, "PairingCodes")).codes,
  });
}

export function useRemoteAudit(userId: RemoteUserId | null) {
  const client = useClient();
  return useInfiniteQuery({
    queryKey: remoteKeys.audit(userId),
    initialPageParam: null as number | null,
    queryFn: async ({ pageParam }): Promise<RemoteAuditEntry[]> =>
      (
        await client.request(
          { type: "ListRemoteAudit", user_id: userId, before: pageParam, limit: AUDIT_PAGE },
          "RemoteAudit",
        )
      ).entries,
    getNextPageParam: (last) =>
      last.length < AUDIT_PAGE ? undefined : (last.at(-1)?.id ?? undefined),
  });
}

export function useRemoteContentAllows() {
  const client = useClient();
  return useQuery({
    queryKey: remoteKeys.allows,
    queryFn: async () =>
      (await client.request({ type: "ListRemoteContentAllows" }, "RemoteContentAllows")).entries,
  });
}
