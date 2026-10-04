import { describe, expect, it } from "vitest";
import { MockTransport } from "../../api/mock";
import { features } from "../registry";
import { ACTION_AVAILABILITIES, runtimeIds } from "./messages";
import { MAX_RECENT_ACTIONS, isActionId, isPaletteRequest, recordAction } from "./mock";

describe("palette slot (M4c contract)", () => {
  it("is registered and lists its runtime ids", () => {
    expect(features.palette).toBeDefined();
    expect(ACTION_AVAILABILITIES).toHaveLength(8);
    expect(runtimeIds()).toContain("action-availability-desktop");
    expect(isPaletteRequest({ type: "GetRecentActions" })).toBe(true);
    expect(isPaletteRequest({ type: "Ping" })).toBe(false);
  });

  it("validates action ids like the core", () => {
    expect(isActionId("mail.reply")).toBe(true);
    expect(isActionId("")).toBe(false);
    expect(isActionId("mail reply")).toBe(false);
    expect(isActionId("x".repeat(161))).toBe(false);
  });

  it("records recent actions most recent first, counted and capped", () => {
    let recent = { actions: [] as { action_id: string; last_used_at: string; count: number }[] };
    recent = recordAction(recent, "a", "t1");
    recent = recordAction(recent, "b", "t2");
    recent = recordAction(recent, "a", "t3");
    expect(recent.actions.map((a) => [a.action_id, a.count])).toEqual([
      ["a", 2],
      ["b", 1],
    ]);
    for (let n = 0; n < 30; n++) recent = recordAction(recent, `n.${String(n)}`, "t");
    expect(recent.actions).toHaveLength(MAX_RECENT_ACTIONS);
  });

  it("answers through the mock transport", async () => {
    const t = new MockTransport();
    expect(await t.send({ type: "RecordActionUsed", action_id: "mail.reply" })).toMatchObject({
      type: "RecentActions",
      recent: { actions: [{ action_id: "mail.reply", count: 1 }] },
    });
    expect(await t.send({ type: "RecordActionUsed", action_id: "bad id" })).toMatchObject({
      type: "Error",
      code: "invalid_request",
      localized: { id: "palette-invalid-action" },
    });
    expect(await t.send({ type: "ClearRecentActions" })).toEqual({
      type: "RecentActions",
      recent: { actions: [] },
    });
  });
});
