import { describe, expect, it } from "vitest";
import type { HtmlDarkMode, ResolvedTheme } from "../api/generated";
import { defaultTheme } from "../api/mockData";
import { resolveTokenRefs } from "./compile";
import { hexContrastRatio } from "./contrast";
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
    // A loaded image keeps its height attribute (a 1×1 spacer at width 200 / height 50 stays 50 high).
    expect(css).toContain("img[src]:not([height]) { height: auto; }");
    expect(css).not.toMatch(/img\[src\] \{ height: auto; \}/);
  });

  it("recolours plain mail in the smart dark mode", () => {
    const css = readerCss(withDarkMode("smart"), "dark");
    expect(css).toContain(":root { color-scheme: dark; }");
    expect(css).toContain(
      "body:not(:has(> style, table, [bgcolor], [background], [style*='background'])) * { background-color: transparent !important;",
    );
    // No nested :has() (CSS forbids it; Chromium drops the whole rule).
    expect(css).not.toMatch(/:has\([^)]*:has\(/);
    expect(css).not.toContain("var(--nail");
  });

  it("draws designed mail on a light paper in the theme's print colours", () => {
    const theme = withDarkMode("smart");
    const css = readerCss(theme, "dark", true);
    expect(css).toContain(":root { color-scheme: light; }");
    expect(css).not.toContain("color-scheme: dark");
    expect(css).not.toContain("!important");
    expect(css).not.toMatch(/:has\([^)]*:has\(/);
    const print = (name: string) => resolveTokenRefs(`var(--nail-color-${name})`, theme, "print");
    // The paper rules come after the theme's dark ones, so they win.
    const paper = `:where(body) { color: ${print("fg")}; background-color: ${print("bg")}; }`;
    const link = `:where(a) { color: ${print("accent")}; }`;
    expect(css).toContain(paper);
    expect(css).toContain(link);
    expect(css.lastIndexOf(":where(a)")).toBe(css.indexOf(link));
    expect(css.indexOf(paper)).toBeGreaterThan(css.indexOf(":where(body) {"));
    // The text keeps off the paper's edge, after the base body rule.
    const padding = "body { padding: 12px 16px 24px; }";
    expect(css.indexOf(padding)).toBeGreaterThan(css.indexOf("body { margin: 0;"));
    expect(readerCss(theme, "dark", false)).not.toContain(padding);
    // A light link colour on white paper would be unreadable (≥ 4.5:1 wanted).
    expect(contrast(print("accent"), print("bg"))).toBeGreaterThanOrEqual(4.5);
  });

  it("falls back to a white paper for a theme without print colours", () => {
    const theme = withDarkMode("smart");
    const modes = Object.fromEntries(Object.entries(theme.modes).filter(([m]) => m !== "print"));
    const css = readerCss({ ...theme, modes }, "dark", true);
    expect(css).toContain(":where(body) { color: #1f1f1f; background-color: #ffffff; }");
    expect(css).toContain(":where(a) { color: #0b57d0; }");
  });

  it("does not use the paper in light mode", () => {
    expect(readerCss(withDarkMode("smart"), "light", true)).toBe(
      readerCss(withDarkMode("smart"), "light"),
    );
  });

  it("forces the theme's colours on every mail in high contrast", () => {
    const theme = withDarkMode("smart");
    const fg = resolveTokenRefs("var(--nail-color-fg)", theme, "high-contrast");
    for (const designed of [false, true]) {
      const css = readerCss(theme, "high-contrast", designed);
      expect(css).toContain(":root { color-scheme: dark; }");
      expect(css).toContain(
        `body * { background-color: transparent !important; color: ${fg} !important; }`,
      );
      expect(css).not.toContain("#ffffff; }");
    }
    expect(readerCss(withDarkMode("off"), "high-contrast", true)).not.toContain("!important");
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

/** WCAG contrast ratio of two hex colours. */
const contrast = (a: string, b: string): number => hexContrastRatio(a, b) ?? 0;
