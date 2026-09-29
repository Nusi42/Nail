/**
 * The mock core's answers to the M2c sync-policy and body requests
 * (ADR 0043), used by `MockTransport` (`../../api/mock.ts` routes exactly
 * these requests here). Stateless: the policy lives in the mock's
 * `accountsConfig`, as the core's lives in `accounts.kdl`.
 *
 * Every mock message has its body, so `FetchMessageBodies` reports known
 * messages as fetched and unknown ones as failed (like the core before
 * on-demand fetching is wired).
 */
import type { Request, Response } from "../../api/generated";
import { errorResponse, type MockCore } from "../../api/mockMail";
import { resolvePolicy, withFolderRule } from "./model";

/** The requests this feature answers. */
export const SYNC_REQUESTS = [
  "GetFolderSyncPolicy",
  "SetFolderSyncPolicy",
  "FetchMessageBodies",
] as const satisfies readonly Request["type"][];

export type SyncRequest = Extract<Request, { type: (typeof SYNC_REQUESTS)[number] }>;

export function isSyncRequest(request: Request): request is SyncRequest {
  return (SYNC_REQUESTS as readonly string[]).includes(request.type);
}

export function answerSync(core: MockCore, request: SyncRequest): Response {
  const d = core.data;
  switch (request.type) {
    case "GetFolderSyncPolicy":
    case "SetFolderSyncPolicy": {
      const folder = d.folders.find((f) => f.id === request.folder_id);
      if (!folder) return errorResponse("not_found", `folder ${request.folder_id}`);
      const index = d.accountsConfig.accounts.findIndex((a) => a.id === folder.account_id);
      const account = d.accountsConfig.accounts[index];
      if (!account) {
        return errorResponse("unsupported", `folder ${folder.id} has no configured account`);
      }
      if (request.type === "GetFolderSyncPolicy") {
        return { type: "FolderSyncPolicy", policy: resolvePolicy(account, folder) };
      }
      if (request.bodies?.mode === "recent" && request.bodies.days === 0) {
        return errorResponse("invalid_request", "`recent` needs at least 1 day");
      }
      const updated = withFolderRule(
        account,
        folder.path,
        request.bodies,
        request.enabled,
        request.idle,
      );
      d.accountsConfig.accounts[index] = updated;
      core.emit({ type: "AccountsChanged" });
      return { type: "FolderSyncPolicy", policy: resolvePolicy(updated, folder) };
    }
    case "FetchMessageBodies": {
      const known = new Set(d.messages.map((m) => m.id));
      return {
        type: "BodiesFetched",
        fetched: request.message_ids.filter((id) => known.has(id)),
        failed: request.message_ids.filter((id) => !known.has(id)),
      };
    }
  }
}
