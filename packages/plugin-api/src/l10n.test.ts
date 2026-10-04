import { describe, expect, it } from "vitest";

import { createLocalizer, FluentCatalog, toL10nArgs } from "./l10n";

const EN = `
# A comment
-brand = Nail
hello = Hello, { $name }!
about = About { -brand }
quoted = Braces: {"{"} and {"}"}
long =
    First line
    second line
items = { $count ->
    [0] No items
    [one] One item
   *[other] { $count } items
}
title = Title
    .tooltip = An attribute, ignored
after = After the attribute
broken = { $oops
only-en = English only
`;

// German catalog. spellchecker:off
const DE = `
hello = Hallo, { $name }!
items = { $count ->
    [one] Ein Eintrag
   *[other] { $count } Einträge
}
`;
// spellchecker:on

describe("FluentCatalog", () => {
  const catalog = new FluentCatalog(EN);

  it("formats messages, terms, literals and multi-line values", () => {
    expect(catalog.format("hello", { name: "Ada" })).toBe("Hello, Ada!");
    expect(catalog.format("about")).toBe("About Nail");
    expect(catalog.format("quoted")).toBe("Braces: { and }");
    expect(catalog.format("long")).toBe("First line\nsecond line");
    expect(catalog.format("hello")).toBe("Hello, {$name}!");
  });

  it("selects variants by exact key, plural category and default", () => {
    expect(catalog.format("items", { count: 0 })).toBe("No items");
    expect(catalog.format("items", { count: 1 })).toBe("One item");
    expect(catalog.format("items", { count: 5 })).toBe("5 items");
    expect(catalog.format("items", {})).toBe("{$count} items");
  });

  it("ignores attributes, skips broken entries and hides terms", () => {
    expect(catalog.format("title")).toBe("Title");
    expect(catalog.format("after")).toBe("After the attribute");
    expect(catalog.has("broken")).toBe(false);
    expect(catalog.has("-brand")).toBe(false);
    expect(catalog.format("missing")).toBeUndefined();
    expect(catalog.ids()).toContain("only-en");
  });
});

describe("createLocalizer", () => {
  it("falls back per message to English, then to the id", () => {
    const l10n = createLocalizer("de-AT", { en: EN, de: DE });
    expect(l10n.locale).toBe("de-AT");
    expect(l10n.t("hello", { name: "Ada" })).toBe("Hallo, Ada!");
    expect(l10n.t("items", { count: 1 })).toBe("Ein Eintrag"); // spellchecker:disable-line
    expect(l10n.t("items", { count: 3 })).toBe("3 Einträge");
    expect(l10n.t("only-en")).toBe("English only");
    expect(l10n.t("nowhere")).toBe("nowhere");
    expect(l10n.has("only-en")).toBe(true);
    expect(l10n.has("nowhere")).toBe(false);
  });

  it("uses English when the language has no catalog", () => {
    const l10n = createLocalizer("fr", { en: EN });
    expect(l10n.t("hello", { name: "Lou" })).toBe("Hello, Lou!");
    expect(createLocalizer("en", {}).t("x")).toBe("x");
  });

  it("turns JSON values into arguments", () => {
    expect(toL10nArgs({ a: 1, b: "x", c: null, d: [1], e: { k: true } })).toEqual({
      a: 1,
      b: "x",
      c: null,
      d: "[1]",
      e: '{"k":true}',
    });
  });
});
