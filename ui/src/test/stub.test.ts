import { must } from "./must";
import { stubProperties } from "./stub";

describe("stubProperties", () => {
  it("replaces existing properties and restores them", () => {
    const target = { greet: () => "hello" };
    const restore = stubProperties(target, { greet: () => "stubbed" });
    expect(target.greet()).toBe("stubbed");
    restore();
    expect(target.greet()).toBe("hello");
  });

  it("removes properties that did not exist before", () => {
    const target: { extra?: number } = {};
    const restore = stubProperties(target, { extra: 1 });
    expect(target.extra).toBe(1);
    restore();
    expect("extra" in target).toBe(false);
  });
});

describe("must", () => {
  it("returns present values and fails on missing ones", () => {
    expect(must(0)).toBe(0);
    expect(() => {
      must(null, "row");
    }).toThrow("expected a row, got null");
    expect(() => {
      must(undefined);
    }).toThrow("expected a value, got undefined");
  });
});
