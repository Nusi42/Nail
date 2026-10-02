/**
 * Server data of the confirmation center (TanStack Query through
 * `NailClient`). Everything lives under `["agents"]`.
 */
import { useInfiniteQuery, useQuery } from "@tanstack/react-query";
import type { ActionActor, Confirmation, ConfirmationId } from "../../api/generated";
import { useClient } from "../../state/queries";
import { actorKey } from "./model";

/** History entries per page. */
export const HISTORY_PAGE = 50;
/** Pending confirmations loaded at once (the core keeps at most 20 per actor). */
export const PENDING_LIMIT = 500;

/** Query keys of this feature. */
export const agentsKeys = {
  all: ["agents"] as const,
  confirmations: ["agents", "confirmations"] as const,
  pending: ["agents", "confirmations", "pending"] as const,
  history: ["agents", "confirmations", "history"] as const,
  policies: (actor: ActionActor) => ["agents", "policies", actorKey(actor)] as const,
};

/** The confirmations waiting for the user, oldest first. */
export function usePendingConfirmations() {
  const client = useClient();
  return useQuery({
    queryKey: agentsKeys.pending,
    queryFn: async (): Promise<Confirmation[]> =>
      (
        await client.request(
          {
            type: "ListConfirmations",
            filter: "pending",
            actor: null,
            before: null,
            limit: PENDING_LIMIT,
          },
          "Confirmations",
        )
      ).confirmations,
  });
}

/** Decided confirmations, newest first, paged with `before`. */
export function useConfirmationHistory(enabled = true) {
  const client = useClient();
  return useInfiniteQuery({
    queryKey: agentsKeys.history,
    enabled,
    initialPageParam: null as ConfirmationId | null,
    queryFn: async ({ pageParam }): Promise<Confirmation[]> =>
      (
        await client.request(
          {
            type: "ListConfirmations",
            filter: "decided",
            actor: null,
            before: pageParam,
            limit: HISTORY_PAGE,
          },
          "Confirmations",
        )
      ).confirmations,
    getNextPageParam: (last) =>
      last.length < HISTORY_PAGE ? undefined : (last[last.length - 1]?.id ?? undefined),
  });
}

/** One actor's policy table (`ActionKind::ALL` order). */
export function useActionPolicies(actor: ActionActor) {
  const client = useClient();
  return useQuery({
    queryKey: agentsKeys.policies(actor),
    queryFn: async () =>
      (await client.request({ type: "ListActionPolicies", actor }, "ActionPolicies")).policies,
  });
}

/** The pending confirmations of one actor (from the shared pending list). */
export function pendingOf(
  confirmations: readonly Confirmation[] | undefined,
  actor: ActionActor,
): Confirmation[] {
  const key = actorKey(actor);
  return (confirmations ?? []).filter((c) => actorKey(c.requested_by.actor) === key);
}
