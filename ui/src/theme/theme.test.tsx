import { render } from "@testing-library/react";
import type { ResolvedTheme } from "../api/generated";
import { defaultTheme } from "../api/mockData";
import { normalizeTheme } from "../api/normalize";
import {
  compileTheme,
  cssString,
  effectiveMode,
  isSafeValue,
  resolveTokenRefs,
  ruleSelector,
} from "./compile";
import { READER_CSP, readerCss, readerDocument } from "./reader";
import { StyleManager } from "./styleManager";
import { ThemeProvider, useGeometry } from "./ThemeProvider";

const styleElements = () => document.head.querySelectorAll("style[data-nail-theme]");

describe("theme compiler", () => {
  const { css, warnings } = compileTheme(defaultTheme());

  it("compiles the default theme without dropping rules", () => {
    expect(warnings).toEqual([]);
  });

  it("emits tokens as custom properties and mode overrides keyed on data-nail-mode", () => {
    expect(css).toContain("--nail-color-accent: #2e5eaa;");
    expect(css).toContain("--nail-border-thin: 1px solid var(--nail-color-border);");
    expect(css).toMatch(
      /:root\[data-nail-mode="dark"\] \{[^}]*--nail-color-accent: #7aa7e8;[^}]*color-scheme: dark;/,
    );
    expect(css).toContain(':root[data-nail-mode="high-contrast"]');
  });

  it("keys part rules on data-part with states, pseudo-classes and conditions", () => {
    expect(css).toContain('[data-part="message-list.row"][data-state~="selected"] {');
    expect(css).toContain('[data-part="split.divider"]:hover {');
    expect(css).toContain('[data-part="folder-tree.item"][data-role~="inbox"] {');
    expect(css).toContain(
      '[data-part="message-list.row"][data-state~="unread"] [data-part="message-list.row.subject"] {\n  font-weight: var(--nail-font-weight-bold);',
    );
  });

  it("keeps part rules in source order", () => {
    const plain = css.indexOf('[data-part="toolbar.button"] {');
    const hover = css.indexOf('[data-part="toolbar.button"]:hover {');
    expect(plain).toBeGreaterThan(0);
    expect(hover).toBeGreaterThan(plain);
  });

  it("builds selectors for conditions without values and quotes values", () => {
    expect(
      ruleSelector({
        selector: "message-list.row",
        states: ["focus-visible"],
        conditions: [{ key: "encrypted", value: null }],
        ancestors: [],
      }),
    ).toBe('[data-part="message-list.row"]:focus-visible[data-encrypted]');
    expect(cssString('a"b\\c')).toBe('"a\\"b\\\\c"');
    expect(
      ruleSelector({
        selector: "x",
        states: [],
        conditions: [{ key: "tag", value: 'w"] body {' }],
        ancestors: [],
      }),
    ).toBe('[data-part="x"][data-tag~="w\\"] body {"]');
  });

  it("drops unsafe rules with a warning", () => {
    const theme = normalizeTheme({
      parts: [
        { selector: "app", declarations: { color: "red; } body { display: none" } },
        { selector: "app", declarations: { background: "url(https://evil.example/x.png)" } },
        { selector: "Bad Part", declarations: { color: "red" } },
        { selector: "app", declarations: { "col:or": "red" } },
        { selector: "app", states: ["x]"], declarations: { color: "red" } },
        { selector: "app", declarations: { color: "blue" } },
      ],
      raw_css: [
        { selector: "app", css: "@import 'x.css'" },
        { selector: "text", css: "text-wrap: balance;" },
      ],
    });
    const out = compileTheme(theme);
    expect(out.warnings).toHaveLength(6);
    expect(out.css).not.toContain("evil");
    expect(out.css).not.toContain("body { display: none");
    expect(out.css).toContain('[data-part="app"] {\n  color: blue;\n}');
    expect(out.css).toContain('[data-part="text"] {\n  text-wrap: balance;\n}');
  });

  it("styles plug-in parts: base rules, defaults and one plug-in's parts", () => {
    // Structural base rules of the plug-in UI and the default theme's styles.
    expect(css).toContain('[data-part="plugin.dialog.backdrop"] { position: fixed;');
    expect(css).toContain('[data-part="plugin.button"][data-variant~="primary"] {');
    // `plugin.<id>.<part>` is one plug-in's generic part.
    expect(
      ruleSelector({
        selector: "plugin.follow-up.button",
        states: [],
        conditions: [],
        ancestors: [],
      }),
    ).toBe('[data-part="plugin.button"][data-plugin="follow-up"]');
    // Plug-in widgets outside the generic set keep their own part.
    expect(
      ruleSelector({
        selector: "plugin-manager.plugin",
        states: [],
        conditions: [],
        ancestors: [],
      }),
    ).toBe('[data-part="plugin-manager.plugin"]');
  });

  it("validates values", () => {
    expect(isSafeValue("0 1px 3px rgb(0 0 0 / 0.12)")).toBe(true);
    expect(isSafeValue('"Segoe UI", sans-serif')).toBe(true);
    for (const bad of [
      "a;b",
      "}",
      "url(x)",
      "URL (x)",
      "expression(alert(1))",
      "/* x */",
      "a\\62 c",
      "<b>",
    ]) {
      expect(isSafeValue(bad)).toBe(false);
    }
  });

  it("chooses the effective mode", () => {
    expect(effectiveMode("system", false, false)).toBe("light");
    expect(effectiveMode("system", true, false)).toBe("dark");
    expect(effectiveMode("system", true, true)).toBe("high-contrast");
    expect(effectiveMode("light", true, true)).toBe("light");
    expect(effectiveMode("dark", false, false)).toBe("dark");
  });

  it("resolves token references for the reader iframe", () => {
    const theme = defaultTheme();
    expect(resolveTokenRefs("var(--nail-color-fg)", theme, "light")).toBe("#1d2129");
    expect(resolveTokenRefs("var(--nail-color-fg)", theme, "dark")).toBe("#e3e6eb");
    expect(resolveTokenRefs("var(--nail-border-thin)", theme, "dark")).toBe("1px solid #353b45");
    expect(resolveTokenRefs("var(--nail-nope, red)", theme, "light")).toBe("red");
  });
});

