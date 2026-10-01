import { describe, expect, it } from "vitest";
import type { GmailLabel } from "../../api/generated";
import {
  categoryLabel,
  categoryOfQuery,
  categoryQuery,
  isWithin,
  labelIssues,
  labelQuery,
  labelTree,
  leafOf,
  listed,
  parentOf,
  pruneTree,
} from "./model";
import { GMAIL_PALETTE, LABEL_COLORS, inPalette, paletteName } from "./palette";
import { GMAIL_BASE_CSS } from "./parts";

const label = (name: string, extra: Partial<GmailLabel> = {}): GmailLabel => ({
  account_id: "gm",
  id: `L-${name}`,
  name,
  kind: "user",
  color: null,
  list_visibility: "show",
  message_visibility: "show",
  tag: name,
  folder_id: null,
  total: 0,
  unread: 0,
  sync: "synced",
  ...extra,
});

describe("gmail model", () => {
  it("builds a sorted tree, with levels that have no label of their own", () => {
    const tree = labelTree([
      label("Work/Projects"),
      label("Travel"),
      label("Archive/2025/Q1"),
      label("INBOX", { kind: "system" }),
      label("Work"),
    ]);
    expect(tree.map((n) => n.name)).toEqual(["Archive", "Travel", "Work"]);
    expect(tree[0]?.label).toBeNull();
    expect(tree[0]?.children[0]?.children[0]?.path).toBe("Archive/2025/Q1");
    expect(tree[2]?.children[0]?.label?.name).toBe("Work/Projects");
  });

  it("hides labels by their list visibility and prunes empty levels", () => {
    const hidden = label("Old", { list_visibility: "hide" });
    const unreadOnly = label("Follow up", { list_visibility: "show-if-unread" });
    expect(listed(hidden, false)).toBe(false);
    expect(listed(hidden, true)).toBe(true);
    expect(listed(unreadOnly, false)).toBe(false);
    expect(listed({ ...unreadOnly, unread: 2 }, false)).toBe(true);
    const tree = pruneTree(labelTree([hidden, label("Old/Keep"), unreadOnly]), false);
    expect(tree.map((n) => n.name)).toEqual(["Old"]);
    expect(tree[0]?.children.map((n) => n.name)).toEqual(["Keep"]);
  });

  it("mirrors the core's label issues", () => {
    const s = (name: string) => ({
      name,
      color: null,
      list_visibility: "show" as const,
      message_visibility: "show" as const,
    });
    expect(labelIssues(s(""))).toEqual(["missing-name"]);
    expect(labelIssues(s("starred"))).toEqual(["invalid-name"]);
    expect(labelIssues(s("/x"))).toEqual(["invalid-name"]);
    expect(labelIssues(s("x/"))).toEqual(["invalid-name"]);
    expect(labelIssues(s("a\u0007b"))).toEqual(["invalid-name"]);
    expect(labelIssues(s("Work"), [label("work")])).toEqual(["duplicate-name"]);
    expect(labelIssues({ ...s("ok"), color: { background: "#fff", text: "#000000" } })).toEqual([
      "invalid-color",
    ]);
    expect(labelIssues(s("Fine/Name"))).toEqual([]);
  });

  it("builds the label and category queries", () => {
    expect(labelQuery("Travel")).toBe("tag:Travel");
    expect(labelQuery("Follow up")).toBe('tag:"Follow up"');
    expect(categoryQuery("social")).toBe("category:social");
    expect(categoryOfQuery("category:Social")).toBe("social");
    expect(categoryOfQuery("category:social from:x")).toBeNull();
    expect(categoryOfQuery("category:nope")).toBeNull();
    expect(categoryOfQuery(null)).toBeNull();
    expect(categoryLabel("primary")).toBe("CATEGORY_PERSONAL");
    expect(categoryLabel("forums")).toBe("CATEGORY_FORUMS");
  });

  it("splits names", () => {
    expect(parentOf("Work/Projects/Nail")).toBe("Work/Projects");
    expect(parentOf("Work")).toBeNull();
    expect(leafOf("Work/Projects")).toBe("Projects");
    expect(isWithin("Work/Projects", "Work")).toBe(true);
    expect(isWithin("Workshop", "Work")).toBe(false);
  });

  it("offers only colours Gmail accepts, each name once", () => {
    expect(GMAIL_PALETTE.size).toBe(102);
    for (const p of LABEL_COLORS) expect(inPalette(p.color), p.name).toBe(true);
    expect(new Set(LABEL_COLORS.map((p) => p.name)).size).toBe(LABEL_COLORS.length);
    expect(inPalette({ background: "#123456", text: "#ffffff" })).toBe(false);
    expect(paletteName({ background: "#FB4C2F", text: "#FFFFFF" })).toBe("red");
    expect(paletteName({ background: "#123456", text: "#ffffff" })).toBeNull();
    expect(paletteName(null)).toBeNull();
  });

  it("base CSS is safe and parses", () => {
    expect(GMAIL_BASE_CSS).not.toMatch(/url\s*\(|@import|expression\s*\(|javascript:/i);
    const sheet = new CSSStyleSheet();
    const rules = GMAIL_BASE_CSS.split("\n").filter((line) => line.trim() !== "");
    for (const rule of rules) {
      expect(() => sheet.insertRule(rule, sheet.cssRules.length), rule).not.toThrow();
    }
  });
});
