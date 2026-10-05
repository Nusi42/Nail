import { render } from "@testing-library/react";
import type { ResolvedTheme } from "../api/generated";
import { defaultTheme } from "../api/mockData";
import { BASE_CSS, COMPACT_VARIANT, compileTheme, resolveTokenRefs } from "./compile";
import {
  AVATAR_COLORS,
  CONTROL_WIDTHS,
  CONTROL_WIDTH_VAR,
  SKIN_CSS,
  colorSlot,
  stableHash,
} from "./skin";
import { ThemeProvider } from "./ThemeProvider";
import { PIM_SKIN_CSS } from "../features/pim/skin";
import { SETTINGS_SKIN_CSS } from "../features/settings/skin";
import { SHELL_SKIN_CSS } from "../features/shell/skin";
import { WINDOWS_SKIN_CSS } from "../features/windows/skin";
import { COMPOSER_SKIN_CSS } from "../features/composer/skin";
import { POPOUT_BASE_CSS } from "../features/popout/parts";
import { MESSAGE_LIST_SKIN_CSS } from "../widgets/messageListSkin";
import { THREAD_VIEW_SKIN_CSS } from "../widgets/threadViewSkin";
import { must } from "../test/must";

/** WCAG relative luminance of `#rrggbb`. */
function luminance(hex: string): number {
  const channel = (i: number) => {
    const c = parseInt(hex.slice(i, i + 2), 16) / 255;
    return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
  };
  return 0.2126 * channel(1) + 0.7152 * channel(3) + 0.0722 * channel(5);
}

function contrast(a: string, b: string): number {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x) as [number, number];
  return (hi + 0.05) / (lo + 0.05);
}

