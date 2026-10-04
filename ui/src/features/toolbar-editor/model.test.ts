/**
 * M5d WP182: the toolbar customizer's model — the bar / "More" split, which
 * item may go where, destination indexes after the item left its place, and
 * how the three patches change a toolbar (the mock core applies them with
 * this model; nail-config's `layout_edit.rs` implements the same rules).
 */
import type { MenuItem, ResolvedLayout, ToolbarItem } from "../../api/generated";
import { normalizeLayout } from "../../api/normalize";
import defaultLayoutJson from "../../../../crates/nail-api/fixtures/default-layout.json";
import { applyToolbarEditPatch, isToolbarEditPatch } from "./mock";
import {
  ToolbarEditError,
  addPatch,
  applyToolbarEdit,
  areaItems,
  asMenuItem,
  asToolbarItem,
  barItems,
  canPlace,
  isNoOpMove,
  moreItemOf,
  moreItems,
  moveDestination,
  movePatch,
  removePatch,
  resetPatch,
  usedActions,
  type ToolbarEdit,
} from "./model";
import { customizerWindow, toolbarOfWindow } from "./ids";
import { toolbarTitleId } from "./names";

type ActionItem = Extract<ToolbarItem, { type: "action" }>;
const action = (id: string, extra: Partial<ActionItem> = {}): ActionItem => ({
  type: "action",
  action: id,
  args: [],
  label: null,
  icon: null,
  ...extra,
});
const menuAction = (id: string): MenuItem => ({
  type: "action",
  action: id,
  args: [],
  label: null,
  icon: null,
});

/** reply | archive (rank 2, icon) | separator | spacer | More: [header, read, junk] */
function sample(): ToolbarItem[] {
  return [
    action("mail.reply"),
    action("mail.archive", { rank: 2, display: "icon" }),
    { type: "separator" },
    { type: "spacer" },
    {
      type: "more",
      label: null,
      icon: null,
      items: [
        { type: "header", label: "@toolbar.group.message" },
        menuAction("mail.toggle-read"),
        menuAction("junk.mark"),
      ],
    },
  ];
}

const ids = (items: readonly (ToolbarItem | MenuItem)[]) =>
  items.map((item) => (item.type === "action" ? item.action : item.type));

const apply = (items: ToolbarItem[], edit: ToolbarEdit) => applyToolbarEdit(items, edit);

