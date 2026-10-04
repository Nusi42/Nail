/**
 * The mock core's layout-editor patches (M5d WP181): the semantics of
 * `nail_api::customize::LayoutPatch` on the resolved layout, and the whole
 * `UpdateLayout` path (base revision, undo history, `ConfigChanged`).
 */
import type {
  Event as NailEvent,
  LayoutNode,
  LayoutNodeRef,
  ResolvedLayout,
} from "../../api/generated";
import defaultLayoutJson from "../../../../crates/nail-api/fixtures/default-layout.json";
import { MockTransport } from "../../api/mock";
import { createMockData } from "../../api/mockData";
import { normalizeLayout } from "../../api/normalize";
import { must } from "../../test/must";
import { applyLayoutEditPatch, isLayoutEditPatch, type LayoutEditPatch } from "./mock";
import { nodeAt } from "./model";

const layout = (): ResolvedLayout => normalizeLayout(defaultLayoutJson);
const ref = (path: number[], window = "mail", breakpoint = 0): LayoutNodeRef => ({
  window,
  breakpoint,
  path,
});
const root = (l: ResolvedLayout, breakpoint = 0): LayoutNode =>
  must(l.windows.mail?.[breakpoint]).root;

/** The shape of a tree: ids, with containers as `type(children)`, hidden marked `!`. */
function shape(node: LayoutNode): string {
  const own = `${node.id ?? node.type}${node.hidden === true ? "!" : ""}`;
  if (node.type === "widget") return own;
  if (node.id !== null && ["sidebar", "list-pane", "reader"].includes(node.id)) return own;
  const kind = node.type === "split" ? `${node.type}-${node.orientation[0] ?? ""}` : node.type;
  return `${node.id === null ? kind : own}(${node.children.map(shape).join(",")})`;
}

function apply(l: ResolvedLayout, patch: LayoutEditPatch): string | null {
  return applyLayoutEditPatch(l, patch);
}

