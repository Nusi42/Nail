/**
 * The layout editor's model (M5d WP181, ADR 0131): which nodes of a window
 * are panes (they get a header in customize mode), where a moved pane or a
 * palette widget may go, the patches that do it, and which presets fit a
 * breakpoint. Pure functions over the resolved layout; the write itself is
 * an `UpdateLayout` request (`./useLayoutEdit.ts`).
 */
import { DOCK_SIDE_VALUES } from "../../api/generated";
import type {
  DockSide,
  JsonValue,
  LayoutNode,
  LayoutNodeRef,
  LayoutPatch,
  LayoutPreset,
  NewWidget,
  PresetNode,
  ResolvedLayout,
} from "../../api/generated";
import { selectBreakpointIndex } from "../../layout/engine";
import { hostsViews } from "../../layout/views";
import { nodeAt } from "../../layout/engine";

/** Where a dragged pane or widget can be dropped on a pane. */
export type DropZone = DockSide | "swap" | "into";

export { nodeAt };

/** The dock sides, in the order the keyboard reaches them. */
export const DOCK_SIDES: readonly DockSide[] = DOCK_SIDE_VALUES;

/** What is being moved (picked up with the keyboard, or dragged). */
export type Moving =
  { kind: "pane"; ref: LayoutNodeRef } | { kind: "widget"; widget: NewWidget; name: string };

/** The root of a reference's breakpoint. */
export function rootOf(layout: ResolvedLayout, ref: LayoutNodeRef): LayoutNode | null {
  return layout.windows[ref.window]?.[ref.breakpoint]?.root ?? null;
}

/** The node a reference names. */
export function nodeOf(layout: ResolvedLayout, ref: LayoutNodeRef): LayoutNode | null {
  const root = rootOf(layout, ref);
  return root ? nodeAt(root, ref.path) : null;
}

/** Whether `a` is `b` or one of its ancestors (same window and breakpoint). */
export function containsRef(a: LayoutNodeRef, b: LayoutNodeRef): boolean {
  return (
    a.window === b.window &&
    a.breakpoint === b.breakpoint &&
    a.path.length <= b.path.length &&
    a.path.every((index, i) => b.path[i] === index)
  );
}

/** Whether two references name the same node. */
export function sameRef(a: LayoutNodeRef, b: LayoutNodeRef): boolean {
  return containsRef(a, b) && a.path.length === b.path.length;
}

/** A node the user can name: a widget, or a container with an id or label. */
function named(node: LayoutNode): boolean {
  return node.type === "widget" || node.id !== null || node.label !== null;
}

/**
 * Whether the node at `path` is a pane: a named node that is a child of
 * the root or of a split / tabs, and inside no other pane. Panes get a
 * header with a drag handle, the eye and Remove.
 */
export function isPane(root: LayoutNode, path: readonly number[]): boolean {
  if (path.length === 0) return false;
  let parent = root;
  for (let depth = 1; depth <= path.length; depth++) {
    if (parent.type === "widget") return false;
    const node = parent.children[path[depth - 1] ?? -1];
    if (node === undefined) return false;
    const qualifies = named(node) && (depth === 1 || parent.type !== "stack");
    if (qualifies) return depth === path.length;
    parent = node;
  }
  return false;
}

/**
 * Whether the root is shown as a fixed frame (a window that is one widget:
 * it cannot move, but panes can be docked beside it).
 */
export function isFixedRoot(root: LayoutNode, path: readonly number[]): boolean {
  return path.length === 0 && root.type === "widget";
}

/** One pane of a breakpoint. */
export interface PaneInfo {
  node: LayoutNode;
  ref: LayoutNodeRef;
}

/** Every pane of a breakpoint, in document order (a widget root counts). */
export function panesOf(layout: ResolvedLayout, window: string, breakpoint: number): PaneInfo[] {
  const root = layout.windows[window]?.[breakpoint]?.root;
  if (!root) return [];
  if (root.type === "widget") return [{ node: root, ref: { window, breakpoint, path: [] } }];
  const out: PaneInfo[] = [];
  const walk = (node: LayoutNode, path: number[]) => {
    if (path.length > 0 && isPane(root, path)) {
      out.push({ node, ref: { window, breakpoint, path } });
      return;
    }
    if (node.type !== "widget") {
      node.children.forEach((child, index) => {
        walk(child, [...path, index]);
      });
    }
  };
  walk(root, []);
  return out;
}

/** The breakpoint of `window` the width shows (`-1` = none). */
export function shownBreakpoint(layout: ResolvedLayout, window: string, width: number): number {
  return selectBreakpointIndex(layout.windows[window] ?? [], width);
}

/** The drop zones a pane offers for what is being moved. */
export function zonesFor(
  moving: Moving,
  target: LayoutNodeRef,
  targetNode: LayoutNode,
): readonly DropZone[] {
  if (moving.kind === "pane") {
    if (containsRef(moving.ref, target) || containsRef(target, moving.ref)) return [];
    // The fixed root (a one-widget window) can only be docked to.
    return target.path.length === 0 ? DOCK_SIDES : [...DOCK_SIDES, "swap"];
  }
  return targetNode.type === "widget" ? DOCK_SIDES : [...DOCK_SIDES, "into"];
}

