import type { ResolvedTheme } from "../api/generated";
import { defaultTheme } from "../api/mockData";
import { APPEARANCE_SKIN_CSS } from "../features/appearance/skin";
import { compileTheme } from "./compile";
import { SKIN_CSS } from "./skin";
import {
  DIALOG_BUTTON_ROWS,
  FIELD_ROWS,
  FIELDS_CONTAINER,
  GROWING_DIALOGS,
  INLINE_FIELD_ROWS,
  RESPONSIVE_SKIN_CSS,
  TWO_PANES_NARROW,
} from "./skinResponsive";
import { uiSources } from "../test/sources";

/** Every component's source, to check that the parts named here exist. */
const sources = uiSources;

/** The rule bodies whose selector list contains `selector`. */
const rulesFor = (selector: string): string[] =>
  RESPONSIVE_SKIN_CSS.split("}")
    .map((block) => block.split("{"))
    .filter(([selectors = ""]) => selectors.includes(selector))
    .map(([, body = ""]) => body);

describe("P1 responsive skin (theme/skinResponsive.ts)", () => {
  it("comes after the shared and feature skins and before the theme's part rules", () => {
    const theme: ResolvedTheme = {
      ...defaultTheme(),
      parts: [
        {
          selector: "form.row",
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
    const feature = css.indexOf(APPEARANCE_SKIN_CSS);
    const responsive = css.indexOf(RESPONSIVE_SKIN_CSS);
    const rule = css.indexOf('[data-part="form.row"] {\n  color: red;');
    expect(skin).toBeGreaterThanOrEqual(0);
    expect(feature).toBeGreaterThan(skin);
    expect(responsive).toBeGreaterThan(feature);
    expect(rule).toBeGreaterThan(responsive);
  });

  it("uses tokens and data-part selectors only", () => {
    expect(RESPONSIVE_SKIN_CSS.match(/#[0-9a-f]{3,8}\b/gi) ?? []).toEqual([]);
    expect(RESPONSIVE_SKIN_CSS).not.toMatch(/(^|\s)\.[a-z]/m);
    expect(RESPONSIVE_SKIN_CSS.split("{").length).toBe(RESPONSIVE_SKIN_CSS.split("}").length);
    // Colours come from the token schema.
    expect(RESPONSIVE_SKIN_CSS).not.toMatch(/\b(rgb|hsl)a?\(/);
  });

  it("names only parts the components emit", () => {
    const named = [...RESPONSIVE_SKIN_CSS.matchAll(/data-part="([^"]+)"/g)].map((m) => m[1] ?? "");
    const missing = [...new Set(named)].filter((part) => !sources.includes(`"${part}"`));
    expect(missing).toEqual([]);
    for (const part of [
      ...DIALOG_BUTTON_ROWS,
      ...GROWING_DIALOGS,
      ...INLINE_FIELD_ROWS,
      ...FIELD_ROWS,
    ]) {
      expect(sources, part).toContain(`"${part}"`);
    }
  });

  it("shares a row of controls between its fields; a sized control keeps its width (ADR 0160)", () => {
    const [rows] = rulesFor(':is([data-part="rules.add"]');
    expect(rows).toBeDefined();
    expect(rows).toMatch(/align-items: flex-end;/);
    for (const part of FIELD_ROWS) {
      expect(RESPONSIVE_SKIN_CSS, part).toContain(`[data-part="${part}"]`);
    }
    // A control marked `data-width` keeps the width the shared skin gives it.
    expect(RESPONSIVE_SKIN_CSS).toContain(
      '[data-part="form.textarea"]):not([data-width]) {\n  flex: 1 1 8rem; min-width: 0;\n}',
    );
  });

  it("lets the window pad and the dialog inside shrink instead of scrolling sideways", () => {
    expect(RESPONSIVE_SKIN_CSS).toContain(
      `[data-part="window.body"] { container: ${FIELDS_CONTAINER} / inline-size; }`,
    );
    expect(RESPONSIVE_SKIN_CSS).toContain(
      `[data-part="window.body"] > [data-part] { min-width: 0; padding: 0; }`,
    );
    // Dialogs that cap their width grow with a wide window.
    for (const part of GROWING_DIALOGS) {
      expect(RESPONSIVE_SKIN_CSS).toContain(`[data-part="window.body"] > [data-part="${part}"]`);
    }
  });

  it("keeps each dialog's last button row at the bottom of its window", () => {
    for (const part of DIALOG_BUTTON_ROWS) {
      const [body] = rulesFor(`[data-part="window.body"] [data-part="${part}"]:last-child`);
      expect(body, part).toBeDefined();
      // It reaches into the window body's bottom padding and fills it, so
      // nothing scrolls by below it.
      expect(body).toMatch(/position: sticky; bottom: calc\(-1 \* var\(--nail-space-pane\)\)/);
      expect(body).toMatch(/padding-block: var\(--nail-space-sm\) var\(--nail-space-pane\)/);
      expect(body).toMatch(/background-color: var\(--nail-color-bg\)/);
      expect(body).toMatch(/flex-wrap: wrap/);
    }
    // Only the last row of a parent sticks (the diagnostics dialog has two).
    expect(RESPONSIVE_SKIN_CSS).not.toMatch(/\[data-part="wizard\.buttons"\] \{[^}]*sticky/);
  });

  it("wraps rows of buttons and keeps hints and labels from overflowing", () => {
    const [rows] = rulesFor(':is([data-part$=".actions"], [data-part$=".buttons"]');
    expect(rows).toMatch(/display: flex; flex-wrap: wrap;[^}]*gap: var\(--nail-space-sm\)/);
    expect(RESPONSIVE_SKIN_CSS).toMatch(
      /\[data-part="form.check.label"\], \[data-part="form.hint"\] \{ overflow-wrap: anywhere; \}/,
    );
    // The setup steps keep their numbers inside each step ("1. Address  2. Servers").
    expect(rulesFor('[data-part="account-setup.steps"]')[0]).toMatch(
      /flex-wrap: wrap;[^}]*list-style-position: inside/,
    );
  });

  it("stacks a list and its editor in a narrow card or window", () => {
    const narrow = RESPONSIVE_SKIN_CSS.slice(
      RESPONSIVE_SKIN_CSS.indexOf(
        `@container ${FIELDS_CONTAINER} (max-width: ${TWO_PANES_NARROW})`,
      ),
    );
    expect(narrow).toMatch(
      /:is\(\[data-part="account-settings"\], \[data-part="rules"\]\) \{\s*grid-template-columns: minmax\(0, 1fr\);/,
    );
    expect(narrow).toMatch(
      /\[data-part="account-settings.list"\] \{ flex-direction: row; flex-wrap: wrap;/,
    );
  });

  it("lays out inline field rows, view switchers and the key list", () => {
    for (const part of INLINE_FIELD_ROWS) {
      expect(RESPONSIVE_SKIN_CSS).toContain(`[data-part="${part}"]`);
    }
    const [switcher] = rulesFor(
      '[data-part="confirmation-center.view"], [data-part="rules.view"])[aria-pressed="true"]',
    );
    expect(switcher).toMatch(/font-weight: var\(--nail-font-weight-semibold\)/);
    // Key rows: aligned columns that the theme's flex row cannot undo.
    const [keys] = rulesFor('[data-part="security-keys.list"] [data-part="security-keys.key"]');
    expect(keys).toMatch(/display: grid; grid-template-columns: minmax\(0, 1fr\)/);
    expect(rulesFor('[data-part="security-key.list"]')[0]).toMatch(
      /grid-template-columns: max-content minmax\(0, 1fr\)/,
    );
  });

  it("says a valid configuration in the success colour and keeps the file input reachable", () => {
    const [valid] = rulesFor(
      '[data-part="config-errors"][data-state="valid"] > [data-part="config-errors.summary"]',
    );
    expect(valid).toMatch(/color: var\(--nail-color-success\)/);
    // Visually hidden, not display: none (the keyboard reaches "Load a file").
    const [input] = rulesFor(
      '[data-part="merge-dialog.upload"] > [data-part="merge-dialog.upload.input"]',
    );
    expect(input).toMatch(/display: block; position: absolute; width: 1px; height: 1px/);
    expect(input).toMatch(/clip-path: inset\(50%\)/);
  });

  it("starts the free/busy hour scale where the tracks start", () => {
    expect(rulesFor('[data-part="freebusy"] > [data-part="freebusy.scale"]')[0]).toMatch(
      /margin-inline-start: calc\(160px \+ var\(--nail-space-xs\)\)/,
    );
  });
});
