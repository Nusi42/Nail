import { QueryClient } from "@tanstack/react-query";
import { describe, expect, it, vi } from "vitest";
import type { Request } from "../../api/generated";
import { MockTransport } from "../../api/mock";
import type { FeatureEventContext } from "../types";
import { PIM_EVENTS, handlePimEvent, pimKeys } from ".";
import { PIM_REQUESTS, isPimRequest } from "./mock";

describe("pim mock", () => {
  it("routes exactly its requests", () => {
    expect(new Set(PIM_REQUESTS).size).toBe(PIM_REQUESTS.length);
    expect(PIM_REQUESTS).toHaveLength(52);
    expect(isPimRequest({ type: "ListPimSources" })).toBe(true);
    expect(isPimRequest({ type: "Ping" })).toBe(false);
  });

  it("answers like the core at the contract stage", async () => {
    const t = new MockTransport();
    expect(t.data.features.pim.calendars).toEqual([]);
    const requests: Request[] = [
      { type: "ListPimSources" },
      {
        type: "ListEventInstances",
        from: "2026-10-01T00:00:00Z",
        to: "2026-10-08T00:00:00Z",
        calendar_ids: null,
        tz: "Europe/Berlin",
      },
      { type: "DismissReminders", reminder_ids: ["r1"] },
    ];
    for (const request of requests) {
      expect(await t.send(request)).toMatchObject({ type: "Error", code: "unsupported" });
    }
  });

  it("refetches PIM queries on PIM events only", () => {
    const queryClient = new QueryClient();
    const invalidate = vi.spyOn(queryClient, "invalidateQueries");
    const ctx = { queryClient } as unknown as FeatureEventContext;
    handlePimEvent({ type: "EventsChanged", calendar_ids: ["c1"] }, ctx);
    expect(invalidate).toHaveBeenCalledWith({ queryKey: pimKeys.all });
    invalidate.mockClear();
    handlePimEvent({ type: "AccountsChanged" }, ctx);
    expect(invalidate).not.toHaveBeenCalled();
    expect(PIM_EVENTS).toContain("ReminderDue");
  });
});
