import { describe, expect, it } from "vitest";
import type { MenuItem, ResolvedLayout } from "../../api/generated";
import { contextMenuName, keyContextOf, resolveMenu } from "./resolve";

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
  };
}

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
