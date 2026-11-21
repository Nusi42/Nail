import { mergeFeatures } from "../registry";
import { literalMenuLabel, menuSourceFor, type MenuSource } from "./sources";

describe("menu sources (R2 contract, ADR 0222)", () => {
  const source: MenuSource = { load: () => Promise.resolve([]) };

  it("finds the source of an action and nothing else", () => {
    expect(menuSourceFor({ "rules.run-menu": source }, "rules.run-menu")).toBe(source);
    expect(menuSourceFor({ "rules.run-menu": source }, "toString")).toBeNull();
    expect(literalMenuLabel("Home")).toBe("Home");
    expect(literalMenuLabel("@home")).not.toBe("@home");
  });

  it("merges the features' sources and refuses one registered twice", () => {
    const slot = { widgets: {}, actions: {}, menuSources: { "x.menu": source } };
    const merged = mergeFeatures({ widgets: {}, actions: {} }, { a: slot });
    expect(merged.menuSources).toEqual({ "x.menu": source });
    expect(() => mergeFeatures({ widgets: {}, actions: {} }, { a: slot, b: slot })).toThrow(
      /menu source "x.menu"/,
    );
  });
});
