import { describe, expect, it } from "vitest";
import { KEBAB } from "./kebab";

describe("KEBAB", () => {
  it("accepts lowercase words joined by single hyphens", () => {
    for (const name of ["follow-up", "csv-preview", "mail-list2", "a"])
      expect(KEBAB.test(name)).toBe(true);
  });

  it("refuses capitals, leading digits, double and trailing hyphens", () => {
    for (const name of ["Follow-up", "2fa", "follow--up", "follow-", "-follow", "follow_up", ""]) {
      expect(KEBAB.test(name)).toBe(false);
    }
  });
});
