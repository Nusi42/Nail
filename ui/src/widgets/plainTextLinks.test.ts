/** Links in plain-text mail (ADR 0211). */
import { describe, expect, it } from "vitest";
import { EXTERNAL_LINK_SCHEMES, MAILTO_SCHEME } from "../api/generated";
import { linkify } from "./plainTextLinks";

const links = (text: string) =>
  linkify(text)
    .map((p) => p.href)
    .filter((h) => h !== null);

describe("linkify", () => {
  it("finds web, phone and mail links and keeps the text between them", () => {
    expect(linkify("See https://example.com/a?b=1 or call tel:+1-555-0100 now")).toEqual([
      { text: "See ", href: null },
      { text: "https://example.com/a?b=1", href: "https://example.com/a?b=1" },
      { text: " or call ", href: null },
      { text: "tel:+1-555-0100", href: "tel:+1-555-0100" },
      { text: " now", href: null },
    ]);
    expect(links("write mailto:ada@example.org?subject=Hi.")).toEqual([
      "mailto:ada@example.org?subject=Hi",
    ]);
  });

  it("joins back to the original text", () => {
    const text = "a http://x.example/ b\nhttps://y.example/z)\n\nend";
    expect(
      linkify(text)
        .map((p) => p.text)
        .join(""),
    ).toBe(text);
  });

  it("leaves sentence punctuation and unbalanced brackets out of the link", () => {
    expect(links("Go to https://example.com/page.")).toEqual(["https://example.com/page"]);
    expect(links("(see https://example.com/page)")).toEqual(["https://example.com/page"]);
    expect(links("<https://example.com/page>,")).toEqual(["https://example.com/page"]);
    expect(links("https://example.com/wiki/Rust_(language)")).toEqual([
      "https://example.com/wiki/Rust_(language)",
    ]);
    expect(links('"https://example.com/q?a=1&b=2";')).toEqual(["https://example.com/q?a=1&b=2"]);
  });

  it("only makes the schemes the core opens clickable", () => {
    expect(links("file:///C:/Windows/notepad.exe javascript:alert(1) ftp://example.com/x")).toEqual(
      [],
    );
    expect(links("data:text/html,hi")).toEqual([]);
    // Every scheme of the core's list is recognized.
    for (const scheme of [...EXTERNAL_LINK_SCHEMES, MAILTO_SCHEME]) {
      const target = ["http", "https"].includes(scheme)
        ? `${scheme}://example.com/`
        : `${scheme}:x@example.com`;
      expect(links(`go ${target} end`)).toEqual([target]);
    }
  });

  it("ignores a scheme inside a word and a scheme without an address", () => {
    expect(links("xhttp://example.com")).toEqual([]);
    expect(links("http:// and https:/ and tel: and mailto:")).toEqual([]);
  });

  it("handles empty text", () => {
    expect(linkify("")).toEqual([]);
  });
});
