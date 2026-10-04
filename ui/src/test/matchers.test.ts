import {
  arrayContaining,
  notArrayContaining,
  objectContaining,
  stringContaining,
  stringMatching,
} from "./matchers";

describe("typed asymmetric matchers", () => {
  it("match like Vitest's own matchers", () => {
    expect({ local: "2026-10-04T11:00:00", tz: "UTC" }).toEqual({
      local: stringMatching(/T11:00:00$/),
      tz: "UTC",
    });
    expect({ subject: "Lunch on Friday?" }).toEqual({ subject: stringContaining("Friday") });
    expect({ args: { id: "m1", extra: 1 } }).toEqual({ args: objectContaining({ id: "m1" }) });
    expect({ items: ["a", "b", "c"] }).toEqual({ items: arrayContaining(["c", "a"]) });
    expect({ items: ["a", "b"] }).toEqual({ items: notArrayContaining(["z"]) });
    expect({ items: ["a", "z"] }).not.toEqual({ items: notArrayContaining(["z"]) });
  });
});
