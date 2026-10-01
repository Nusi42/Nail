/**
 * Data of the scheduling UI, always through `NailClient`: invitation cards,
 * free/busy, scheduling settings and the user's own addresses; writes
 * (`RespondToInvitation`, `CancelMeeting`, …) go to the core, which queues
 * the calendar changes in the PIM queue and the iMIP mail in the outbox.
 */
import { useQuery, type QueryClient } from "@tanstack/react-query";
import type { NailClient } from "../../api/client";
import type {
  EventId,
  IdentityId,
  Invitation,
  MessageId,
  Request,
  SchedulingResult,
} from "../../api/generated";
import { queryKeys, useAccounts, useClient, useIdentities } from "../../state/queries";
import { pimKeys } from "../pim/queries";

/** Query keys of this feature. */
export const schedulingKeys = {
  all: ["scheduling"] as const,
  invitation: (messageId: MessageId | null) => ["scheduling", "invitation", messageId] as const,
  freeBusy: (
    attendees: readonly string[],
    from: string,
    to: string,
    identityId: IdentityId | null,
    ignore: EventId | null,
  ) => ["scheduling", "free-busy", attendees.join(","), from, to, identityId, ignore] as const,
  settings: ["scheduling", "settings"] as const,
};

/** The invitation card of a message (`null` = it carries none). */
export function useInvitation(messageId: MessageId | null, initial?: Invitation | null) {
  const client = useClient();
  return useQuery({
    queryKey: schedulingKeys.invitation(messageId),
    queryFn: async () =>
      (await client.request({ type: "GetInvitation", message_id: messageId ?? "" }, "Invitation"))
        .invitation,
    enabled: messageId !== null,
    ...(initial !== undefined && initial !== null ? { initialData: initial } : {}),
  });
}

/** Free/busy of `attendees` in `[from, to)`. */
export function useFreeBusy(
  attendees: readonly string[],
  from: string,
  to: string,
  identityId: IdentityId | null,
  ignoreEventId: EventId | null,
) {
  const client = useClient();
  return useQuery({
    queryKey: schedulingKeys.freeBusy(attendees, from, to, identityId, ignoreEventId),
    queryFn: async () =>
      (
        await client.request(
          {
            type: "QueryFreeBusy",
            attendees: [...attendees],
            from,
            to,
            identity_id: identityId,
            ignore_event_id: ignoreEventId,
          },
          "FreeBusy",
        )
      ).result,
    enabled: attendees.length > 0,
  });
}

export function useSchedulingSettings() {
  const client = useClient();
  return useQuery({
    queryKey: schedulingKeys.settings,
    queryFn: async () =>
      (await client.request({ type: "GetSchedulingSettings" }, "SchedulingSettings")).settings,
  });
}

/** The user's addresses (every identity and account address), lower case. */
export function useMyAddresses(): string[] {
  const identities = useIdentities(null);
  const accounts = useAccounts();
  const out = new Set<string>();
  for (const identity of identities.data ?? []) out.add(identity.email.toLowerCase());
  for (const account of accounts.data ?? []) {
    if (account.email) out.add(account.email.toLowerCase());
  }
  return [...out];
}

/** What a scheduling write needs. */
export interface ScheduleContext {
  client: NailClient;
  queryClient: QueryClient;
}

/** Requests answered with `Scheduled`. */
export type SchedulingWrite = Extract<
  Request,
  {
    type:
      | "RespondToInvitation"
      | "ProposeNewTime"
      | "RefreshInvitation"
      | "ForwardInvitation"
      | "ApplyInvitation"
      | "SendInvitations"
      | "CancelMeeting"
      | "RespondToCounter";
  }
>;

/** The message a write is about, if any. */
function messageOf(request: SchedulingWrite): MessageId | null {
  if ("target" in request) {
    return request.target.type === "message" ? request.target.message_id : null;
  }
  return "message_id" in request ? request.message_id : null;
}

/**
 * Sends a scheduling write and refreshes what it changed: the card (from
 * the answer), the calendars, the outbox and the message.
 */
export async function scheduleWrite(
  ctx: ScheduleContext,
  request: SchedulingWrite,
): Promise<SchedulingResult> {
  const { result } = await ctx.client.request(request, "Scheduled");
  const messageId = messageOf(request);
  if (result.invitation) {
    ctx.queryClient.setQueryData(
      schedulingKeys.invitation(result.invitation.message_id),
      result.invitation,
    );
  }
  void ctx.queryClient.invalidateQueries({ queryKey: schedulingKeys.all });
  void ctx.queryClient.invalidateQueries({ queryKey: pimKeys.all });
  if (result.sent.some((s) => s.outbox_id !== null)) {
    void ctx.queryClient.invalidateQueries({ queryKey: queryKeys.outbox });
  }
  if (messageId !== null) {
    void ctx.queryClient.invalidateQueries({ queryKey: queryKeys.message(messageId) });
  }
  return result;
}
