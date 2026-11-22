import { describe, expect, it } from "vitest";
import { sameJson } from "./json";

describe("sameJson", () => {
  it("compares plain data by value", () => {
    expect(sameJson({ a: [1, "x"] }, { a: [1, "x"] })).toBe(true);
    expect(sameJson([1, 2], [2, 1])).toBe(false);
    expect(sameJson(null, null)).toBe(true);
    expect(sameJson(null, {})).toBe(false);
    // Key order is part of the value (states are built the same way).
    expect(sameJson({ a: 1, b: 2 }, { b: 2, a: 1 })).toBe(false);
  });
});
