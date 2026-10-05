import { screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { RuleManagerView } from "../features/rules/RuleManager";
import { fastStylesActive, subjectParts, withoutFastStyles } from "./fastStyles";
import { createEnv, renderNode, renderWithProviders, settleQueries, widget } from "./harness";

describe("subjectParts", () => {
  it("reads the data-part a selector's subject requires", () => {
    expect(subjectParts('[data-part="a.b"]')).toEqual(["a.b"]);
    expect(subjectParts("[data-part='a']:hover::before")).toEqual(["a"]);
    expect(subjectParts("[data-part=a-b]")).toEqual(["a-b"]);
    expect(subjectParts('[data-part="a"] > [data-part="b"][data-state~="x"]')).toEqual(["b"]);
    expect(subjectParts(':root[data-nail-mode="dark"] [data-part="c"]:focus-visible')).toEqual([
      "c",
    ]);
    expect(subjectParts('[data-part="a"], [data-part="b"]:hover')).toEqual(["a", "b"]);
    expect(subjectParts('[data-part="a"]:not([data-part="b"])')).toEqual(["a"]);
  });

  it("does not filter selectors whose subject may lack the part", () => {
    // Some branch without a part, the part only on an ancestor, inside a
    // functional pseudo-class, another operator, or escapes.
    expect(subjectParts('[data-part="a"], button')).toBeNull();
    expect(subjectParts('[data-part="a"] span')).toBeNull();
    expect(subjectParts('[data-part="a"] + *')).toBeNull();
    expect(subjectParts(':is([data-part="a"], [data-part="b"])')).toBeNull();
    expect(subjectParts('[data-part^="a"]')).toBeNull();
    expect(subjectParts('[data-part~="a"]')).toBeNull();
    expect(subjectParts('[data-part="a" i]')).toBeNull();
    expect(subjectParts('[data-part="a\\"b"]')).toBeNull();
    expect(subjectParts(":root")).toBeNull();
    expect(subjectParts("[popover]:not(:popover-open):not(dialog[open])")).toBeNull();
  });
});

const PROPERTIES = [
  "display",
  "visibility",
  "pointer-events",
  "color",
  "background-color",
  "font-weight",
  "opacity",
];

/**
 * The computed style of the first element of each part under `root` (plus
 * the root): the declared properties and a few inherited ones. One element
 * per part keeps the slow, unfiltered pass short.
 */
function snapshot(root: Element): string[] {
  const seen = new Set<string>();
  const elements = [root, ...Array.from(root.querySelectorAll("[data-part]"))].filter((el) => {
    const part = el.getAttribute("data-part") ?? "";
    if (seen.has(part)) return false;
    seen.add(part);
    return true;
  });
  return elements.map((el) => {
    const style = getComputedStyle(el);
    const declared = Array.from(style).map((p) => `${p}=${style.getPropertyValue(p)}`);
    const inherited = PROPERTIES.map((p) => `${p}=${style.getPropertyValue(p)}`);
    return `${el.tagName} ${el.getAttribute("data-part") ?? ""}: ${[...declared, ...inherited].join("; ")}`;
  });
}

describe("fast computed styles (setup.ts)", () => {
  it("is installed in the jsdom suites", () => {
    // Fails when a jsdom update changes the internals the filter uses.
    expect(fastStylesActive()).toBe(true);
  });

  it("computes the same styles as jsdom on its own, with the theme applied", async () => {
    const env = createEnv();
    const user = userEvent.setup();
    const { container } = renderWithProviders(<RuleManagerView />, env);
    await user.click(await screen.findByRole("button", { name: "New rule" }));
    await screen.findByRole("form", { name: "Rule" });
    renderNode(widget("folder-tree"), env);
    await settleQueries(env);
    expect(document.querySelectorAll("style").length).toBeGreaterThan(0);

    const fast = snapshot(document.body);
    const slow = withoutFastStyles(() => snapshot(document.body));
    expect(fast.length).toBeGreaterThan(30);
    expect(fast).toEqual(slow);
    // The theme's part rules do reach the elements (not an empty comparison):
    // themed parts get padding, gaps, borders … from them.
    const themed = fast.filter((line) => /^\w+ [\w.-]+: .*(padding|gap|border)[\w-]*=/.test(line));
    expect(themed.length).toBeGreaterThan(15);
    expect(container.querySelector("[data-part]")).not.toBeNull();
  });
});
