/**
 * The PIM skin (WP164): safe, parseable CSS on tokens only, emitted by the
 * theme compiler after the shared skin and before the theme's part rules,
 * styling the redesigned parts (one-line toolbar, segmented view switch,
 * event cards, calendar list, contacts, tasks) — and keeping the calendar
 * colour bar of event cards in High contrast.
 */
import type { ResolvedTheme } from "../../api/generated";
import { defaultTheme } from "../../api/mockData";
import { compileTheme } from "../../theme/compile";
import { AVATAR_COLORS, SKIN_CSS } from "../../theme/skin";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { PIM_SKIN_CSS, PIM_SKIN_PARTS } from "./skin";

/** The top-level rules of a stylesheet text (brace depth 0 → 0). */
function topLevelRules(css: string): string[] {
  const rules: string[] = [];
  let depth = 0;
  let start = 0;
  for (let i = 0; i < css.length; i++) {
    if (css[i] === "{") depth++;
    if (css[i] === "}") {
      depth--;
      if (depth === 0) {
        rules.push(css.slice(start, i + 1).trim());
        start = i + 1;
      }
    }
  }
  expect(css.slice(start).trim()).toBe("");
  expect(depth).toBe(0);
  return rules;
}

/** The declarations block of the first rule whose selector is exactly `selector`. */
function ruleOf(selector: string): string {
  const rule = topLevelRules(PIM_SKIN_CSS).find((r) => r.startsWith(`${selector} {`));
  if (!rule) throw new Error(`no rule for ${selector}`);
  return rule;
}

