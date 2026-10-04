import { charCount, chars } from "./chars";

describe("code point text measures", () => {
  it("counts code points like Rust's str::chars, not UTF-16 units", () => {
    expect(charCount("")).toBe(0);
    expect(charCount("abc")).toBe(3);
    // U+1F600 is one code point but two UTF-16 units.
    expect("\u{1F600}".length).toBe(2);
    expect(charCount("a\u{1F600}b")).toBe(3);
    // An emoji with a skin-tone modifier: two code points.
    expect(charCount("\u{1F44D}\u{1F3FD}")).toBe(2);
  });

  it("splits text into code points", () => {
    expect(chars("a\u{1F600}b")).toEqual(["a", "\u{1F600}", "b"]);
    expect(chars("")).toEqual([]);
  });
});
