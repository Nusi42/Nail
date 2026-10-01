import { QueryClient } from "@tanstack/react-query";
import { describe, expect, it, vi } from "vitest";
import { MockTransport } from "../../api/mock";
import type { FeatureEventContext } from "../types";
import { GMAIL_EVENTS, gmailKeys, handleGmailEvent } from ".";
import { runtimeIds } from "./messages";
import { GMAIL_REQUESTS, isGmailRequest } from "./mock";

describe("gmail mock", () => {
  it("routes exactly its requests", () => {
    expect(new Set(GMAIL_REQUESTS).size).toBe(GMAIL_REQUESTS.length);
    expect(GMAIL_REQUESTS).toHaveLength(4);
    expect(isGmailRequest({ type: "ListGmailLabels", account_id: "gm" })).toBe(true);
    expect(isGmailRequest({ type: "GetSchedulingSettings" })).toBe(false);
  });

  it("answers like the core at the contract stage", async () => {
    const t = new MockTransport();
    expect(t.data.features.gmail.labels).toEqual({});
    expect(await t.send({ type: "ListGmailLabels", account_id: "gm" })).toMatchObject({
      type: "Error",
      code: "unsupported",
    });
    expect(
      await t.send({
        type: "CreateGmailLabel",
        account_id: "gm",
        settings: {
          name: "Work",
          color: null,
          list_visibility: "show",
          message_visibility: "show",
        },
      }),
    ).toMatchObject({ type: "Error", code: "unsupported" });
  });

  it("refetches one account's labels on label events, everything on mail events", () => {
    const queryClient = new QueryClient();
    const invalidate = vi.spyOn(queryClient, "invalidateQueries");
    const ctx = { queryClient } as unknown as FeatureEventContext;
    handleGmailEvent({ type: "GmailLabelsChanged", account_id: "gm" }, ctx);
    expect(invalidate).toHaveBeenCalledWith({ queryKey: gmailKeys.labels("gm") });
    invalidate.mockClear();
    handleGmailEvent({ type: "MailChanged", folder_ids: ["f1"] }, ctx);
    expect(invalidate).toHaveBeenCalledWith({ queryKey: gmailKeys.all });
    invalidate.mockClear();
    handleGmailEvent({ type: "PimSourcesChanged" }, ctx);
    expect(invalidate).not.toHaveBeenCalled();
    expect(GMAIL_EVENTS).toContain("GmailLabelsChanged");
  });

  it("lists the core's label and category texts", () => {
    expect(runtimeIds()).toContain("gmail-label-issue-duplicate-name");
    expect(runtimeIds()).toContain("gmail-category-promotions");
  });
});