describe("reader document", () => {
  it("carries a strict CSP and resolved reader styles", () => {
    const theme = defaultTheme();
    const css = readerCss(theme, "dark");
    expect(css).toContain("color-scheme: dark");
    expect(css).toContain("#e3e6eb");
    expect(css).not.toContain("var(--nail");
    expect(css).toContain("blockquote blockquote {");
    const doc = readerDocument("<p>Hi</p>", css);
    expect(doc).toContain(`<meta http-equiv="Content-Security-Policy" content="${READER_CSP}">`);
    expect(READER_CSP).toContain("default-src 'none'");
    expect(doc).toContain("<body><p>Hi</p></body>");
  });
});

describe("StyleManager", () => {
  it("keeps geometry rules across theme swaps", () => {
    const manager = new StyleManager();
    manager.setTheme('[data-part="app"] { color: red; }');
    manager.setGeometry("g1", { flex: "0 0 240px", "min-width": "160px" });
    manager.setGeometry("g2", { height: "10px", evil: "url(x)" });
    const rules = () => [...(manager.element.sheet?.cssRules ?? [])].map((r) => r.cssText);
    expect(rules()).toHaveLength(3);
    expect(manager.geometryOf("g2")).toEqual({ height: "10px" });

    manager.setTheme('[data-part="app"] { color: blue; }');
    expect(rules()[0]).toContain("blue");
    expect(rules().some((r) => r.includes('[data-geom="g1"]') && r.includes("240px"))).toBe(true);

    manager.setGeometry("g1", { flex: "0 0 300px" });
    const g1 = rules().find((r) => r.includes('[data-geom="g1"]')) ?? "";
    expect(g1).toContain("300px");
    expect(g1).not.toContain("min-width");

    manager.removeGeometry("g2");
    expect(rules()).toHaveLength(2);
    manager.dispose();
    expect(styleElements()).toHaveLength(0);
  });
});

function GeometryProbe({ size }: { size: string }) {
  useGeometry("probe", { width: size });
  return <div data-part="probe" data-geom="probe" />;
}

describe("ThemeProvider", () => {
  const withAccent = (theme: ResolvedTheme, accent: string): ResolvedTheme => ({
    ...theme,
    tokens: { ...theme.tokens, color: { ...theme.tokens.color, accent } },
  });

  it("injects one style element, hot-swaps themes and sets the mode", () => {
    const theme = defaultTheme();
    const { rerender, unmount } = render(
      <ThemeProvider theme={theme}>
        <GeometryProbe size="10px" />
      </ThemeProvider>,
    );
    expect(styleElements()).toHaveLength(1);
    expect(styleElements()[0]?.textContent).toContain("--nail-color-accent: #2e5eaa;");
    expect(document.documentElement.dataset.nailMode).toBe("light");

    rerender(
      <ThemeProvider theme={withAccent(theme, "#ff00aa")} modeOverride="dark">
        <GeometryProbe size="20px" />
      </ThemeProvider>,
    );
    expect(styleElements()).toHaveLength(1);
    const style = styleElements()[0] as HTMLStyleElement;
    expect(style.textContent).toContain("--nail-color-accent: #ff00aa;");
    expect(document.documentElement.dataset.nailMode).toBe("dark");
    const rules = [...(style.sheet?.cssRules ?? [])].map((r) => r.cssText);
    expect(rules.filter((r) => r.includes('[data-geom="probe"]'))).toEqual([
      expect.stringContaining("width: 20px"),
    ]);

    unmount();
    expect(styleElements()).toHaveLength(0);
  });

  it("follows prefers-color-scheme in system mode", () => {
    const original = window.matchMedia;
    window.matchMedia = ((query: string) => ({
      matches: query === "(prefers-color-scheme: dark)",
      media: query,
      addEventListener: () => {},
      removeEventListener: () => {},
    })) as unknown as typeof window.matchMedia;
    const { unmount } = render(
      <ThemeProvider theme={defaultTheme()}>
        <span />
      </ThemeProvider>,
    );
    expect(document.documentElement.dataset.nailMode).toBe("dark");
    unmount();
    window.matchMedia = original;
  });
});
