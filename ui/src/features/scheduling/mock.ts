/**
 * The mock core's answers to the M3b invitation, scheduling and free/busy
 * requests (ADR 0065), used by `MockTransport` (`../../api/mock.ts` routes
 * exactly these requests here).
 *
 * Mirrors the core at the contract stage: every scheduling request answers
 * `unsupported` until the integration (WP85) wires `nail-itip` and CalDAV
 * scheduling. WP84 turns this into a working in-memory scheduler
 * (invitation cards for demo messages, answers, counters, free/busy of
 * demo people) on `data.features.scheduling` for its UI tests.
 */
import type {
  FreeBusyResult,
  Invitation,
  Request,
  Response,
  SchedulingSettings,
} from "../../api/generated";
import { errorResponse, type MockCore } from "../../api/mockMail";

/** The requests this feature answers. */
export const SCHEDULING_REQUESTS = [
  "GetInvitation",
  "RespondToInvitation",
  "ProposeNewTime",
  "RefreshInvitation",
  "ForwardInvitation",
  "ApplyInvitation",
  "SendInvitations",
  "CancelMeeting",
  "RespondToCounter",
  "QueryFreeBusy",
  "GetSchedulingSettings",
  "SetSchedulingSettings",
] as const satisfies readonly Request["type"][];

export type SchedulingRequest = Extract<Request, { type: (typeof SCHEDULING_REQUESTS)[number] }>;

export function isSchedulingRequest(request: Request): request is SchedulingRequest {
  return (SCHEDULING_REQUESTS as readonly string[]).includes(request.type);
}

/** The mock's scheduling data (WP84 fills and uses it). */
export interface SchedulingMockState {
  /** Invitation cards by message id. */
  invitations: Record<string, Invitation>;
  /** Free/busy answers by attendee address. */
  freeBusy: Record<string, FreeBusyResult["attendees"][number]>;
  settings: SchedulingSettings;
}

export function createSchedulingMockState(): SchedulingMockState {
  return {
    invitations: {},
    freeBusy: {},
    settings: {
      add_invitations: true,
      apply_updates: true,
      apply_replies: true,
      apply_cancellations: false,
      calendar_id: null,
      confirm_replies: false,
    },
  };
}

/** Answers like the core at the contract stage (`unsupported`, ADR 0065). */
export function answerScheduling(_core: MockCore, request: SchedulingRequest): Response {
  return errorResponse("unsupported", `${request.type} is not wired yet (M3b WP85)`);
}