describe("M5a skin (theme/skin.ts)", () => {
  it("sits between the structure and the theme's part rules", () => {
    const theme: ResolvedTheme = {
      ...defaultTheme(),
      parts: [
        {
          selector: "form.button",
          states: [],
          conditions: [],
          ancestors: [],
          declarations: { color: "red" },
          source: null,
        },
      ],
    };
    const css = compileTheme(theme).css;
    const base = css.indexOf(BASE_CSS);
    const skin = css.indexOf(SKIN_CSS);
    const rule = css.indexOf('[data-part="form.button"] {\n  color: red;');
    expect(base).toBe(0);
    expect(skin).toBeGreaterThan(base);
    expect(rule).toBeGreaterThan(skin);
  });

  it("styles every primitive the plan names", () => {
    for (const part of [
      "form.button",
      "toolbar.button",
      "form.input",
      "form.select",
      "chip",
      "tag",
      "badge",
      "context-menu.item",
      "context-menu.header",
      "context-menu.separator",
      "context-menu.item.shortcut",
      "tabs.tab",
      "segmented.option",
      "card",
      "switch",
      "message-list.row.avatar",
      "message-list.row.unread-dot",
      "message-list.row.tag-line",
      "message-view.column",
      "attachment-card",
    ]) {
      expect(SKIN_CSS).toContain(`[data-part="${part}"]`);
    }
    for (const variant of ["primary", "danger", "ghost", "icon"]) {
      expect(SKIN_CSS).toContain(`[data-variant~="${variant}"]`);
    }
  });

  it("every skin closes each block it opens (a stray brace drops the next rule)", () => {
    for (const [name, css] of Object.entries({
      SKIN_CSS,
      SHELL_SKIN_CSS,
      SETTINGS_SKIN_CSS,
      PIM_SKIN_CSS,
      WINDOWS_SKIN_CSS,
      COMPOSER_SKIN_CSS,
      POPOUT_BASE_CSS,
      MESSAGE_LIST_SKIN_CSS,
      THREAD_VIEW_SKIN_CSS,
    })) {
      let depth = 0;
      for (const char of css) {
        if (char === "{") depth += 1;
        if (char === "}") depth -= 1;
        expect(depth, `${name} closes a block it never opened`).toBeGreaterThanOrEqual(0);
      }
      expect(depth, `${name} leaves a block open`).toBe(0);
    }
  });

  it("takes colours only from tokens (literal colours only as token fallbacks)", () => {
    for (const css of [
      SKIN_CSS,
      WINDOWS_SKIN_CSS,
      COMPOSER_SKIN_CSS,
      POPOUT_BASE_CSS,
      MESSAGE_LIST_SKIN_CSS,
      THREAD_VIEW_SKIN_CSS,
    ]) {
      const withoutFallbacks = css.replace(/var\(--nail-[a-z0-9-]+, #[0-9a-f]{3,8}\)/gi, "");
      expect(withoutFallbacks).not.toMatch(/#[0-9a-f]{3,8}\b/i);
      expect(css).not.toMatch(/url\(|@import/);
    }
  });

  it("emits the M5c window manager and composer skins after the shared skin, before the theme", () => {
    const theme: ResolvedTheme = {
      ...defaultTheme(),
      parts: [
        {
          selector: "window",
          states: [],
          conditions: [],
          ancestors: [],
          declarations: { color: "red" },
          source: null,
        },
      ],
    };
    const css = compileTheme(theme).css;
    const skin = css.indexOf(SKIN_CSS);
    const windows = css.indexOf(WINDOWS_SKIN_CSS);
    const composer = css.indexOf(COMPOSER_SKIN_CSS);
    const rule = css.indexOf('[data-part="window"] {\n  color: red;');
    expect(css.indexOf(POPOUT_BASE_CSS)).toBeGreaterThan(-1);
    expect(css.indexOf(POPOUT_BASE_CSS)).toBeLessThan(skin);
    expect(windows).toBeGreaterThan(skin);
    expect(composer).toBeGreaterThan(windows);
    expect(rule).toBeGreaterThan(composer);
    // The modal-frame rules of M1 are gone: every open window is shown.
    expect(BASE_CSS).not.toContain('[data-part="window"]:not([data-state~="active"])');
  });

  it("puts a state on every selector of a group, not only the last one", () => {
    const states = [":hover", ":focus-visible", ":focus-within", ":disabled", ":active"];
    for (const [selectors = ""] of SKIN_CSS.split("}").map((block) => block.split("{"))) {
      const items = selectors.split(",").map((item) => item.trim());
      if (items.length < 2) continue;
      for (const state of states) {
        if (items.some((item) => item.includes(state))) {
          expect(
            // A bare part next to `part:state` is the `ps(…):state` slip.
            items.filter((item) => /^\[data-part="[^"]+"\]$/.test(item)),
            `${state} in ${selectors.trim()}`,
          ).toEqual([]);
        }
      }
    }
  });

  it("lets text-like controls fill their column unless data-width sizes them (ADR 0160)", () => {
    const rule = SKIN_CSS.split("}")
      .map((block) => block.trim())
      .find((block) =>
        block.startsWith(
          '[data-part="form.input"], [data-part="form.select"], [data-part="form.textarea"] {',
        ),
      );
    expect(rule).toBeDefined();
    // Full width in blocks, grids and columns; the content as basis in a flex row.
    expect(rule).toContain(
      `width: var(${CONTROL_WIDTH_VAR}, 100%); flex-basis: content; min-width: 0; max-width: 100%;`,
    );
    for (const [name, width] of Object.entries(CONTROL_WIDTHS)) {
      expect(SKIN_CSS).toContain(
        `[data-part][data-width="${name}"] { ${CONTROL_WIDTH_VAR}: ${width}; width: var(${CONTROL_WIDTH_VAR}); max-width: 100%;`,
      );
    }
  });

  it("outlines input controls with the theme's control border", () => {
    // The base theme has no default rules for these since M5a (WP155).
    const blocks = SKIN_CSS.split("}").map((block) => block.split("{"));
    for (const part of ["form.input", "form.select", "form.textarea", "search-bar.field"]) {
      const own = blocks.filter(([selectors = ""]) =>
        selectors.split(",").some((s) => s.trim() === `[data-part="${part}"]`),
      );
      expect(
        own.some(([, body = ""]) => body.includes("border: var(--nail-border-control,")),
        part,
      ).toBe(true);
    }
    expect(defaultTheme().parts.some((r) => r.selector === "form.input")).toBe(false);
  });

  it("keeps the reader text at a readable line length in a centred column", () => {
    expect(SKIN_CSS).toMatch(/\[data-part="message-view.text"\] \{[^}]*max-width: 68ch/);
    expect(SKIN_CSS).toMatch(/\[data-part="message-view.column"\] \{[^}]*margin-inline: auto/);
    // The face and line length of the text come from the theme's `message`
    // section, after the skin, so a user can still pick monospace.
    const css = compileTheme(defaultTheme()).css;
    const reader = css.slice(css.lastIndexOf('[data-part="message-view.text"] {'));
    expect(css.lastIndexOf('[data-part="message-view.text"] {')).toBeGreaterThan(
      css.indexOf(SKIN_CSS),
    );
    expect(reader).toMatch(/font-family: var\(--nail-font-reader\)/);
  });

  /** The declarations of the skin's rule whose selector list is exactly `selector`. */
  const ruleBody = (selector: string) =>
    SKIN_CSS.split("}")
      .map((block) => block.split("{"))
      .find(([selectors = ""]) => selectors.trim() === selector)?.[1] ?? "";

  it("draws an off switch as a visible thumb on a plain, outlined track", () => {
    // Off: `fg-muted` thumb on a `bg` track (checked as a pair in
    // tests/theme_contrast.rs), outlined like every input control
    // (2px in High contrast).
    const track = ruleBody('[data-part="switch"]');
    expect(track).toContain("border: var(--nail-border-control,");
    expect(track).toContain("background-color: var(--nail-color-bg);");
    expect(ruleBody('[data-part="switch.thumb"]')).toContain(
      "background-color: var(--nail-color-fg-muted);",
    );
    // On: the accent-fg thumb on the accent track.
    expect(
      ruleBody('[data-part="switch"][aria-checked="true"] [data-part="switch.thumb"]'),
    ).toContain("background-color: var(--nail-color-accent-fg);");
    expect(ruleBody('[data-part="switch"][aria-checked="true"]')).toContain(
      "background-color: var(--nail-color-accent);",
    );
  });

  it("rings the focused message list with the focus colour", () => {
    expect(ruleBody('[data-part="message-list"]:focus-visible')).toContain(
      "outline: 2px solid var(--nail-color-focus-ring)",
    );
  });

  it("aligns the reader's banners with the centred column", () => {
    const body = ruleBody(
      [
        "security-message",
        "security-warnings",
        "remote-content",
        "junk-verdict",
        "sync-message-body",
      ]
        .map((part) => `[data-part="${part}"]`)
        .join(", "),
    );
    expect(body).toContain("margin-inline: auto");
    expect(body).toContain("width: min(68ch, 100% - 2 * var(--nail-space-xxl))");
    // The reader fills its row, so an HTML body gets the rest of the pane.
    expect(ruleBody('[data-part="message-view"]')).toContain("height: 100%");
    expect(ruleBody('[data-part="message-view.body"]')).toContain("flex: 1 1 auto");
  });

  it("puts the attachment save status next to the size, not on a new card row", () => {
    expect(ruleBody('[data-part="attachment-card.text"]')).toContain(
      "grid-template-columns: auto minmax(0, 1fr)",
    );
    expect(ruleBody('[data-part="attachment-card.name"]')).toContain("grid-column: 1 / -1");
    const status = ruleBody('[data-part="attachment-card.status"]');
    expect(status).toContain("grid-column: 2");
    expect(status).toContain("white-space: nowrap");
  });

  it("sets the composer's Technology label like the Sign / Encrypt labels beside it (P1 verify)", () => {
    const label = ruleBody(
      '[data-part="security-compose.toggles"] > [data-part="form.field"] > [data-part="form.label"]',
    );
    expect(label).toContain("font-size: var(--nail-font-size-body)");
    expect(label).toContain("font-weight: var(--nail-font-weight-regular)");
  });

  it("hides the tag line in Compact", () => {
    expect(SKIN_CSS).toContain(
      ':root[data-nail-density="compact"] [data-part="message-list.row.tag-line"]',
    );
    expect(SKIN_CSS).toContain(
      '[data-part="app"][data-density="compact"] [data-part="message-list.row.tag-line"]',
    );
  });

  it("gives white text enough contrast on every avatar and file-type colour (WCAG AA)", () => {
    const fallbacks = [
      ...SKIN_CSS.matchAll(/--nail-color-(?:avatar-\d+|file-[a-z]+), (#[0-9a-f]{6})\)/g),
    ];
    expect(fallbacks.length).toBeGreaterThan(AVATAR_COLORS.length);
    for (const color of [...AVATAR_COLORS, ...fallbacks.map((m) => must(m[1]))]) {
      expect(contrast(color, "#ffffff")).toBeGreaterThanOrEqual(4.5);
    }
  });

  it("hashes addresses to stable palette slots", () => {
    expect(stableHash("ada@example.org")).toBe(stableHash("ada@example.org"));
    expect(colorSlot(" Ada@Example.org ")).toBe(colorSlot("ada@example.org"));
    const slots = new Set(
      ["a", "b", "c", "d", "e", "f", "g", "h", "i", "j"].map((x) => colorSlot(`${x}@x.org`)),
    );
    expect(slots.size).toBeGreaterThan(3);
    for (const slot of slots) expect(slot).toBeLessThan(AVATAR_COLORS.length);
  });
});

describe("conversations (P2 quick win, ADR 0148)", () => {
  const css = compileTheme(defaultTheme()).css;

  it("styles the reader's strip on tokens after the shared skin: no band, no scroll area", () => {
    const skin = css.indexOf(SKIN_CSS);
    const list = css.indexOf(MESSAGE_LIST_SKIN_CSS);
    const strip = css.indexOf(THREAD_VIEW_SKIN_CSS);
    expect(list).toBeGreaterThan(skin);
    expect(strip).toBeGreaterThan(list);
    // The default theme no longer styles the strip (it was a grey band of
    // at most 40vh with its own scroll bar).
    expect(css).not.toMatch(/\[data-part="thread-view"\] \{[^}]*40vh/);
    const strip0 = must(/\[data-part="thread-view"\] \{[^}]*\}/.exec(THREAD_VIEW_SKIN_CSS))[0];
    expect(strip0).toContain("background-color: var(--nail-color-bg);");
    expect(strip0).not.toMatch(/overflow|max-height/);
    expect(THREAD_VIEW_SKIN_CSS).toContain("var(--nail-space-row)");
    expect(THREAD_VIEW_SKIN_CSS).toContain(
      "--nail-skin-avatar-sm: calc(var(--nail-size-avatar) * 28 / 34)",
    );
  });

  it("draws the strip's avatars in the avatar palette and hides an empty count pill", () => {
    expect(SKIN_CSS).toContain('[data-color="0"][data-part="thread-view.item.avatar"]');
    expect(SKIN_CSS).toContain('[data-color="7"][data-part="thread-view.card.avatar"]');
    expect(MESSAGE_LIST_SKIN_CSS).toContain(
      '[data-part="message-list.row.count"]:empty { display: none; }',
    );
  });
});

describe("conversations (P2 review fixes)", () => {
  /** The declarations of the first rule whose selector is exactly `selector`. */
  const rule = (css: string, selector: string) =>
    css
      .split("}")
      .map((block) => block.split("{"))
      .find(([sel = ""]) => sel.trim() === selector)?.[1] ?? "";

  it("never stretches the last open card, and keeps the quote toggle under the text", () => {
    // The last open card grew to the pane's height and the text grid shared
    // the extra height between the text and the quote block.
    expect(THREAD_VIEW_SKIN_CSS).not.toMatch(/:last-child\[data-state~="expanded"\]/);
    expect(
      rule(
        THREAD_VIEW_SKIN_CSS,
        '[data-part="thread-view.card.body"] [data-part="message-view.text"]',
      ),
    ).toContain("align-content: start;");
  });

  it("draws the fold as a line of its own between cards", () => {
    expect(THREAD_VIEW_SKIN_CSS).not.toContain(
      '[data-part="thread-view.card"] > [data-part="thread-view.fold"]',
    );
    expect(rule(THREAD_VIEW_SKIN_CSS, '[data-part="thread-view.fold-row"]')).toContain(
      "display: flex;",
    );
  });

  it("gives Design B child rows body-size text and padding from the row token", () => {
    const child = rule(MESSAGE_LIST_SKIN_CSS, '[data-part="message-list.child"]');
    expect(child).toContain("font-size: var(--nail-font-size-body);");
    expect(child).toContain("padding: calc(var(--nail-space-row) * 5 / 6) var(--nail-space-pane);");
  });
});

describe("density in the compiler (ADR 0102)", () => {
  const compactTheme = (): ResolvedTheme => ({ ...defaultTheme(), density: "compact" });

  it("puts the compact variant under data-nail-density, after the colour modes", () => {
    const css = compileTheme(defaultTheme()).css;
    const compact = css.indexOf(':root[data-nail-density="compact"] {');
    expect(compact).toBeGreaterThan(css.indexOf(':root[data-nail-mode="dark"]'));
    expect(css).not.toContain(':root[data-nail-mode="compact"]');
    const block = css.slice(compact, css.indexOf("}", compact));
    expect(block).toContain("--nail-space-row:");
    expect(block).not.toContain("color-scheme");
    expect(COMPACT_VARIANT).toBe("compact");
  });

  it("resolves token references with the compact values when the theme is compact", () => {
    const comfortable = defaultTheme();
    const row = (t: ResolvedTheme) => resolveTokenRefs("var(--nail-space-row)", t, "light");
    expect(row(compactTheme())).toBe(comfortable.modes.compact?.space?.row);
    expect(row(comfortable)).toBe(comfortable.tokens.space?.row);
  });

  it("sets data-nail-density on the document from the theme", () => {
    const { rerender } = render(<ThemeProvider theme={defaultTheme()}>{null}</ThemeProvider>);
    expect(document.documentElement.dataset.nailDensity).toBe("comfortable");
    rerender(<ThemeProvider theme={compactTheme()}>{null}</ThemeProvider>);
    expect(document.documentElement.dataset.nailDensity).toBe("compact");
  });
});

describe("lists of rows", () => {
  it("draws no bullets beside the key list's rows (P1 review)", () => {
    // The line of the rule whose selector list names the key list (jsdom
    // expands no `list-style` shorthand, so the rule itself is checked).
    const rule = BASE_CSS.split("\n").find((line) =>
      line.includes('[data-part="security-keys.list"]'),
    );
    expect(rule).toMatch(/\{ list-style: none;/);
  });
});