describe("layout editor mock patches", () => {
  it("knows its ops", () => {
    expect(isLayoutEditPatch({ op: "reset-window", window: "mail" })).toBe(true);
    expect(isLayoutEditPatch({ op: "reset-toolbar", toolbar: "reader" })).toBe(false);
  });

  it("swaps panes and keeps each slot's size", () => {
    const l = layout();
    expect(apply(l, { op: "swap-panes", a: ref([0]), b: ref([2]) })).toBeNull();
    const r = root(l);
    expect(shape(r)).toBe("panes(reader,list-pane,sidebar)");
    expect(r.type === "split" && r.sizes).toEqual(["260px", "1fr", "1.4fr"]);
    expect(apply(l, { op: "swap-panes", a: ref([0]), b: ref([0, 1]) })).toBe(
      "edit-issue-invalid-placement",
    );
  });

  it("docks a pane beside another, into the split of that orientation or a new one", () => {
    const l = layout();
    // Right of the reader: the horizontal split takes it; the old slot goes.
    expect(
      apply(l, {
        op: "move-pane",
        node: ref([0]),
        to: { kind: "beside", target: ref([2]), side: "right" },
      }),
    ).toBeNull();
    expect(shape(root(l))).toBe("panes(list-pane,reader,sidebar)");
    const r = root(l);
    expect(r.type === "split" && r.sizes).toEqual(["1fr", "1.4fr", "260px"]);
    // Below the reader (now at 1): a vertical split wraps it.
    expect(
      apply(l, {
        op: "move-pane",
        node: ref([0]),
        to: { kind: "beside", target: ref([1]), side: "bottom" },
      }),
    ).toBeNull();
    expect(shape(root(l))).toBe("panes(split-v(reader,list-pane),sidebar)");
    // Back beside the sidebar: the vertical split is left with one child and dissolves.
    expect(
      apply(l, {
        op: "move-pane",
        node: ref([0, 1]),
        to: { kind: "beside", target: ref([1]), side: "left" },
      }),
    ).toBeNull();
    expect(shape(root(l))).toBe("panes(reader,list-pane,sidebar)");
  });

  it("moves into a container at an index of the tree before the move", () => {
    const l = layout();
    expect(
      apply(l, {
        op: "move-pane",
        node: ref([0]),
        to: { kind: "into", parent: ref([]), index: 3 },
      }),
    ).toBeNull();
    expect(shape(root(l))).toBe("panes(list-pane,reader,sidebar)");
    expect(
      apply(l, {
        op: "move-pane",
        node: ref([]),
        to: { kind: "into", parent: ref([0]), index: 0 },
      }),
    ).toBe("edit-issue-invalid-placement");
    expect(
      apply(l, {
        op: "move-pane",
        node: ref([0]),
        to: { kind: "into", parent: ref([0, 0]), index: 0 },
      }),
    ).toBe("edit-issue-invalid-placement");
    expect(
      apply(l, {
        op: "move-pane",
        node: ref([7]),
        to: { kind: "into", parent: ref([]), index: 0 },
      }),
    ).toBe("edit-issue-node-not-found");
  });

  it("adds widgets with ids unique in the window", () => {
    const l = layout();
    const add = (side: "top" | "bottom") =>
      apply(l, {
        op: "add-widget",
        widget: { kind: "mini-month", id: null, label: null, options: {} },
        to: { kind: "beside", target: ref([0, 0]), side },
      });
    expect(add("bottom")).toBeNull();
    expect(add("top")).toBeNull();
    // The stack is no split: the folder tree is wrapped in a vertical one.
    const sidebar = must(nodeAt(root(l), [0]));
    expect(shape(sidebar)).toBe("sidebar");
    const first = must(nodeAt(sidebar, [0]));
    expect(shape(first)).toBe("split-v(mini-month-2,split-v(folders,mini-month))");
    // Into the stack: a stack with sizes gets one for the new child.
    expect(
      apply(l, {
        op: "add-widget",
        widget: { kind: "task-list", id: null, label: null, options: {} },
        to: { kind: "into", parent: ref([0]), index: 1 },
      }),
    ).toBeNull();
    expect(sidebar.type === "stack" && sidebar.children.map((c) => c.id)).toEqual([
      null,
      "task-list",
      "gmail-labels",
      "exchange-mailboxes",
      "exchange-categories",
    ]);
    expect(sidebar.type === "stack" && sidebar.sizes).toEqual([
      "1fr",
      "1fr",
      "auto",
      "auto",
      "auto",
    ]);
    expect(
      apply(l, {
        op: "add-widget",
        widget: { kind: "plugin-panel", id: null, label: null, options: {} },
        to: { kind: "into", parent: ref([2]), index: 0 },
      }),
    ).toBe("edit-issue-unknown-widget");
    expect(
      apply(l, {
        op: "add-widget",
        widget: { kind: "Bad Kind", id: null, label: null, options: {} },
        to: { kind: "into", parent: ref([2]), index: 0 },
      }),
    ).toBe("edit-issue-unknown-widget");
  });

  it("removes nodes and tidies the containers", () => {
    const l = layout();
    expect(apply(l, { op: "remove-node", node: ref([1]) })).toBeNull();
    expect(apply(l, { op: "remove-node", node: ref([1]) })).toBeNull();
    // A split left with one child is replaced by it.
    expect(shape(root(l))).toBe("sidebar");
    expect(apply(l, { op: "remove-node", node: ref([]) })).toBe("edit-issue-invalid-placement");
  });

  it("hides and shows nodes", () => {
    const l = layout();
    expect(apply(l, { op: "set-hidden", node: ref([0]), hidden: true })).toBeNull();
    expect(shape(root(l))).toBe("panes(sidebar!,list-pane,reader)");
    expect(apply(l, { op: "set-hidden", node: ref([0]), hidden: false })).toBeNull();
    expect("hidden" in must(nodeAt(root(l), [0]))).toBe(false);
  });

  it("applies presets; unnamed panes stay hidden in a stack", () => {
    const l = layout();
    const preset = (id: string) => apply(l, { op: "apply-preset", preset: id, target: ref([]) });
    expect(preset("reader-below")).toBeNull();
    expect(shape(root(l))).toBe("split-h(sidebar,split-v(list-pane,reader))");
    expect(preset("focus")).toBeNull();
    expect(shape(root(l))).toBe("stack(reader,sidebar!,list-pane!)");
    expect(preset("three-panes")).toBeNull();
    expect(shape(root(l))).toBe("split-h(sidebar,list-pane,reader)");
    const r = root(l);
    expect(r.type === "split" && r.sizes).toEqual(["260px", "1fr", "1.4fr"]);
    expect(preset("nope")).toBe("edit-issue-unknown-preset");
    expect(apply(l, { op: "remove-node", node: ref([2]) })).toBeNull();
    expect(preset("focus")).toBe("edit-issue-preset-pane-missing");
  });

  it("resets a window to the default", () => {
    const l = layout();
    apply(l, { op: "swap-panes", a: ref([0]), b: ref([1]) });
    apply(l, { op: "swap-panes", a: ref([0], "mail", 1), b: ref([1], "mail", 1) });
    expect(apply(l, { op: "reset-window", window: "mail" })).toBeNull();
    expect(l.windows.mail).toEqual(layout().windows.mail);
    l.windows.extra = structuredClone(must(l.windows.tasks));
    expect(apply(l, { op: "reset-window", window: "extra" })).toBeNull();
    expect(l.windows.extra).toBeUndefined();
  });
});

describe("UpdateLayout with layout-editor patches (mock core)", () => {
  it("writes, records the undo entry and checks the base revision", async () => {
    const transport = new MockTransport({ data: createMockData(5) });
    const events: NailEvent[] = [];
    transport.onEvent((e) => events.push(e));
    const swap = { op: "swap-panes" as const, a: ref([0]), b: ref([2]) };
    expect(await transport.send({ type: "UpdateLayout", patches: [swap] })).toEqual({
      type: "Done",
    });
    expect(shape(root(transport.data.layout))).toBe("panes(reader,list-pane,sidebar)");
    const changed = events.find((e) => e.type === "ConfigChanged");
    expect(changed?.type === "ConfigChanged" && shape(root(changed.layout))).toBe(
      "panes(reader,list-pane,sidebar)",
    );
    const history = await transport.send({ type: "GetEditHistory", scope: "layout" });
    if (history.type !== "EditHistory") throw new Error(history.type);
    expect(history.history.undo.map((e) => e.kind)).toEqual(["swap-panes"]);
    // A stale base revision is a conflict; nothing is written.
    expect(
      await transport.send({ type: "UpdateLayout", patches: [swap], base_revision: "none" }),
    ).toMatchObject({ type: "Error", code: "conflict" });
    // Errors of a patch answer invalid_request.
    expect(
      await transport.send({
        type: "UpdateLayout",
        patches: [{ op: "remove-node", node: ref([]) }],
        base_revision: history.history.revision,
      }),
    ).toMatchObject({ type: "Error", code: "invalid_request" });
    expect(await transport.send({ type: "UndoEdit", scope: "layout", force: false })).toMatchObject(
      { type: "EditHistory" },
    );
    expect(shape(root(transport.data.layout))).toBe("panes(sidebar,list-pane,reader)");
  });
});
