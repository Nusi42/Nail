/**
 * The mock core's side of the layout editor's patches (M5d, ADR 0131):
 * `move-pane`, `swap-panes`, `add-widget`, `remove-node`, `set-hidden`,
 * `apply-preset`, `reset-window`, applied to the mock's resolved layout by
 * the customization mock's `UpdateLayout` (`../customization/mock.ts`),
 * which also records the undo entry and announces `ConfigChanged`.
 *
 * The semantics follow `nail_api::customize::LayoutPatch`:
 * - placements reference the tree before the change; docking beside a node
 *   goes into its parent split of that orientation, else the node is
 *   wrapped in a new split `1fr 1fr`;
 * - a container left without children is removed, a split left with one
 *   child is replaced by it;
 * - a node that moves or swaps between containers of the same kind (stack
 *   to stack, split to split of the same orientation) takes its size and
 *   limits along (ADR 0138), else the slots keep theirs / new entries are
 *   `1fr` without limits;
 * - a preset arranges the panes it names in its containers; the
 *   breakpoint's other panes stay, hidden, after the preset's tree in a
 *   stack at the root; a pane in a preset split keeps its limits from a
 *   split of the same orientation (its current one, else the default's).
 *
 * Errors are answered as the `edit-issue-*` id of the problem.
 */
import type {
  LayoutNode,
  LayoutNodeRef,
  LayoutPatch,
  LayoutPlacement,
  NewWidget,
  PresetNode,
  ResolvedLayout,
} from "../../api/generated";
import defaultLayoutJson from "../../../../crates/nail-api/fixtures/default-layout.json";
import { normalizeLayout } from "../../api/normalize";
import { containsRef, nodeAt, nodeIds, presetPaneIds } from "./model";

/** The ops this module applies. */
export const LAYOUT_EDIT_OPS = [
  "move-pane",
  "swap-panes",
  "add-widget",
  "remove-node",
  "set-hidden",
  "apply-preset",
  "reset-window",
] as const satisfies readonly LayoutPatch["op"][];

export type LayoutEditPatch = Extract<LayoutPatch, { op: (typeof LAYOUT_EDIT_OPS)[number] }>;

export function isLayoutEditPatch(patch: LayoutPatch): patch is LayoutEditPatch {
  return (LAYOUT_EDIT_OPS as readonly string[]).includes(patch.op);
}

type Container = Exclude<LayoutNode, { type: "widget" }>;

/** A slot of a container: what a child keeps when it moves along. */
interface Slot {
  /** `stack`, `split-horizontal`, `split-vertical`, `tabs`. */
  kind: string;
  size: string | undefined;
  min: string | null | undefined;
  max: string | null | undefined;
  collapsible: boolean | undefined;
}

class PatchError extends Error {}

function fail(message: string): never {
  throw new PatchError(message);
}

function rootOf(layout: ResolvedLayout, ref: LayoutNodeRef): LayoutNode {
  return layout.windows[ref.window]?.[ref.breakpoint]?.root ?? fail("edit-issue-node-not-found");
}

function setRoot(layout: ResolvedLayout, ref: LayoutNodeRef, root: LayoutNode): void {
  const breakpoint = layout.windows[ref.window]?.[ref.breakpoint];
  if (!breakpoint) fail("edit-issue-node-not-found");
  breakpoint.root = root;
}

function resolve(layout: ResolvedLayout, ref: LayoutNodeRef): LayoutNode {
  return nodeAt(rootOf(layout, ref), ref.path) ?? fail("edit-issue-node-not-found");
}

/** The container holding `target` (by identity) and its index there. */
function parentOf(
  root: LayoutNode,
  target: LayoutNode,
): { parent: Container; index: number } | null {
  if (root.type === "widget") return null;
  for (const [index, child] of root.children.entries()) {
    if (child === target) return { parent: root, index };
    const found = parentOf(child, target);
    if (found) return found;
  }
  return null;
}

function kindOf(parent: Container): string {
  return parent.type === "split" ? `split-${parent.orientation}` : parent.type;
}

/** The entry of child `index` in a list that has one entry per child. */
function entryOf<T>(parent: Container, list: readonly T[], index: number): T | undefined {
  return list.length === parent.children.length ? list[index] : undefined;
}

function slotOf(parent: Container, index: number): Slot {
  return {
    kind: kindOf(parent),
    size: parent.type === "tabs" ? undefined : entryOf(parent, parent.sizes, index),
    min: parent.type === "split" ? entryOf(parent, parent.min_sizes, index) : undefined,
    max: parent.type === "split" ? entryOf(parent, parent.max_sizes, index) : undefined,
    collapsible: parent.type === "split" ? entryOf(parent, parent.collapsible, index) : undefined,
  };
}

