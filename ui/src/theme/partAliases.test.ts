import { PART_ALIASES, aliasedPart, aliasedRule } from "./partAliases";

const ALIASES = { "thread-view.item": "thread-view.card" };

describe("renamed parts (P2, ADR 0148)", () => {
  it("maps an old part and its sub-parts to the new names, nothing else", () => {
    expect(aliasedPart("thread-view.item", ALIASES)).toBe("thread-view.card");
    expect(aliasedPart("thread-view.item.header", ALIASES)).toBe("thread-view.card.header");
    expect(aliasedPart("thread-view.items", ALIASES)).toBeNull();
    expect(aliasedPart("thread-view", ALIASES)).toBeNull();
  });

  it("renames a rule's selector and ancestors, or answers null", () => {
    const rule = {
      selector: "thread-view.item.from",
      ancestors: [{ selector: "thread-view.item", states: ["unread"], conditions: [] }],
    };
    expect(aliasedRule(rule, ALIASES)).toEqual({
      selector: "thread-view.card.from",
      ancestors: [{ selector: "thread-view.card", states: ["unread"], conditions: [] }],
    });
    expect(aliasedRule({ selector: "message-view", ancestors: [] }, ALIASES)).toBeNull();
  });

  it("has no aliases until the card feed replaces the strip", () => {
    expect(PART_ALIASES).toEqual({});
    expect(aliasedRule({ selector: "thread-view.item", ancestors: [] })).toBeNull();
  });
});
