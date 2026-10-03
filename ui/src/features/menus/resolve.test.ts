import { describe, expect, it } from "vitest";
import type { MenuItem, ResolvedLayout } from "../../api/generated";
import { contextMenuName, keyContextOf, resolveMenu, withoutDisabled } from "./resolve";

const item = (action: string): MenuItem => ({
  type: "action",
  action,
  args: [],
  label: null,
  icon: null,
});

function layout(menu: MenuItem[]): ResolvedLayout {
  return {
    name: "t",
    windows: {},
    actions: {
      "mail.archive": {
        label: "@mail.action.archive",
        icon: "archive",
        alias_of: null,
        args: [],
      },
    },
    keys: [
      { context: "global", key: "ctrl+r", action: "mail.reply", args: [] },
      { context: "list", key: "e", action: "mail.archive", args: [] },
      { context: "list", key: "a", action: "mail.archive", args: [] },
      { context: "reader", key: "r", action: "mail.reply", args: [] },
    ],
    toolbars: {},
    menus: { [contextMenuName("message-list.row")]: menu },
    toolbar_settings: {},
    views: {},
    default_view: null,
    rail: [],
    settings: [],
    window_frames: {},
  };
}

const header = (label: string): MenuItem => ({ type: "header", label });

// Mirrors `nail_api::menus::tests::headers_are_kept_only_above_entries`.
describe("menu headings (M5b)", () => {
  it("keeps headings only above entries", () => {
    const l = layout([
      header("@a"),
      { type: "separator" },
      header("@b"),
      item("mail.reply"),
      { type: "separator" },
      header("@c"),
      header("@d"),
      item("mail.archive"),
      header("@e"),
    ]);
    const menu = resolveMenu(l, "context:message-list.row", "list");
    expect(
      menu?.items.map((e) =>
        e.type === "action" ? e.action : e.type === "separator" ? "-" : e.label,
      ),
    ).toEqual(["@b", "mail.reply", "-", "@d", "mail.archive"]);
    expect(
      resolveMenu(
        layout([header("@a"), { type: "separator" }, header("@b")]),
        "context:message-list.row",
        "list",
      ),
    ).toBeNull();
  });
});

// Mirrors `nail_api::menus` tests.
describe("context menus", () => {
  it("resolves labels, icons and shortcuts and normalizes separators", () => {
    const l = layout([
      { type: "separator" },
      item("mail.reply"),
      { type: "separator" },
      { type: "separator" },
      item("mail.archive"),
      { type: "action", action: "view.toggle", args: ["reader"], label: "Reader", icon: null },
      { type: "submenu", label: "@menu.empty", items: [{ type: "separator" }] },
      { type: "separator" },
    ]);
    const name = contextMenuName("message-list.row");
    expect(name).toBe("context:message-list.row");
    const menu = resolveMenu(l, name, keyContextOf("message-list.row"));
    expect(menu?.items).toEqual([
      {
        type: "action",
        action: "mail.reply",
        args: [],
        label: null,
        icon: null,
        shortcut: "ctrl+r",
      },
      { type: "separator" },
      {
        type: "action",
        action: "mail.archive",
        args: [],
        label: "@mail.action.archive",
        icon: "archive",
        shortcut: "a",
      },
      {
        type: "action",
        action: "view.toggle",
        args: ["reader"],
        label: "Reader",
        icon: null,
        shortcut: null,
      },
    ]);
  });

  it("resolves empty and missing menus to null", () => {
    const l = layout([{ type: "separator" }]);
    expect(resolveMenu(l, "context:message-list.row", "list")).toBeNull();
    expect(resolveMenu(l, "context:nothing", "list")).toBeNull();
    expect(keyContextOf("composer")).toBe("composer");
    expect(keyContextOf("unknown.part")).toBe("global");
  });
});

describe("withoutDisabled (M5b review)", () => {
  const entry = (action: string) => ({
    type: "action" as const,
    action,
    args: [],
    label: null,
    icon: null,
    shortcut: null,
  });
  const off = new Set(["junk.not-junk", "gmail.apply-labels", "sync.download-message"]);
  const enabled = (action: string) => !off.has(action);

  it("drops what does not apply and normalizes headings and separators", () => {
    const items = withoutDisabled(
      [
        entry("mail.reply"),
        { type: "separator" },
        { type: "header", label: "@g.message" },
        entry("junk.mark"),
        entry("junk.not-junk"),
        entry("sync.download-message"),
        { type: "separator" },
        { type: "header", label: "@g.labels" },
        entry("gmail.apply-labels"),
        { type: "separator" },
        { type: "submenu", label: "@more", items: [entry("gmail.apply-labels")] },
      ],
      enabled,
    );
    expect(items.map((e) => (e.type === "action" ? e.action : e.type))).toEqual([
      "mail.reply",
      "separator",
      "header",
      "junk.mark",
    ]);
  });

  it("is empty when nothing applies", () => {
    expect(withoutDisabled([entry("junk.not-junk"), { type: "separator" }], enabled)).toEqual([]);
  });
});
