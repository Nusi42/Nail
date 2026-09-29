/**
 * Owns the single `<style>` element of the app. The ThemeProvider is its only
 * user; components never write CSS themselves.
 *
 * Besides the compiled theme, the sheet holds *geometry rules*: per-instance
 * sizes that come from data rather than from the theme (split pane sizes from
 * the layout and the user's dragging, virtual-list spacer heights). They are
 * CSSOM rules keyed on `[data-geom="<id>"]`, updated in place without
 * re-parsing the theme, and re-inserted after each theme swap.
 */
import { cssString, isSafeValue } from "./compile";

export type Declarations = Record<string, string>;

interface GeometryRule {
  declarations: Declarations;
  rule: CSSStyleRule | null;
}

const PROPERTY_RE = /^-?[a-z][a-z0-9-]*$/;

/** Selector of a geometry rule. */
export function geometrySelector(id: string): string {
  return `[data-geom=${cssString(id)}]`;
}

export class StyleManager {
  private readonly doc: Document;
  private el: HTMLStyleElement | null = null;
  private readonly geometry = new Map<string, GeometryRule>();
  private themeCss = "";

  /** Creates the manager; the `<style>` element is added on first use. */
  constructor(doc: Document = document) {
    this.doc = doc;
  }

  /** The managed `<style>` element (created on demand). */
  get element(): HTMLStyleElement {
    if (!this.el || !this.el.isConnected) {
      const el = this.doc.createElement("style");
      el.dataset.nailTheme = "";
      el.textContent = this.themeCss;
      this.doc.head.appendChild(el);
      this.el = el;
      for (const [id, entry] of this.geometry) {
        entry.rule = this.insert(id, entry.declarations);
      }
    }
    return this.el;
  }

  /** Replaces the compiled theme; geometry rules survive. */
  setTheme(css: string): void {
    if (css === this.themeCss && this.el?.isConnected) return;
    this.themeCss = css;
    this.element.textContent = css;
    for (const [id, entry] of this.geometry) {
      entry.rule = this.insert(id, entry.declarations);
    }
  }

  /** Sets (or replaces) the geometry rule for element `data-geom=id`. */
  setGeometry(id: string, declarations: Declarations): void {
    const clean: Declarations = {};
    for (const [property, value] of Object.entries(declarations)) {
      if (PROPERTY_RE.test(property) && isSafeValue(value)) {
        clean[property] = value;
      }
    }
    const existing = this.geometry.get(id);
    if (existing?.rule) {
      const style = existing.rule.style;
      for (const property of Object.keys(existing.declarations)) {
        if (!(property in clean)) style.removeProperty(property);
      }
      for (const [property, value] of Object.entries(clean)) {
        style.setProperty(property, value);
      }
      existing.declarations = clean;
      return;
    }
    const entry: GeometryRule = { declarations: clean, rule: null };
    this.geometry.set(id, entry);
    if (this.el?.isConnected) {
      entry.rule = this.insert(id, clean);
    } else {
      void this.element; // creating the element inserts all geometry rules
    }
  }

  /** Removes the geometry rule for `id`. */
  removeGeometry(id: string): void {
    const entry = this.geometry.get(id);
    this.geometry.delete(id);
    const sheet = this.el?.sheet;
    if (!entry?.rule || !sheet) return;
    const index = Array.prototype.indexOf.call(sheet.cssRules, entry.rule);
    if (index >= 0) sheet.deleteRule(index);
  }

  /** Current geometry declarations for `id` (tests and the inspector). */
  geometryOf(id: string): Declarations | undefined {
    return this.geometry.get(id)?.declarations;
  }

  /** The compiled theme text currently applied. */
  get css(): string {
    return this.themeCss;
  }

  /** Removes the `<style>` element; geometry rules are kept for a re-attach. */
  dispose(): void {
    this.el?.remove();
    this.el = null;
    for (const entry of this.geometry.values()) {
      entry.rule = null;
    }
  }

  private insert(id: string, declarations: Declarations): CSSStyleRule | null {
    const sheet = this.el?.sheet;
    if (!sheet) return null;
    const body = Object.entries(declarations)
      .map(([p, v]) => `${p}: ${v};`)
      .join(" ");
    try {
      const index = sheet.insertRule(`${geometrySelector(id)} { ${body} }`, sheet.cssRules.length);
      return sheet.cssRules[index] as CSSStyleRule;
    } catch (error) {
      console.warn("nail: rejected geometry rule", id, error);
      return null;
    }
  }
}