describe("PIM skin", () => {
  it("is safe and every rule parses", () => {
    expect(PIM_SKIN_CSS).not.toMatch(/url\s*\(|@import|expression\s*\(|javascript:/i);
    const sheet = new CSSStyleSheet();
    const rules = topLevelRules(PIM_SKIN_CSS);
    expect(rules.length).toBeGreaterThan(80);
    for (const rule of rules) {
      expect(() => sheet.insertRule(rule, sheet.cssRules.length), rule).not.toThrow();
    }
    expect(sheet.cssRules.length).toBe(rules.length);
  });

  it("takes colours only from tokens (literal colours only as token fallbacks)", () => {
    const withoutFallbacks = PIM_SKIN_CSS.replace(/var\(--nail-[a-z0-9-]+, #[0-9a-f]{3,8}\)/gi, "");
    expect(withoutFallbacks).not.toMatch(/#[0-9a-f]{3,8}\b/i);
    expect(withoutFallbacks).not.toMatch(/\brgb\(|\bhsl\(/i);
  });

  it("sits after the shared skin and before the theme's part rules", () => {
    const theme: ResolvedTheme = {
      ...defaultTheme(),
      parts: [
        {
          selector: "calendar.toolbar",
          states: [],
          conditions: [],
          ancestors: [],
          declarations: { "background-color": "red" },
          source: null,
        },
      ],
    };
    const css = compileTheme(theme).css;
    const shared = css.indexOf(SKIN_CSS);
    const pim = css.indexOf(PIM_SKIN_CSS);
    const rule = css.indexOf('[data-part="calendar.toolbar"] {\n  background-color: red;');
    expect(shared).toBeGreaterThan(0);
    expect(pim).toBeGreaterThan(shared);
    expect(rule).toBeGreaterThan(pim);
    expect(compileTheme(theme).warnings).toEqual([]);
  });

  it("keeps the calendar toolbar on one line and cuts the range", () => {
    expect(ruleOf('[data-part="calendar.toolbar"]')).toMatch(/flex-wrap: nowrap;/);
    const title = ruleOf('[data-part="calendar.title"]');
    expect(title).toMatch(/white-space: nowrap;/);
    expect(title).toMatch(/text-overflow: ellipsis;/);
    expect(title).toMatch(/min-width: 0;/);
    expect(PIM_SKIN_CSS).toContain('[data-part="segmented"][data-field="calendar-view"]');
  });

  it("writes card text in the theme's fg, never a muted colour", () => {
    for (const part of ["event-card.title", "event-card.time", "event-card.location"]) {
      const rule = ruleOf(`[data-part="${part}"]`);
      expect(rule, part).toMatch(/color: inherit;/);
      expect(rule, part).not.toMatch(/fg-muted/);
    }
    for (const part of ["calendar.event", "calendar.chip", "calendar.agenda.event"]) {
      expect(PIM_SKIN_CSS).toContain(`[data-part="${part}"]`);
    }
  });

  it("drops the tint in High contrast but keeps the calendar colour bar", () => {
    const rule = topLevelRules(PIM_SKIN_CSS).find((r) =>
      r.startsWith(':root[data-nail-mode="high-contrast"] [data-part="calendar.event"]'),
    );
    expect(rule).toBeDefined();
    expect(rule).toMatch(/background-color: var\(--nail-color-bg\);/);
    expect(rule).toMatch(/border-left-width: 6px;/);
    // No `border:` / `border-left:` shorthand that would reset the bar's colour.
    expect(rule).not.toMatch(/border(-left)?:\s/);
    expect(rule).not.toMatch(/border-left-color/);
  });

  it("styles the calendar list's colour squares, menus and the colour choice", () => {
    // Unchecked squares are hollow: this rule outranks the colour's geometry rule.
    expect(PIM_SKIN_CSS).toContain(
      '[data-part="calendar-list.toggle"]:not(:checked) { background-color: transparent; }',
    );
    for (const part of [
      "calendar-list.add",
      "calendar-list.menu",
      "calendar-list.source.header",
      "context-menu.item.swatch",
      "pim.colors.option",
      "pim.calendar-dialog",
      "mini-month.grid.day",
      "task-list.due",
      "contact-card.avatar",
    ]) {
      expect(PIM_SKIN_CSS).toContain(`[data-part="${part}"]`);
    }
    for (let i = 0; i < AVATAR_COLORS.length; i++) {
      expect(PIM_SKIN_CSS).toContain(
        `[data-color="${String(i)}"][data-part="contact-list.avatar"]`,
      );
    }
  });

  it("resets the browser look of the buttons it styles (M5b review: grey day heads in dark mode)", () => {
    for (const part of ["calendar.day-head", "task-list.title"]) {
      const rule = ruleOf(`[data-part="${part}"]`);
      expect(rule, part).toMatch(/background-color: transparent/);
      expect(rule, part).toMatch(/border: 0/);
    }
  });

  it("keeps the avatar's own text colour in a selected contact row", () => {
    expect(PIM_SKIN_CSS).not.toContain(
      '[data-part="contact-list.item"][aria-selected="true"] > * ',
    );
    expect(PIM_SKIN_CSS).toContain(
      '[data-part="contact-list.item"][aria-selected="true"] > :not([data-part="contact-list.avatar"]) { color: inherit; }',
    );
  });

  it("lists the parts it styles, sorted and without duplicates", () => {
    expect([...PIM_SKIN_PARTS].sort()).toEqual(PIM_SKIN_PARTS);
    expect(new Set(PIM_SKIN_PARTS).size).toBe(PIM_SKIN_PARTS.length);
    for (const part of [
      "calendar.toolbar",
      "calendar.title",
      "event-card.title",
      "task-list.item",
    ]) {
      expect(PIM_SKIN_PARTS).toContain(part);
    }
  });

  describe("M5b review: the M3a rules that left defaults/theme.kdl", () => {
    it("stacks the weekday above the date in the week's day heads", () => {
      const rule = ruleOf('[data-part="calendar.day-head"]');
      expect(rule).toMatch(/display: flex;/);
      expect(rule).toMatch(/flex-direction: column;/);
      expect(rule).toMatch(/align-items: center;/);
    });

    it("gives the current-time line a height", () => {
      expect(ruleOf('[data-part="calendar.now"]')).toMatch(/height: 2px;/);
    });

    it("draws the month grid: cell lines, height, outside days, today, drop target, focus", () => {
      expect(ruleOf('[data-part="calendar.month"]')).toMatch(/height: 100%;/);
      const day = ruleOf('[data-part="calendar.month.day"]');
      expect(day).toMatch(/border: 1px solid var\(--nail-color-border\);/);
      expect(day).toMatch(/height: /);
      expect(day).toMatch(/overflow: hidden;/);
      for (const state of ["outside", "today", "drop-target"]) {
        expect(PIM_SKIN_CSS).toContain(`[data-part="calendar.month.day"][data-state~="${state}"]`);
      }
      expect(ruleOf('[data-part="calendar.month.day"]:focus-visible')).toMatch(
        /outline: 2px solid var\(--nail-color-focus-ring\);/,
      );
      expect(ruleOf('[data-part="calendar.month.date"]')).toMatch(/display: block;/);
      // Chips are cut at the cell's edge instead of running into the next day.
      expect(ruleOf('[data-part="calendar.month.events"] > *')).toMatch(/text-overflow: ellipsis;/);
      expect(ruleOf('[data-part="calendar.month.item"] > [data-part="calendar.chip"]')).toMatch(
        /max-width: 100%;/,
      );
    });

    it("indents subtasks, strikes done tasks, marks the current one", () => {
      expect(ruleOf('[data-part="task-list.item"][data-level="2"]')).toMatch(/padding-left/);
      expect(ruleOf('[data-part="task-list.item"][data-level="3"]')).toMatch(/padding-left/);
      expect(
        ruleOf('[data-part="task-list.item"][data-state~="done"] [data-part="task-list.title"]'),
      ).toMatch(/text-decoration: line-through;/);
      expect(ruleOf('[data-part="task-list.item"][data-state~="current"]')).toMatch(
        /background-color/,
      );
    });

    it("sizes the contact card's avatar", () => {
      const rule = ruleOf('[data-part="contact-card.avatar"]');
      expect(rule).toMatch(/width: 72px;/);
      expect(rule).toMatch(/height: 72px;/);
    });

    it("leaves no rule in defaults/theme.kdl that would override a part it styles", () => {
      const theme = readFileSync(resolve(process.cwd(), "../defaults/theme.kdl"), "utf8");
      const themed = new Set(
        [...theme.matchAll(/^\s*part "([a-z0-9.-]+)/gm)].map((m) => m[1] ?? ""),
      );
      // `calendar` itself: its structure (a column) comes from parts.ts.
      const clash = PIM_SKIN_PARTS.filter((part) => themed.has(part));
      expect(clash).toEqual([]);
      expect(theme).not.toMatch(/part "calendar" \{[^}]*display/);
    });
  });
});