describe("toolbar customizer model", () => {
  it("splits a toolbar into the bar and its More menu", () => {
    const items = sample();
    expect(ids(barItems(items))).toEqual(["mail.reply", "mail.archive", "separator", "spacer"]);
    expect(ids(moreItems(items))).toEqual(["header", "mail.toggle-read", "junk.mark"]);
    expect(areaItems(items, "bar")).toEqual(barItems(items));
    expect(moreItemOf(items)?.type).toBe("more");
    expect(moreItems([action("a")])).toEqual([]);
    expect(moreItemOf([action("a")])).toBeNull();
  });

  it("knows which item may go where", () => {
    expect(canPlace(action("a"), "bar")).toBe(true);
    expect(canPlace(action("a"), "more")).toBe(true);
    expect(canPlace({ type: "separator" }, "more")).toBe(true);
    expect(canPlace({ type: "spacer" }, "bar")).toBe(true);
    expect(canPlace({ type: "spacer" }, "more")).toBe(false);
    expect(canPlace({ type: "header", label: "x" }, "bar")).toBe(false);
    expect(canPlace({ type: "submenu", label: "x", items: [] }, "more")).toBe(true);
    expect(canPlace({ type: "more", label: null, icon: null, items: [] }, "bar")).toBe(false);
  });

  it("converts between bar and menu items, keeping label and icon", () => {
    const bar = action("mail.archive", {
      label: "Keep",
      icon: "archive",
      rank: 2,
      display: "icon",
    });
    expect(asMenuItem(bar)).toEqual({
      type: "action",
      action: "mail.archive",
      args: [],
      label: "Keep",
      icon: "archive",
    });
    expect(asToolbarItem(bar)).toEqual(bar);
    expect(asToolbarItem(menuAction("junk.mark"))).toEqual(action("junk.mark"));
    expect(asMenuItem({ type: "spacer" })).toBeNull();
    expect(asToolbarItem({ type: "header", label: "x" })).toBeNull();
    expect(asToolbarItem({ type: "separator" })).toEqual({ type: "separator" });
  });

  it("counts a destination after the item left its place", () => {
    expect(moveDestination({ area: "bar", index: 0 }, { area: "bar", index: 3 })).toEqual({
      area: "bar",
      index: 2,
    });
    expect(moveDestination({ area: "bar", index: 3 }, { area: "bar", index: 1 })).toEqual({
      area: "bar",
      index: 1,
    });
    expect(moveDestination({ area: "bar", index: 1 }, { area: "more", index: 0 })).toEqual({
      area: "more",
      index: 0,
    });
    expect(isNoOpMove({ area: "bar", index: 1 }, { area: "bar", index: 1 })).toBe(true);
    expect(isNoOpMove({ area: "bar", index: 1 }, { area: "more", index: 1 })).toBe(false);
  });

  it("builds the contract's patches", () => {
    expect(movePatch("reader", { area: "bar", index: 1 }, { area: "more", index: 0 })).toEqual({
      op: "move-toolbar-item",
      toolbar: "reader",
      from: { area: "bar", index: 1 },
      to: { area: "more", index: 0 },
    });
    expect(addPatch("reader", { type: "separator" }, { area: "bar", index: 2 })).toEqual({
      op: "add-toolbar-item",
      toolbar: "reader",
      item: { type: "separator" },
      to: { area: "bar", index: 2 },
    });
    expect(removePatch("reader", { area: "more", index: 1 })).toEqual({
      op: "remove-toolbar-item",
      toolbar: "reader",
      from: { area: "more", index: 1 },
    });
    expect(resetPatch("reader")).toEqual({ op: "reset-toolbar", toolbar: "reader" });
  });

  it("moves within the bar", () => {
    const next = apply(
      sample(),
      movePatch("t", { area: "bar", index: 0 }, { area: "bar", index: 1 }),
    );
    expect(ids(barItems(next))).toEqual(["mail.archive", "mail.reply", "separator", "spacer"]);
  });

  it("moves an action into More and back, as a menu item", () => {
    const items = sample();
    const inMore = apply(
      items,
      movePatch("t", { area: "bar", index: 1 }, { area: "more", index: 1 }),
    );
    expect(ids(moreItems(inMore))).toEqual([
      "header",
      "mail.archive",
      "mail.toggle-read",
      "junk.mark",
    ]);
    expect(moreItems(inMore)[1]).toEqual(menuAction("mail.archive"));
    expect(ids(barItems(inMore))).toEqual(["mail.reply", "separator", "spacer"]);
    // The input is not changed.
    expect(ids(barItems(items))).toHaveLength(4);
    const back = apply(
      inMore,
      movePatch("t", { area: "more", index: 2 }, { area: "bar", index: 0 }),
    );
    expect(ids(barItems(back))).toEqual(["mail.toggle-read", "mail.reply", "separator", "spacer"]);
    // An item appended to the bar goes before the More item.
    const end = apply(
      inMore,
      movePatch("t", { area: "more", index: 3 }, { area: "bar", index: 99 }),
    );
    expect(end.at(-1)?.type).toBe("more");
    expect(ids(barItems(end)).at(-1)).toBe("junk.mark");
  });

  it("adds a More item when the toolbar has none", () => {
    const next = apply(
      [action("a"), action("b")],
      movePatch("t", { area: "bar", index: 0 }, { area: "more", index: 0 }),
    );
    expect(next).toEqual([
      action("b"),
      { type: "more", label: null, icon: null, items: [menuAction("a")] },
    ]);
  });

  it("adds separators, spacers and actions; refuses what does not fit", () => {
    const items = sample();
    const added = apply(items, addPatch("t", action("print.message"), { area: "bar", index: 1 }));
    expect(ids(barItems(added))).toEqual([
      "mail.reply",
      "print.message",
      "mail.archive",
      "separator",
      "spacer",
    ]);
    const sep = apply(items, addPatch("t", { type: "separator" }, { area: "more", index: 99 }));
    expect(ids(moreItems(sep)).at(-1)).toBe("separator");
    expect(() =>
      apply(items, addPatch("t", { type: "spacer" }, { area: "more", index: 0 })),
    ).toThrow(ToolbarEditError);
    expect(() =>
      apply(
        items,
        addPatch(
          "t",
          { type: "more", label: null, icon: null, items: [] },
          { area: "bar", index: 0 },
        ),
      ),
    ).toThrow(ToolbarEditError);
    // A heading cannot leave the menu.
    expect(() =>
      apply(items, movePatch("t", { area: "more", index: 0 }, { area: "bar", index: 0 })),
    ).toThrow(ToolbarEditError);
  });

  it("adds to an empty bar before the More item", () => {
    const items: ToolbarItem[] = [{ type: "more", label: null, icon: null, items: [] }];
    expect(apply(items, addPatch("t", action("a"), { area: "bar", index: 0 }))).toEqual([
      action("a"),
      { type: "more", label: null, icon: null, items: [] },
    ]);
  });

  it("removes from the bar and from More; refuses an empty slot", () => {
    const items = sample();
    expect(ids(barItems(apply(items, removePatch("t", { area: "bar", index: 2 }))))).toEqual([
      "mail.reply",
      "mail.archive",
      "spacer",
    ]);
    expect(ids(moreItems(apply(items, removePatch("t", { area: "more", index: 0 }))))).toEqual([
      "mail.toggle-read",
      "junk.mark",
    ]);
    expect(() => apply(items, removePatch("t", { area: "bar", index: 9 }))).toThrow(
      ToolbarEditError,
    );
    expect(() => apply(items, removePatch("t", { area: "more", index: 3 }))).toThrow(
      ToolbarEditError,
    );
    expect(() => apply([action("a")], removePatch("t", { area: "more", index: 0 }))).toThrow(
      ToolbarEditError,
    );
  });

  it("lists the actions a toolbar shows (bar, More and submenus)", () => {
    const items: ToolbarItem[] = [
      action("a"),
      {
        type: "more",
        label: null,
        icon: null,
        items: [menuAction("b"), { type: "submenu", label: "s", items: [menuAction("c")] }],
      },
    ];
    expect([...usedActions(items)].sort()).toEqual(["a", "b", "c"]);
  });

  it("names the window per toolbar and the default toolbars", () => {
    expect(customizerWindow("reader")).toBe("toolbar-customizer:reader");
    expect(customizerWindow(null)).toBe("toolbar-customizer");
    expect(customizerWindow("not valid!")).toBe("toolbar-customizer");
    expect(toolbarOfWindow("toolbar-customizer:reader")).toBe("reader");
    expect(toolbarOfWindow("toolbar-customizer")).toBeNull();
    expect(toolbarOfWindow("composer:w1")).toBeNull();
    expect(toolbarTitleId("reader")).toBe("toolbar-editor-toolbar-reader");
    expect(toolbarTitleId("mine")).toBeNull();
  });
});

