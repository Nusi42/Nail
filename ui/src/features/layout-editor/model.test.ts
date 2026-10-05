/**
 * The layout editor's model (M5d WP181): panes, drop zones and their
 * patches, presets that fit and the one shown, the windows offered.
 */
import type { LayoutNode, LayoutNodeRef, ResolvedLayout } from "../../api/generated";
import defaultLayoutJson from "../../../../crates/nail-api/fixtures/default-layout.json";
import { normalizeLayout } from "../../api/normalize";
import { must } from "../../test/must";
import {
  containsRef,
  customizableWindows,
  dropPatch,
  isFixedRoot,
  isPane,
  nodeOf,
  panesOf,
  paneRefById,
  presetActive,
  presetFits,
  presetPaneIds,
  presetsOf,
  refKey,
  sameRef,
  shownBreakpoint,
  zonesFor,
  type Moving,
} from "./model";

const layout = (): ResolvedLayout => normalizeLayout(defaultLayoutJson);
const ref = (path: number[], window = "mail", breakpoint = 0): LayoutNodeRef => ({
  window,
  breakpoint,
  path,
});
const mailRoot = (l: ResolvedLayout, breakpoint = 0): LayoutNode =>
  must(l.windows.mail?.[breakpoint]).root;

describe("layout editor model", () => {
  it("finds the mail view's three panes (not the widgets inside them)", () => {
    const l = layout();
    const panes = panesOf(l, "mail", 0);
    expect(panes.map((p) => p.node.id)).toEqual(["sidebar", "list-pane", "reader"]);
    expect(panes.map((p) => p.ref.path)).toEqual([[0], [1], [2]]);
    const root = mailRoot(l);
    expect(isPane(root, [])).toBe(false);
    expect(isPane(root, [0])).toBe(true);
    expect(isPane(root, [0, 0])).toBe(false);
    expect(isPane(root, [9])).toBe(false);
    // The narrow breakpoint: the panes are tabs.
    expect(panesOf(l, "mail", 1).map((p) => p.node.id)).toEqual(["sidebar", "list-pane", "reader"]);
  });

  it("treats children of a root stack as panes and a one-widget window as fixed", () => {
    const l = layout();
    const tasks = must(l.windows.tasks?.[0]).root;
    expect(isFixedRoot(tasks, [])).toBe(true);
    expect(panesOf(l, "tasks", 0)).toHaveLength(1);
    const stack: LayoutNode = {
      type: "stack",
      id: null,
      label: null,
      sizes: [],
      children: [
        must(nodeOf(l, ref([2]))),
        { ...must(nodeOf(l, ref([0]))), hidden: true },
        // An unnamed container is not a pane; its named children are.
        {
          type: "split",
          id: null,
          label: null,
          orientation: "vertical",
          sizes: [],
          min_sizes: [],
          max_sizes: [],
          collapsible: [],
          resizable: true,
          children: [must(nodeOf(l, ref([1])))],
        },
      ],
    };
    expect(isPane(stack, [0])).toBe(true);
    expect(isPane(stack, [1])).toBe(true);
    expect(isPane(stack, [2])).toBe(false);
    expect(isPane(stack, [2, 0])).toBe(true);
  });

  it("compares references", () => {
    expect(containsRef(ref([0]), ref([0, 2]))).toBe(true);
    expect(containsRef(ref([0, 2]), ref([0]))).toBe(false);
    expect(containsRef(ref([0]), ref([0], "mail", 1))).toBe(false);
    expect(sameRef(ref([1]), ref([1]))).toBe(true);
    expect(sameRef(ref([1]), ref([1, 0]))).toBe(false);
    expect(refKey(ref([2, 1]))).toBe("mail/0/2/1");
  });

  it("offers dock and swap targets, never on the moved pane or inside it", () => {
    const l = layout();
    const reader = must(nodeOf(l, ref([2])));
    const pane: Moving = { kind: "pane", ref: ref([0]) };
    expect(zonesFor(pane, ref([2]), reader)).toEqual(["left", "right", "top", "bottom", "swap"]);
    expect(zonesFor(pane, ref([0]), must(nodeOf(l, ref([0]))))).toEqual([]);
    expect(zonesFor(pane, ref([0, 1]), must(nodeOf(l, ref([0, 1]))))).toEqual([]);
    const widget: Moving = {
      kind: "widget",
      widget: { kind: "mini-month", id: null, label: null, options: {} },
      name: "Mini month",
    };
    expect(zonesFor(widget, ref([2]), reader)).toEqual(["left", "right", "top", "bottom", "into"]);
    expect(zonesFor(widget, ref([2, 0]), must(nodeOf(l, ref([2, 0]))))).toEqual([
      "left",
      "right",
      "top",
      "bottom",
    ]);
  });

  it("turns drops into patches", () => {
    const l = layout();
    const reader = must(nodeOf(l, ref([2])));
    const pane: Moving = { kind: "pane", ref: ref([0]) };
    expect(dropPatch(pane, ref([2]), reader, "swap")).toEqual({
      op: "swap-panes",
      a: ref([0]),
      b: ref([2]),
    });
    expect(dropPatch(pane, ref([2]), reader, "bottom")).toEqual({
      op: "move-pane",
      node: ref([0]),
      to: { kind: "beside", target: ref([2]), side: "bottom" },
    });
    const widget: Moving = {
      kind: "widget",
      widget: { kind: "agenda", id: null, label: null, options: { days: 1 } },
      name: "Agenda",
    };
    expect(dropPatch(widget, ref([2]), reader, "into")).toEqual({
      op: "add-widget",
      widget: widget.widget,
      to: {
        kind: "into",
        parent: ref([2]),
        index: reader.type === "widget" ? 0 : reader.children.length,
      },
    });
  });

  it("knows which presets fit and which one is shown", () => {
    const l = layout();
    const presets = presetsOf(l, "mail");
    expect(presets.map((p) => p.id)).toEqual([
      "three-panes",
      "reader-below",
      "wide-reader",
      "focus",
    ]);
    expect(presetPaneIds(must(presets[1]).root)).toEqual(["sidebar", "list-pane", "reader"]);
    const root = mailRoot(l);
    expect(presets.every((p) => presetFits(p, root))).toBe(true);
    expect(presets.filter((p) => presetActive(p, root)).map((p) => p.id)).toEqual(["three-panes"]);
    // Hidden panes do not count: only the reader shows = Focus.
    const focused = structuredClone(root);
    if (focused.type !== "split") throw new Error("split");
    must(focused.children[0]).hidden = true;
    must(focused.children[1]).hidden = true;
    expect(presets.filter((p) => presetActive(p, focused)).map((p) => p.id)).toEqual(["focus"]);
    // A window without the reader cannot take the presets that need it.
    const noReader = structuredClone(root);
    if (noReader.type !== "split") throw new Error("split");
    noReader.children.pop();
    expect(presets.filter((p) => presetFits(p, noReader))).toEqual([]);
    expect(presetsOf(l, "calendar")).toEqual([]);
  });

  it("offers the views' windows, and the main window only without a view host", () => {
    const l = layout();
    expect(customizableWindows(l)).toEqual(["mail", "calendar", "contacts", "tasks", "settings"]);
    const preRail = structuredClone(l);
    preRail.windows.main = [{ ...must(l.windows.mail?.[0]), min_width: null, max_width: null }];
    expect(customizableWindows(preRail)[0]).toBe("main");
  });

  it("finds a pane by id and the breakpoint the width shows", () => {
    const l = layout();
    expect(paneRefById(l, "mail", 0, "reader")).toEqual(ref([2]));
    expect(paneRefById(l, "mail", 0, "nope")).toBeNull();
    expect(shownBreakpoint(l, "mail", 1200)).toBe(0);
    expect(shownBreakpoint(l, "mail", 600)).toBe(1);
    expect(shownBreakpoint(l, "nope", 600)).toBe(-1);
  });
});