/** Writes a slot's entries at `index` (lists that have one entry per child). */
function setSlot(parent: Container, index: number, slot: Slot): void {
  const count = parent.children.length;
  if (parent.type !== "tabs" && parent.sizes.length === count && slot.size !== undefined) {
    parent.sizes[index] = slot.size;
  }
  if (parent.type !== "split") return;
  if (parent.min_sizes.length === count && slot.min !== undefined) {
    parent.min_sizes[index] = slot.min;
  }
  if (parent.max_sizes.length === count && slot.max !== undefined) {
    parent.max_sizes[index] = slot.max;
  }
  if (parent.collapsible.length === count && slot.collapsible !== undefined) {
    parent.collapsible[index] = slot.collapsible;
  }
}

/** Removes child `index` with its slot entries. */
function removeChild(parent: Container, index: number): void {
  const count = parent.children.length;
  parent.children.splice(index, 1);
  if (parent.type !== "tabs" && parent.sizes.length === count) parent.sizes.splice(index, 1);
  if (parent.type === "split") {
    if (parent.min_sizes.length === count) parent.min_sizes.splice(index, 1);
    if (parent.max_sizes.length === count) parent.max_sizes.splice(index, 1);
    if (parent.collapsible.length === count) parent.collapsible.splice(index, 1);
  }
  if (parent.type === "tabs" && parent.active >= parent.children.length) parent.active = 0;
}

/** Inserts `child` at `index` (slot entries kept consistent). */
function insertChild(parent: Container, index: number, child: LayoutNode, slot?: Slot): void {
  const count = parent.children.length;
  const at = Math.max(0, Math.min(index, count));
  parent.children.splice(at, 0, child);
  if (count === 0) return;
  const carried = slot?.kind === kindOf(parent) ? slot : undefined;
  if (parent.type !== "tabs" && parent.sizes.length === count) {
    parent.sizes.splice(at, 0, carried?.size ?? "1fr");
  }
  if (parent.type === "split") {
    if (parent.min_sizes.length === count) parent.min_sizes.splice(at, 0, carried?.min ?? null);
    if (parent.max_sizes.length === count) parent.max_sizes.splice(at, 0, carried?.max ?? null);
    if (parent.collapsible.length === count) {
      parent.collapsible.splice(at, 0, carried?.collapsible ?? false);
    }
  }
}

function newSplit(orientation: "horizontal" | "vertical", children: LayoutNode[]): Container {
  return {
    type: "split",
    id: null,
    label: null,
    orientation,
    sizes: children.map(() => "1fr"),
    min_sizes: [],
    max_sizes: [],
    collapsible: [],
    resizable: true,
    children,
  };
}

/**
 * Removes `node` (by identity) and tidies: empty containers go, a split
 * with one child is replaced by it. The root is never removed.
 */
function detach(layout: ResolvedLayout, ref: LayoutNodeRef, node: LayoutNode): void {
  let target = node;
  for (;;) {
    const root = rootOf(layout, ref);
    const found = parentOf(root, target);
    if (!found) fail("edit-issue-node-not-found");
    const { parent, index } = found;
    removeChild(parent, index);
    if (parent.children.length === 0 && parent !== root) {
      target = parent;
      continue;
    }
    if (parent.type === "split" && parent.children.length === 1) {
      const only = parent.children[0];
      if (only === undefined) return;
      if (parent === root) {
        setRoot(layout, ref, only);
      } else {
        const up = parentOf(root, parent);
        if (up) up.parent.children[up.index] = only;
      }
    }
    return;
  }
}

/** Puts `node` where `to` says (`resolved` = the node `to` names, found before the change). */
function place(
  layout: ResolvedLayout,
  to: LayoutPlacement,
  node: LayoutNode,
  slot: Slot | undefined,
  resolved: LayoutNode,
): void {
  if (to.kind === "into") {
    if (resolved.type === "widget") fail("edit-issue-invalid-placement");
    insertChild(resolved, to.index, node, slot);
    return;
  }
  const orientation = to.side === "left" || to.side === "right" ? "horizontal" : "vertical";
  const after = to.side === "right" || to.side === "bottom";
  const root = rootOf(layout, to.target);
  const found = parentOf(root, resolved);
  if (found?.parent.type === "split" && found.parent.orientation === orientation) {
    insertChild(found.parent, found.index + (after ? 1 : 0), node, slot);
    return;
  }
  const wrapper = newSplit(orientation, after ? [resolved, node] : [node, resolved]);
  if (found) found.parent.children[found.index] = wrapper;
  else setRoot(layout, to.target, wrapper);
}

