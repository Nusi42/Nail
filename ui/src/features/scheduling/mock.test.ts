import { QueryClient } from "@tanstack/react-query";
import { describe, expect, it, vi } from "vitest";
import type { Request } from "../../api/generated";
import { MockTransport } from "../../api/mock";
import type { FeatureEventContext } from "../types";
import { SCHEDULING_EVENTS, handleSchedulingEvent, schedulingKeys } from ".";
import { runtimeIds } from "./messages";
import { SCHEDULING_REQUESTS, isSchedulingRequest } from "./mock";

describe("scheduling mock", () => {
  it("routes exactly its requests", () => {
    expect(new Set(SCHEDULING_REQUESTS).size).toBe(SCHEDULING_REQUESTS.length);
    expect(SCHEDULING_REQUESTS).toHaveLength(12);
    expect(isSchedulingRequest({ type: "GetSchedulingSettings" })).toBe(true);
    expect(isSchedulingRequest({ type: "ListGmailLabels", account_id: "gm" })).toBe(false);
  });

  it("answers like the core at the contract stage", async () => {
    const t = new MockTransport();
    expect(t.data.features.scheduling.invitations).toEqual({});
    expect(t.data.features.scheduling.settings.add_invitations).toBe(true);
    const requests: Request[] = [
      { type: "GetInvitation", message_id: "m1" },
      {
        type: "RespondToInvitation",
        target: { type: "message", message_id: "m1" },
        response: "accept",
        comment: null,
        send: true,
        calendar_id: null,
      },
      {
        type: "QueryFreeBusy",
        attendees: ["bob@example.org"],
        from: "2026-10-05T00:00:00Z",
        to: "2026-10-06T00:00:00Z",
        identity_id: null,
        ignore_event_id: null,
      },
    ];
    for (const request of requests) {
      expect(await t.send(request)).toMatchObject({ type: "Error", code: "unsupported" });
    }
  });

  it("refetches scheduling queries on scheduling and calendar events only", () => {
    const queryClient = new QueryClient();
    const invalidate = vi.spyOn(queryClient, "invalidateQueries");
    const ctx = { queryClient } as unknown as FeatureEventContext;
    handleSchedulingEvent({ type: "InvitationsChanged", message_ids: ["m1"] }, ctx);
    expect(invalidate).toHaveBeenCalledWith({ queryKey: schedulingKeys.all });
    invalidate.mockClear();
    handleSchedulingEvent({ type: "AccountsChanged" }, ctx);
    expect(invalidate).not.toHaveBeenCalled();
    expect(SCHEDULING_EVENTS).toContain("EventsChanged");
  });

  it("lists the core's issue texts", () => {
    expect(runtimeIds()).toContain("itip-issue-sender-not-organizer");
    expect(runtimeIds()).toContain("freebusy-issue-range-too-long");
  });
});
