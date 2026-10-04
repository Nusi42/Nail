import { describe, expect, it } from "vitest";
import type { HtmlDarkMode, ResolvedTheme } from "../api/generated";
import { defaultTheme } from "../api/mockData";
import { READER_CSP, isDesignedMail, readerCsp, readerCss, readerDocument } from "./reader";

/** The default theme with another dark-mode handling of HTML mail. */
function withDarkMode(mode: HtmlDarkMode): ResolvedTheme {
  const theme = defaultTheme();
  return { ...theme, message: { ...theme.message, html_dark_mode: mode } };
}

describe("reader stylesheet (P1 WP190)", () => {
  it("allows no cid: images: the core embeds inline images as data: URLs", () => {
    expect(READER_CSP).not.toContain("cid:");
    expect(READER_CSP).toContain("img-src data:;");
    expect(readerCsp("https://nail.local:4780")).not.toContain("cid:");
  });

  it("gives way to the mail's own design", () => {
    const css = readerCss(defaultTheme(), "light");
    // Colours at zero specificity, so the mail's `body { … }` wins.
    expect(css).toMatch(
      /:where\(body\) \{ color: #[0-9a-f]{6}; background-color: #[0-9a-f]{6}; \}/,
    );
    expect(css).toMatch(/:where\(a\) \{ color: #[0-9a-f]{6}; \}/);
    expect(css).not.toMatch(/(^|\n)body \{[^}]*color:/);
    // The reading width only for mail without its own layout.
    expect(css).toContain(
      "body:not(:has(> style, table, [bgcolor], [background], [style*='background'])) > * { max-width: 68ch; }",
    );
    expect(css).not.toMatch(/(^|\n)body > \* \{/);
    // No dark-mode rules in light mode.
    expect(css).not.toContain("!important");
  });

  it("shows images that did not load as their alt text, not a broken icon", () => {
    const css = readerCss(defaultTheme(), "light");
    expect(css).toContain("img[alt]::after { content: attr(alt);");
    expect(css).toContain('img:not([src])[alt]:not([alt=""]) { outline: 1px dashed');
    expect(css).toContain('img[alt=""]::after { border: none; background-color: transparent; }');
  });

  it("keeps designed mail's colours on a light paper in the smart dark mode", () => {
    const css = readerCss(withDarkMode("smart"), "dark");
    expect(css).toContain("color-scheme: dark");
    expect(css).toContain(
      "body:not(:has(> style, table, [bgcolor], [background], [style*='background'])) * { background-color: transparent !important;",
    );
    expect(css).toContain(
      ":root:has(> body:has(> style, table, [bgcolor], [background], [style*='background'])) { color-scheme: light; }",
    );
    expect(css).toContain(
      ":where(body:has(> style, table, [bgcolor], [background], [style*='background'])) { color: #1f1f1f; background-color: #ffffff; }",
    );
    expect(css).not.toContain("var(--nail");
  });

  it("keeps the invert and off dark modes", () => {
    expect(readerCss(withDarkMode("invert"), "dark")).toContain(
      "html { filter: invert(1) hue-rotate(180deg); }",
    );
    const off = readerCss(withDarkMode("off"), "dark");
    expect(off).not.toContain("!important");
    expect(off).not.toContain("invert(1)");
  });

  it("puts the mail's style sheet into the body after the reader's", () => {
    const doc = readerDocument(
      '<style>\n.hero { color: red }\n</style><p class="hero">Hi</p>',
      "body{}",
    );
    expect(doc.indexOf("<style>body{}</style></head>")).toBeLessThan(
      doc.indexOf(".hero { color: red }"),
    );
    expect(doc).toContain(
      '<body><style>\n.hero { color: red }\n</style><p class="hero">Hi</p></body>',
    );
  });
});

describe("designed mail", () => {
  it.each([
    ['<style>\n.a { color: red }\n</style><p class="a">x</p>', true],
    ['<table width="600"><tbody><tr><td>x</td></tr></tbody></table>', true],
    ['<div bgcolor="#fff">x</div>', true],
    ['<p style="color: red; background-color: #eee">x</p>', true],
    ["<p>Hello <b>Sam</b></p><blockquote>quoted</blockquote>", false],
    ['<p style="color: red">Styles without a background</p>', false],
  ])("%s → %s", (html, designed) => {
    expect(isDesignedMail(html)).toBe(designed);
  });
});
