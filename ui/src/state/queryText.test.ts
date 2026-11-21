import { describe, expect, it } from "vitest";
import { searchValueText } from "./queryText";

describe("searchValueText", () => {
  it("leaves plain values and quotes the rest with escapes", () => {
    expect(searchValueText("Travel")).toBe("Travel");
    expect(searchValueText("Follow up")).toBe('"Follow up"');
    expect(searchValueText('say "hi"')).toBe('"say \\"hi\\""');
    expect(searchValueText("a\\b")).toBe('"a\\\\b"');
    expect(searchValueText("(x)")).toBe('"(x)"');
  });
});
