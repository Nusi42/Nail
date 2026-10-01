import { QueryClient } from "@tanstack/react-query";
import { describe, expect, it, vi } from "vitest";
import { MockTransport } from "../../api/mock";
import type { FeatureEventContext } from "../types";
import { EXCHANGE_EVENTS, exchangeKeys, handleExchangeEvent } from ".";
import { CATEGORY_COLORS, runtimeIds } from "./messages";
import { EXCHANGE_REQUESTS, isExchangeRequest } from "./mock";

describe("exchange mock", () => {
  it("routes exactly its requests", () => {
    expect(new Set(EXCHANGE_REQUESTS).size).toBe(EXCHANGE_REQUESTS.length);
    expect(EXCHANGE_REQUESTS).toHaveLength(18);
    expect(isExchangeRequest({ type: "GetExchangeStatus", account_id: "work" })).toBe(true);
    expect(
      isExchangeRequest({
        type: "ClassifyFocused",
        message_ids: ["m1"],
        classification: "other",
        always: false,
      }),
    ).toBe(true);
    expect(isExchangeRequest({ type: "ListGmailLabels", account_id: "gm" })).toBe(false);
  });

  it("answers like the core at the contract stage", async () => {
    const t = new MockTransport();
    expect(t.data.features.exchange.categories).toEqual({});
    expect(await t.send({ type: "GetExchangeStatus", account_id: "work" })).toMatchObject({
      type: "Error",
      code: "unsupported",
    });
    expect(
      await t.send({
        type: "SetAutomaticReplies",
        account_id: "work",
        settings: {
          status: "always-enabled",
          start: null,
          end: null,
          internal_reply: "Out",
          external_reply: "Out",
          external_audience: "all",
        },
      }),
    ).toMatchObject({ type: "Error", code: "unsupported" });
  });

  it("refetches the changed areas of one account, everything on mail events", () => {
    const queryClient = new QueryClient();
    const invalidate = vi.spyOn(queryClient, "invalidateQueries");
    const ctx = { queryClient } as unknown as FeatureEventContext;
    handleExchangeEvent(
      { type: "ExchangeChanged", account_id: "work", areas: ["categories", "rules"] },
      ctx,
    );
    expect(invalidate).toHaveBeenCalledWith({
      queryKey: exchangeKeys.area("work", "categories"),
    });
    expect(invalidate).toHaveBeenCalledWith({ queryKey: exchangeKeys.area("work", "rules") });
    invalidate.mockClear();
    handleExchangeEvent({ type: "MailChanged", folder_ids: ["f1"] }, ctx);
    expect(invalidate).toHaveBeenCalledWith({ queryKey: exchangeKeys.all });
    invalidate.mockClear();
    handleExchangeEvent({ type: "PimSourcesChanged" }, ctx);
    expect(invalidate).not.toHaveBeenCalled();
    expect(EXCHANGE_EVENTS).toContain("ExchangeChanged");
  });

  it("lists the core's Exchange texts", () => {
    expect(CATEGORY_COLORS).toHaveLength(26);
    expect(CATEGORY_COLORS[25]).toBe("preset24");
    const ids = runtimeIds();
    expect(ids).toContain("exchange-warning-ews-retired-online");
    expect(ids).toContain("exchange-focus-other");
    expect(ids).toContain("exchange-color-preset24");
    expect(ids).toContain("exchange-oof-issue-end-before-start");
  });
});
