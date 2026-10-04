import { describe, expect, it } from "vitest";
import { createMockData } from "../../api/mockData";
import { applyPatch } from "./mock";

// M2c (ADR 0043): the toolbar and key-binding editors restore defaults with
// `reset-toolbar` / `reset-key-binding`; the mock core applies them like
// nail-config's write-back.
describe("reset patches", () => {
  it("restores a toolbar and a key binding from the default layout", () => {
    const layout = createMockData().layout;
    const defaults = structuredClone(layout);
    const [name] = Object.keys(layout.toolbars);
    if (name === undefined) throw new Error("the default layout has no toolbar");
    expect(applyPatch(layout, { op: "toolbar-order", toolbar: name, items: [] })).toBeNull();
    const binding = defaults.keys[0];
    if (!binding) throw new Error("the default layout has no key binding");
    expect(
      applyPatch(layout, { op: "remove-key-binding", context: binding.context, key: binding.key }),
    ).toBeNull();
    expect(layout.toolbars[name]).toEqual([]);
    expect(layout.keys).not.toContainEqual(binding);

    expect(applyPatch(layout, { op: "reset-toolbar", toolbar: name })).toBeNull();
    expect(
      applyPatch(layout, {
        op: "reset-key-binding",
        context: binding.context,
        key: binding.key.toUpperCase(),
      }),
    ).toBeNull();
    expect(layout.toolbars[name]).toEqual(defaults.toolbars[name]);
    expect(layout.keys).toContainEqual(binding);

    layout.toolbars.mine = [];
    expect(applyPatch(layout, { op: "reset-toolbar", toolbar: "mine" })).toBeNull();
    expect(layout.toolbars.mine).toBeUndefined();
  });
});
