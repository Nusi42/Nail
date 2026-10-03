/**
 * Toolbar overflow (M5b): `overflowOrder` mirrors
 * `nail_api::views::tests::overflow_moves_unranked_from_the_end_then_highest_rank`;
 * fitting and hidden disabled actions are the renderer's shared rules.
 */
import type { ToolbarItem } from "../api/generated";
import { fitToolbar, normalizeSeparators, overflowOrder, shownToolbarItems } from "./overflow";

const act = (action: string, rank?: number): ToolbarItem => ({
  type: "action",
  action,
  args: [],
  label: null,
  icon: null,
  ...(rank === undefined ? {} : { rank }),
});

describe("toolbar overflow (M5b)", () => {
  it("moves unranked items from the end first, then the highest rank", () => {
    const items: ToolbarItem[] = [
      act("mail.reply", 1),
      act("mail.reply-all", 2),
      { type: "separator" },
      act("mail.archive", 4),
      act("mail.delete", 4),
      act("junk.mark"),
      { type: "spacer" },
      act("print.message"),
      { type: "more", label: null, icon: null, items: [] },
    ];
    expect(overflowOrder(items)).toEqual([7, 5, 4, 3, 1, 0]);
    expect(overflowOrder([{ type: "spacer" }])).toEqual([]);
  });

  it("fits a bar, reserving the More button once something moves", () => {
    const items: ToolbarItem[] = [
      act("a", 1),
      act("b", 2),
      { type: "separator" },
      act("c", 3),
      act("d"),
    ];
    const widths = [80, 80, 9, 32, 32];
    // Everything fits (233 px): no More button.
    expect(fitToolbar(items, widths, 300, 20)).toEqual({ visible: [0, 1, 2, 3, 4], overflow: [] });
    // "d" (unranked) moves first, and the More button (20 px) needs room.
    expect(fitToolbar(items, widths, 225, 20)).toEqual({ visible: [0, 1, 2, 3], overflow: [4] });
    // Narrower: "c" moves next; the separator before it would trail and goes.
    expect(fitToolbar(items, widths, 205, 20)).toEqual({ visible: [0, 1], overflow: [3, 4] });
    // Nothing fits: every action moves, rank 1 last.
    expect(fitToolbar(items, widths, 10, 20)).toEqual({ visible: [], overflow: [0, 1, 3, 4] });
  });

  it("counts the toolbar's own More item instead of reserving one", () => {
    const items: ToolbarItem[] = [
      act("a", 1),
      act("b"),
      { type: "more", label: null, icon: null, items: [] },
    ];
    expect(fitToolbar(items, [50, 50, 40], 95, 1000)).toEqual({ visible: [0, 2], overflow: [1] });
  });

  it("normalizes separators and spacers", () => {
    const items: ToolbarItem[] = [
      { type: "separator" },
      act("a"),
      { type: "separator" },
      { type: "spacer" },
      act("b"),
      { type: "separator" },
    ];
    expect(normalizeSeparators(items, [0, 1, 2, 3, 4, 5])).toEqual([1, 3, 4]);
  });

  it("hides actions that cannot run when the toolbar says so", () => {
    const items: ToolbarItem[] = [
      act("mail.reply"),
      act("gmail.apply-labels"),
      {
        type: "more",
        label: null,
        icon: null,
        items: [
          { type: "header", label: "@toolbar.group.message" },
          { type: "action", action: "exchange.classify", args: [], label: null, icon: null },
          {
            type: "submenu",
            label: "x",
            items: [
              { type: "action", action: "gmail.apply-labels", args: [], label: null, icon: null },
            ],
          },
          { type: "action", action: "mail.toggle-read", args: [], label: null, icon: null },
        ],
      },
    ];
    const enabled = (action: string) => action.startsWith("mail.");
    expect(shownToolbarItems(items, { hide_disabled: false, overflow: "more" }, enabled)).toEqual(
      items,
    );
    const shown = shownToolbarItems(items, { hide_disabled: true, overflow: "more" }, enabled);
    expect(shown.map((i) => (i.type === "action" ? i.action : i.type))).toEqual([
      "mail.reply",
      "more",
    ]);
    const more = shown[1];
    expect(more?.type === "more" ? more.items.map((i) => i.type) : []).toEqual([
      "header",
      "action",
    ]);
  });
});
