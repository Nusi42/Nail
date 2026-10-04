/**
 * Layout write-back (design.md, "The file is the truth"): edits made in the
 * UI are sent to the core as typed `LayoutPatch`es (`UpdateLayout`), which
 * writes them into `layout.kdl` keeping comments and formatting. The new
 * layout comes back as `ConfigChanged`.
 *
 * Split sizes: the layout renderer keeps dragged sizes in the UI store
 * (`splitSizes`, keyed by node id or tree path); `SplitWriteBack` watches
 * that map and writes each changed split once the dragging has settled.
 */
import type { NailClient } from "../../api/client";
import type {
  JsonValue,
  LayoutNode,
  LayoutNodeRef,
  LayoutPatch,
  ResolvedLayout,
  ToolbarItem,
} from "../../api/generated";
import { selectBreakpointIndex } from "../../layout/engine";
import { findNodeById } from "./inspect";

/** Quiet time after the last size change before it is written (ms). */
export const WRITE_DELAY_MS = 600;

const PATH_KEY = /^([a-z0-9]+(?:-[a-z0-9]+)*)\/(\d+)((?:\/\d+)*)$/;

function nodeAt(root: LayoutNode, path: readonly number[]): LayoutNode | null {
  let node: LayoutNode | undefined = root;
  for (const index of path) {
    if (!node || node.type === "widget") return null;
    node = node.children[index];
  }
  return node ?? null;
}

/**
 * The `LayoutNodeRef` of a split, from its UI-store key: a tree path
 * (`main/0/1`, as the renderer builds it for nodes without an id) or a node
 * id. An id is looked up in the breakpoint the window width selects first
 * (ids repeat across breakpoints), `main` first.
 */
export function splitRefOf(
  key: string,
  layout: ResolvedLayout,
  width: number,
): LayoutNodeRef | null {
  const isResizable = (node: LayoutNode | null) => node?.type === "split" || node?.type === "stack";
  const byPath = PATH_KEY.exec(key);
  const [, window, breakpointText, pathText] = byPath ?? [];
  if (window !== undefined) {
    const breakpoint = Number(breakpointText);
    const path = (pathText ?? "").split("/").filter(Boolean).map(Number);
    const root = layout.windows[window]?.[breakpoint]?.root;
    return root && isResizable(nodeAt(root, path)) ? { window, breakpoint, path } : null;
  }
  const windows = Object.keys(layout.windows).sort((a, b) =>
    a === "main" ? -1 : b === "main" ? 1 : a.localeCompare(b),
  );
  for (const window of windows) {
    const breakpoints = layout.windows[window] ?? [];
    const shown = selectBreakpointIndex(breakpoints, width);
    const order = [shown, ...breakpoints.keys()].filter((i, n, all) => all.indexOf(i) === n);
    for (const breakpoint of order) {
      const root = breakpoints[breakpoint]?.root;
      const found = root ? findNodeById(root, key) : null;
      if (found && isResizable(found.node)) return { window, breakpoint, path: found.path };
    }
  }
  return null;
}

/** Sends patches; resolves once the core wrote them. */
export async function updateLayout(client: NailClient, patches: LayoutPatch[]): Promise<void> {
  if (patches.length === 0) return;
  await client.request({ type: "UpdateLayout", patches }, "Done");
}

/** Writes a toolbar's items in a new order (or with new contents). */
export function writeToolbar(client: NailClient, toolbar: string, items: ToolbarItem[]) {
  return updateLayout(client, [{ op: "toolbar-order", toolbar, items }]);
}

/** Binds a key (replacing the binding of that key in the context). */
export function writeKeyBinding(
  client: NailClient,
  context: string,
  key: string,
  action: string,
  args: JsonValue[] = [],
) {
  return updateLayout(client, [{ op: "add-key-binding", context, key, action, args }]);
}

/** Removes a key binding (the core writes `unbind` for default bindings). */
export function removeKeyBinding(client: NailClient, context: string, key: string) {
  return updateLayout(client, [{ op: "remove-key-binding", context, key }]);
}

/** Writes a list widget's column order. */
export function writeColumnOrder(client: NailClient, node: LayoutNodeRef, columns: string[]) {
  return updateLayout(client, [{ op: "column-order", node, columns }]);
}

/**
 * Split-size patches for the entries of `sizes` that differ from what was
 * written last (`written` is updated). Keys without a layout node are
 * skipped.
 */
export function splitPatches(
  sizes: Readonly<Record<string, readonly string[]>>,
  written: Map<string, string>,
  layout: ResolvedLayout,
  width: number,
): LayoutPatch[] {
  const patches: LayoutPatch[] = [];
  for (const [key, value] of Object.entries(sizes)) {
    const text = value.join(" ");
    if (written.get(key) === text) continue;
    const node = splitRefOf(key, layout, width);
    written.set(key, text);
    if (node) patches.push({ op: "split-sizes", node, sizes: [...value] });
  }
  return patches;
}
