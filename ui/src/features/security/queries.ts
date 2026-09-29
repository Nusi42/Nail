/**
 * Server data of the security UI (TanStack Query through `NailClient`).
 * Everything lives under the `security` key, so `KeysChanged` and the
 * other events refresh it with one invalidation (see `index.ts`).
 */
import { useQuery, type QueryClient } from "@tanstack/react-query";
import { useEffect, useState } from "react";
import type {
  IdentityId,
  KeyFilter,
  KeyId,
  VaultStatus,
  WindowsCertStore,
} from "../../api/generated";
import { useClient } from "../../state/queries";
import { distinctRecipients } from "./model";

export const securityKeys = {
  all: ["security"] as const,
  keys: ["security", "keys"] as const,
  keyList: (filter: KeyFilter) =>
    ["security", "keys", "list", filter.technology, filter.text, filter.own_only] as const,
  key: (id: KeyId | null) => ["security", "keys", "one", id] as const,
  recipients: ["security", "recipients"] as const,
  recipientCheck: (identity: IdentityId | null, recipients: readonly string[], lookup: boolean) =>
    ["security", "recipients", identity, recipients.join(","), lookup] as const,
  settings: ["security", "settings"] as const,
  vault: ["security", "vault"] as const,
  windows: (store: WindowsCertStore) => ["security", "windows", store] as const,
};

/** Keyring entries matching a filter (own keys first). */
export function useKeys(filter: KeyFilter) {
  const client = useClient();
  return useQuery({
    queryKey: securityKeys.keyList(filter),
    queryFn: async () => (await client.request({ type: "ListKeys", filter }, "Keys")).keys,
  });
}

/** All own keys (for identity settings and Autocrypt). */
export function useOwnKeys() {
  return useKeys({ technology: null, text: null, own_only: true });
}

export function useSecuritySettings() {
  const client = useClient();
  return useQuery({
    queryKey: securityKeys.settings,
    queryFn: async () =>
      (await client.request({ type: "GetSecuritySettings" }, "SecuritySettings")).settings,
  });
}

export function useVaultStatus() {
  const client = useClient();
  return useQuery({
    queryKey: securityKeys.vault,
    queryFn: async () => (await client.request({ type: "GetVaultStatus" }, "VaultStatus")).status,
  });
}

/** Puts a status from a `VaultProgress` event into the cache. */
export function applyVaultStatus(queryClient: QueryClient, status: VaultStatus): void {
  queryClient.setQueryData(securityKeys.vault, status);
}

export function useWindowsCertificates(store: WindowsCertStore, enabled: boolean) {
  const client = useClient();
  return useQuery({
    queryKey: securityKeys.windows(store),
    queryFn: async () =>
      (await client.request({ type: "ListWindowsCertificates", store }, "WindowsCertificates"))
        .certificates,
    enabled,
  });
}

/** Delay after the last recipient change before the key status is checked. */
export const RECIPIENT_CHECK_MS = 300;

/** `value`, but only after it stopped changing for `ms`. */
export function useDebounced<T>(value: T, ms: number, same: (a: T, b: T) => boolean): T {
  const [settled, setSettled] = useState(value);
  useEffect(() => {
    if (same(settled, value)) return;
    const timer = setTimeout(() => setSettled(value), ms);
    return () => clearTimeout(timer);
  }, [value, settled, ms, same]);
  return settled;
}

const sameList = (a: readonly string[], b: readonly string[]) =>
  a.length === b.length && a.every((x, i) => x === b[i]);

/**
 * Live recipient key status while composing (`CheckRecipientKeys`),
 * debounced by `RECIPIENT_CHECK_MS`. `lookup` asks the core to search
 * online for missing keys (the settings' lookup policy still applies).
 */
export function useRecipientKeys(
  identityId: IdentityId | null,
  recipients: readonly string[],
  lookup: boolean,
) {
  const client = useClient();
  const list = distinctRecipients(recipients);
  const settled = useDebounced(list, RECIPIENT_CHECK_MS, sameList);
  const query = useQuery({
    queryKey: securityKeys.recipientCheck(identityId, settled, lookup),
    queryFn: async () =>
      (
        await client.request(
          {
            type: "CheckRecipientKeys",
            identity_id: identityId ?? "",
            recipients: settled,
            lookup,
          },
          "RecipientKeys",
        )
      ).report,
    enabled: identityId !== null,
    placeholderData: (previous) => previous,
  });
  return { ...query, pending: !sameList(list, settled) };
}