function targetRef(to: LayoutPlacement): LayoutNodeRef {
  return to.kind === "into" ? to.parent : to.target;
}

function movePane(layout: ResolvedLayout, ref: LayoutNodeRef, to: LayoutPlacement): void {
  if (ref.path.length === 0) fail("edit-issue-invalid-placement");
  const target = targetRef(to);
  if (target.window !== ref.window || target.breakpoint !== ref.breakpoint) {
    fail("edit-issue-invalid-placement");
  }
  if (containsRef(ref, target)) fail("edit-issue-invalid-placement");
  const node = resolve(layout, ref);
  const resolved = resolve(layout, target);
  const root = rootOf(layout, ref);
  const found = parentOf(root, node) ?? fail("edit-issue-node-not-found");
  const slot = slotOf(found.parent, found.index);
  // A placeholder keeps the old place until the node sits in its new one.
  const placeholder = newSplit("horizontal", []);
  found.parent.children[found.index] = placeholder;
  place(layout, to, node, slot, resolved);
  detach(layout, ref, placeholder);
}

function swapPanes(layout: ResolvedLayout, a: LayoutNodeRef, b: LayoutNodeRef): void {
  if (a.window !== b.window || a.breakpoint !== b.breakpoint) fail("edit-issue-invalid-placement");
  if (containsRef(a, b) || containsRef(b, a)) fail("edit-issue-invalid-placement");
  const root = rootOf(layout, a);
  const nodeA = resolve(layout, a);
  const nodeB = resolve(layout, b);
  const at = parentOf(root, nodeA) ?? fail("edit-issue-invalid-placement");
  const bt = parentOf(root, nodeB) ?? fail("edit-issue-invalid-placement");
  // Same container kind: each takes its own entries along (ADR 0138).
  if (kindOf(at.parent) === kindOf(bt.parent)) {
    const slotA = slotOf(at.parent, at.index);
    const slotB = slotOf(bt.parent, bt.index);
    setSlot(at.parent, at.index, slotB);
    setSlot(bt.parent, bt.index, slotA);
  }
  at.parent.children[at.index] = nodeB;
  bt.parent.children[bt.index] = nodeA;
}

/** A node id derived from `kind` that is unique in the window. */
function uniqueId(layout: ResolvedLayout, window: string, kind: string): string {
  const taken = new Set<string>();
  for (const breakpoint of layout.windows[window] ?? []) {
    for (const id of nodeIds(breakpoint.root)) taken.add(id);
  }
  if (!taken.has(kind)) return kind;
  let n = 2;
  while (taken.has(`${kind}-${String(n)}`)) n += 1;
  return `${kind}-${String(n)}`;
}

const KEBAB = /^[a-z][a-z0-9]*(?:-[a-z0-9]+)*$/;

function addWidget(layout: ResolvedLayout, widget: NewWidget, to: LayoutPlacement): void {
  if (!KEBAB.test(widget.kind)) fail("edit-issue-unknown-widget");
  if (widget.kind === "plugin-panel" && typeof widget.options.panel !== "string") {
    fail("edit-issue-unknown-widget");
  }
  const target = targetRef(to);
  const resolved = resolve(layout, target);
  const node: LayoutNode = {
    type: "widget",
    id: widget.id ?? uniqueId(layout, target.window, widget.kind),
    label: widget.label,
    kind: widget.kind,
    options: structuredClone(widget.options),
    rows: [],
    source: null,
  };
  place(layout, to, node, undefined, resolved);
}

function removeNode(layout: ResolvedLayout, ref: LayoutNodeRef): void {
  if (ref.path.length === 0) fail("edit-issue-invalid-placement");
  detach(layout, ref, resolve(layout, ref));
}

function setHidden(layout: ResolvedLayout, ref: LayoutNodeRef, hidden: boolean): void {
  const node = resolve(layout, ref);
  if (hidden) node.hidden = true;
  else delete node.hidden;
}

