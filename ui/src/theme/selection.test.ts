import { defaultTheme } from "../api/mockData";
import { uiSources } from "../test/sources";
import { BASE_CSS, compileTheme } from "./compile";
import { isSelectable, SELECTABLE_PARTS, SELECTION_CSS } from "./selection";
import { SKIN_CSS } from "./skin";

/** An element of `part` inside `parent` (or the body). */
function el(tag: string, part: string | null, parent: Element = document.body): HTMLElement {
  const element = document.createElement(tag);
  if (part !== null) element.setAttribute("data-part", part);
  parent.append(element);
  return element;
}

describe("text selection (ADR 0182)", () => {
  afterEach(() => {
    document.body.replaceChildren();
  });

  it("makes the page unselectable and the user's data selectable, before skins and themes", () => {
    const css = compileTheme(defaultTheme()).css;
    const base = css.indexOf(BASE_CSS);
    const selection = css.indexOf(SELECTION_CSS);
    expect(base).toBeGreaterThanOrEqual(0);
    expect(selection).toBeGreaterThan(base);
    expect(css.indexOf(SKIN_CSS)).toBeGreaterThan(selection);
    expect(SELECTION_CSS).toMatch(/^body \{ user-select: none; \}/);
    expect(SELECTION_CSS).toContain('[data-part="message-view.header.value"], ');
    expect(SELECTION_CSS).toMatch(
      /textarea, \[contenteditable\]:not\(\[contenteditable="false"\]\) \{ user-select: text; \}$/,
    );
  });

  it("names only parts the UI renders", () => {
    const missing = SELECTABLE_PARTS.filter((part) => !uiSources.includes(`"${part}"`));
    expect(missing).toEqual([]);
    expect(new Set(SELECTABLE_PARTS).size).toBe(SELECTABLE_PARTS.length);
  });

  it("tells selectable text from chrome", () => {
    const header = el("header", "message-view.header");
    const label = el("span", "message-view.header.label", header);
    const value = el("span", "message-view.header.value", header);
    const address = el("a", null, value);
    const body = el("div", "thread-view.body");
    const row = el("div", "message-list.row");
    const button = el("button", "folder-tree.compose");
    const input = el("input", "form.input");
    const textarea = el("textarea", null);
    const editor = el("div", null);
    editor.setAttribute("contenteditable", "true");
    const notEditable = el("div", null);
    notEditable.setAttribute("contenteditable", "false");
    for (const selectable of [value, address, body, input, textarea, editor]) {
      expect(isSelectable(selectable), selectable.outerHTML).toBe(true);
    }
    for (const chrome of [header, label, row, button, notEditable, null]) {
      expect(isSelectable(chrome), chrome?.outerHTML ?? "null").toBe(false);
    }
  });
});
