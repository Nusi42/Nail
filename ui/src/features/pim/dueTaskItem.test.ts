/** R7 S12: where Plan puts a task when no slot of the grid was chosen. */
import { describe, expect, it } from "vitest";
import { fallbackPlanSlot } from "./DueTaskItem";

describe("fallbackPlanSlot", () => {
  it("takes today at the next whole hour in the zone, at the latest the day's last hour", () => {
    // 10:40 UTC is 11:40 in Berlin: noon.
    expect(fallbackPlanSlot(Date.UTC(2026, 10, 25, 10, 40), "Europe/Berlin")).toEqual({
      day: "2026-11-25",
      minute: 12 * 60,
    });
    expect(fallbackPlanSlot(Date.UTC(2026, 10, 25, 23, 30), "UTC")).toEqual({
      day: "2026-11-25",
      minute: 23 * 60,
    });
  });
});