describe("toolbar customizer mock patches", () => {
  const layout = (): ResolvedLayout => normalizeLayout(defaultLayoutJson);

  it("applies its patches to the mock's layout", () => {
    const l = layout();
    const patch = movePatch("reader", { area: "bar", index: 0 }, { area: "more", index: 0 });
    expect(isToolbarEditPatch(patch)).toBe(true);
    expect(isToolbarEditPatch(resetPatch("reader"))).toBe(false);
    if (!isToolbarEditPatch(patch)) throw new Error("not a toolbar patch");
    expect(applyToolbarEditPatch(l, patch)).toBeNull();
    expect(moreItems(l.toolbars.reader ?? [])[0]).toEqual(menuAction("mail.reply"));
  });

  it("answers an error for an unknown toolbar or an invalid slot", () => {
    const l = layout();
    const before = structuredClone(l.toolbars);
    const unknown = removePatch("nope", { area: "bar", index: 0 });
    const invalid = removePatch("reader", { area: "bar", index: 999 });
    if (!isToolbarEditPatch(unknown) || !isToolbarEditPatch(invalid))
      throw new Error("not a toolbar patch");
    expect(applyToolbarEditPatch(l, unknown)).toMatch(/no toolbar/);
    expect(applyToolbarEditPatch(l, invalid)).toMatch(/no item/);
    expect(l.toolbars).toEqual(before);
  });
});
