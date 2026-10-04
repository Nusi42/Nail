import { hasText, nonEmpty } from "./empty";

describe("empty text", () => {
  it("treats empty text like missing text", () => {
    expect(nonEmpty("")).toBeUndefined();
    expect(nonEmpty(null)).toBeUndefined();
    expect(nonEmpty(undefined)).toBeUndefined();
    expect(nonEmpty(" ")).toBe(" ");
    expect(nonEmpty("Ada") ?? "fallback").toBe("Ada");
    expect(nonEmpty("") ?? "fallback").toBe("fallback");
  });

  it("tells whether there is text", () => {
    expect(hasText("x")).toBe(true);
    expect(hasText("")).toBe(false);
    expect(hasText(null)).toBe(false);
  });
});
