/**
 * Badge numbers of the navigation rail (M5b WP161): `unread` = unread mail
 * in the inboxes (the "All accounts" inbox, which the core sums over every
 * account), `confirmations` = writes of agents and plug-ins waiting for the
 * user (the confirmation center's query, refreshed by its events).
 * `MailChanged` / `NewMail` refresh the folders, so the unread badge
 * follows without polling.
 */
import type { BadgeSource } from "../../api/generated";
import { usePendingConfirmations } from "../agents/queries";
import { useFolders } from "../../state/queries";
import { inboxUnread } from "./model";
import { ALL_ACCOUNTS } from "../../api/contract";

export type BadgeCounts = Readonly<Record<BadgeSource, number>>;

/** The current number of every badge source the rail uses (0 = no badge). */
export function useBadgeCounts(sources: ReadonlySet<BadgeSource>): BadgeCounts {
  const folders = useFolders(sources.has("unread") ? ALL_ACCOUNTS : null);
  const pending = usePendingConfirmations();
  return {
    unread: sources.has("unread") ? inboxUnread(folders.data) : 0,
    confirmations: sources.has("confirmations") ? (pending.data?.length ?? 0) : 0,
  };
}
