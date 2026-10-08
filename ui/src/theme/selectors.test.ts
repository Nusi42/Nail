import { HC, hc, p, ps, pss } from "./selectors";

describe("skin selectors", () => {
  it("select parts, lists of parts and suffixed parts", () => {
    expect(p("toolbar.button")).toBe('[data-part="toolbar.button"]');
    expect(ps("a", "b.c")).toBe('[data-part="a"], [data-part="b.c"]');
    expect(pss(":hover", "a", "b")).toBe('[data-part="a"]:hover, [data-part="b"]:hover');
  });

  it("select parts under High contrast", () => {
    expect(HC).toBe(':root[data-nail-mode="high-contrast"]');
    expect(hc("a", "b")).toBe(`${HC} [data-part="a"], ${HC} [data-part="b"]`);
  });
});