function applyPreset(layout: ResolvedLayout, id: string, target: LayoutNodeRef): void {
  const preset =
    layout.presets.find((p) => p.id === id && p.window === target.window) ??
    fail("edit-issue-unknown-preset");
  const root = rootOf(layout, { ...target, path: [] });
  const named = new Set(presetPaneIds(preset.root));
  const ids = nodeIds(root);
  for (const pane of named) if (!ids.has(pane)) fail("edit-issue-preset-pane-missing");
  const taken = new Map<string, LayoutNode>();
  const rest: LayoutNode[] = [];
  // Per pane id its slots: the current tree first, then the defaults.
  const limits = new Map<string, Slot[]>();
  const noteSlots = (node: LayoutNode) => {
    if (node.type === "widget") return;
    for (const [index, child] of node.children.entries()) {
      if (child.id !== null && named.has(child.id)) {
        limits.set(child.id, [...(limits.get(child.id) ?? []), slotOf(node, index)]);
      }
      noteSlots(child);
    }
  };
  noteSlots(root);
  for (const breakpoint of normalizeLayout(defaultLayoutJson).windows[target.window] ?? []) {
    noteSlots(breakpoint.root);
  }
  // Named panes are taken whole; containers holding one are dissolved;
  // everything else is an unnamed pane that stays, hidden.
  const collect = (node: LayoutNode, isRoot: boolean) => {
    if (node.id !== null && named.has(node.id)) {
      taken.set(node.id, node);
      return;
    }
    const holdsNamed = [...nodeIds(node)].some((n) => named.has(n));
    if (node.type !== "widget" && (holdsNamed || isRoot)) {
      for (const child of node.children) collect(child, false);
      return;
    }
    node.hidden = true;
    rest.push(node);
  };
  collect(root, true);
  const build = (node: PresetNode): LayoutNode => {
    switch (node.type) {
      case "pane": {
        const pane = taken.get(node.id) ?? fail("edit-issue-preset-pane-missing");
        delete pane.hidden;
        return pane;
      }
      case "split": {
        const split = newSplit(node.orientation, node.children.map(build));
        if (split.type !== "split") return split;
        if (node.sizes.length === node.children.length) split.sizes = [...node.sizes];
        const kind = `split-${node.orientation}`;
        const slots = node.children.map((child) =>
          child.type === "pane" ? (limits.get(child.id) ?? []).filter((s) => s.kind === kind) : [],
        );
        const pick = <T>(get: (s: Slot) => T | undefined): (T | undefined)[] =>
          slots.map((list) => list.map(get).find((v) => v !== undefined));
        const min = pick((s) => s.min);
        const max = pick((s) => s.max);
        const collapsible = pick((s) => s.collapsible);
        if (min.some((v) => v !== undefined)) split.min_sizes = min.map((v) => v ?? null);
        if (max.some((v) => v !== undefined)) split.max_sizes = max.map((v) => v ?? null);
        if (collapsible.some((v) => v !== undefined)) {
          split.collapsible = collapsible.map((v) => v ?? false);
        }
        return split;
      }
      case "stack":
        return {
          type: "stack",
          id: null,
          label: null,
          sizes: [...node.sizes],
          children: node.children.map(build),
        };
      case "tabs":
        return {
          type: "tabs",
          id: null,
          label: null,
          active: 0,
          children: node.children.map(build),
        };
    }
  };
  const tree = build(preset.root);
  setRoot(
    layout,
    target,
    rest.length === 0
      ? tree
      : { type: "stack", id: null, label: null, sizes: [], children: [tree, ...rest] },
  );
}

function resetWindow(layout: ResolvedLayout, window: string): void {
  const defaults = normalizeLayout(defaultLayoutJson).windows[window];
  if (defaults) layout.windows[window] = defaults;
  else {
    layout.windows = Object.fromEntries(
      Object.entries(layout.windows).filter(([name]) => name !== window),
    );
  }
}

/**
 * Applies one patch to `layout` (in place); answers an error message (an
 * `edit-issue-*` id), or `null` when it was applied.
 */
export function applyLayoutEditPatch(
  layout: ResolvedLayout,
  patch: LayoutEditPatch,
): string | null {
  try {
    switch (patch.op) {
      case "move-pane":
        movePane(layout, patch.node, patch.to);
        break;
      case "swap-panes":
        swapPanes(layout, patch.a, patch.b);
        break;
      case "add-widget":
        addWidget(layout, patch.widget, patch.to);
        break;
      case "remove-node":
        removeNode(layout, patch.node);
        break;
      case "set-hidden":
        setHidden(layout, patch.node, patch.hidden);
        break;
      case "apply-preset":
        applyPreset(layout, patch.preset, patch.target);
        break;
      case "reset-window":
        resetWindow(layout, patch.window);
        break;
    }
    return null;
  } catch (error) {
    if (error instanceof PatchError) return error.message;
    throw error;
  }
}
