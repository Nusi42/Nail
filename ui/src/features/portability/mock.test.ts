import { QueryClient } from "@tanstack/react-query";
import { describe, expect, it, vi } from "vitest";
import { MockTransport } from "../../api/mock";
import { features } from "../registry";
import type { FeatureEventContext } from "../types";
import { PORTABILITY_EVENTS, handlePortabilityEvent, portabilityKeys } from ".";
import { runtimeIds } from "./messages";
import { PORTABILITY_REQUESTS, isPortabilityRequest } from "./mock";

describe("portability slot (M4c contract)", () => {
  it("is registered and routes its requests", () => {
    expect(features.portability).toBeDefined();
    expect(PORTABILITY_REQUESTS).toContain("StartImport");
    expect(isPortabilityRequest({ type: "ListPortabilityJobs" })).toBe(true);
    expect(isPortabilityRequest({ type: "Ping" })).toBe(false);
    expect(runtimeIds()).toContain("m4c-not-ready");
  });

  it("answers like the core at the contract stage", async () => {
    const t = new MockTransport();
    expect(await t.send({ type: "FindImportSources" })).toMatchObject({
      code: "unsupported",
      localized: { id: "m4c-not-ready" },
    });
    expect(
      await t.send({
        type: "PickPath",
        pick: {
          kind: "open-folder",
          title_id: "portability-title",
          filters: [],
          default_path: null,
        },
      }),
    ).toMatchObject({ code: "unsupported", localized: { id: "desktop-unavailable" } });
  });

  it("refreshes the job list on job events", () => {
    const queryClient = new QueryClient();
    const spy = vi.spyOn(queryClient, "invalidateQueries");
    const ctx = { queryClient } as unknown as FeatureEventContext;
    handlePortabilityEvent(
      {
        type: "PortabilityJobProgress",
        job_id: "j1",
        progress: {
          phase: "reading",
          done: 1,
          total: 2,
          bytes_done: 0,
          bytes_total: null,
          current: null,
        },
      },
      ctx,
    );
    expect(spy).toHaveBeenCalledWith({ queryKey: portabilityKeys.jobs });
    expect(PORTABILITY_EVENTS).toEqual(["PortabilityJobProgress", "PortabilityJobFinished"]);
    spy.mockClear();
    handlePortabilityEvent({ type: "AccountsChanged" }, ctx);
    expect(spy).not.toHaveBeenCalled();
  });
});