/** The patch that drops `moving` on zone `zone` of `target`. */
export function dropPatch(
  moving: Moving,
  target: LayoutNodeRef,
  targetNode: LayoutNode,
  zone: DropZone,
): LayoutPatch {
  const to =
    zone === "into" || zone === "swap"
      ? {
          kind: "into" as const,
          parent: target,
          index: targetNode.type === "widget" ? 0 : targetNode.children.length,
        }
      : { kind: "beside" as const, target, side: zone };
  if (moving.kind === "widget") return { op: "add-widget", widget: moving.widget, to };
  if (zone === "swap") return { op: "swap-panes", a: moving.ref, b: target };
  return { op: "move-pane", node: moving.ref, to };
}

// --- Presets ----------------------------------------------------------------

/** The pane ids a preset arranges, in order. */
export function presetPaneIds(node: PresetNode): string[] {
  return node.type === "pane" ? [node.id] : node.children.flatMap(presetPaneIds);
}

/** Every node id below (and including) `node`. */
export function nodeIds(node: LayoutNode): Set<string> {
  const ids = new Set<string>();
  const walk = (n: LayoutNode) => {
    if (n.id !== null) ids.add(n.id);
    if (n.type !== "widget") n.children.forEach(walk);
  };
  walk(node);
  return ids;
}

/** The presets of a window. */
export function presetsOf(layout: ResolvedLayout, window: string): LayoutPreset[] {
  return layout.presets.filter((preset) => preset.window === window);
}

/** Whether a breakpoint has every pane a preset names. */
export function presetFits(preset: LayoutPreset, root: LayoutNode): boolean {
  const ids = nodeIds(root);
  return presetPaneIds(preset.root).every((id) => ids.has(id));
}

/** A tree's arrangement of the given panes, as a comparable text. */
function arrangement(node: LayoutNode, panes: ReadonlySet<string>): string | null {
  if (node.hidden === true) return null;
  if (node.id !== null && panes.has(node.id)) return `pane:${node.id}`;
  // Anything else shown (a pane the preset does not name) breaks the match.
  if (node.type === "widget" || ![...nodeIds(node)].some((id) => panes.has(id))) return "other";
  const parts = node.children
    .map((child) => arrangement(child, panes))
    .filter((part) => part !== null);
  if (parts.length === 0) return null;
  if (parts.length === 1) return parts[0] ?? null;
  const kind = node.type === "split" ? `split-${node.orientation}` : node.type;
  return `${kind}(${parts.join(",")})`;
}

function presetArrangement(node: PresetNode): string {
  if (node.type === "pane") return `pane:${node.id}`;
  const parts = node.children.map(presetArrangement);
  if (parts.length === 1) return parts[0] ?? "";
  const kind = node.type === "split" ? `split-${node.orientation}` : node.type;
  return `${kind}(${parts.join(",")})`;
}

/**
 * Whether the breakpoint shows the preset's arrangement now: its panes
 * shown in the preset's containers and nothing else shown (sizes aside).
 */
export function presetActive(preset: LayoutPreset, root: LayoutNode): boolean {
  const panes = new Set(presetPaneIds(preset.root));
  return arrangement(root, panes) === presetArrangement(preset.root);
}

// --- The widget palette -----------------------------------------------------

/** The kinds "Add to layout" offers (message ids `layout-editor-widget-<kind>`). */
export type PaletteKind = "mini-month" | "agenda" | "task-list" | "smart-folders";

/** A built-in entry of "Add to layout". */
export interface PaletteWidget {
  /** Widget kind (registered in nail-config). */
  kind: PaletteKind;
  /** Lucide icon. */
  icon: string;
  /** Options written with the widget. */
  options: Readonly<Record<string, JsonValue>>;
}

/** The built-in widgets "Add to layout" offers (plug-in panels follow). */
export const PALETTE_WIDGETS: readonly PaletteWidget[] = [
  { kind: "mini-month", icon: "calendar-days", options: {} },
  { kind: "agenda", icon: "list-checks", options: { days: 1 } },
  { kind: "task-list", icon: "square-check-big", options: {} },
  { kind: "smart-folders", icon: "search", options: {} },
];

/** The `NewWidget` of a palette entry. */
export function newWidget(kind: string, options: Readonly<Record<string, JsonValue>>): NewWidget {
  return { kind, id: null, label: null, options: { ...options } };
}

/** The `NewWidget` of a plug-in panel (`plugin-panel "<plugin>.<panel>"`). */
export function pluginPanelWidget(plugin: string, panel: string): NewWidget {
  return newWidget("plugin-panel", { panel: `${plugin}.${panel}` });
}

// --- Windows ----------------------------------------------------------------

/**
 * The windows "Customize layout" offers: the views' windows (Mail first),
 * and the main window when it has no view host (a pre-rail layout).
 */
export function customizableWindows(layout: ResolvedLayout): string[] {
  const out: string[] = [];
  for (const view of Object.values(layout.views)) {
    if (Object.hasOwn(layout.windows, view.window) && !out.includes(view.window)) {
      out.push(view.window);
    }
  }
  if (Object.hasOwn(layout.windows, "main") && !hostsViews(layout)) out.unshift("main");
  return out;
}

/**
 * The reference of the pane with node id `id` in a breakpoint (for moving
 * the focus back to a pane after an edit re-rendered the tree).
 */
export function paneRefById(
  layout: ResolvedLayout,
  window: string,
  breakpoint: number,
  id: string,
): LayoutNodeRef | null {
  return panesOf(layout, window, breakpoint).find((pane) => pane.node.id === id)?.ref ?? null;
}

/** A stable DOM key of a reference. */
export function refKey(ref: LayoutNodeRef): string {
  return [ref.window, ref.breakpoint, ...ref.path].join("/");
}
