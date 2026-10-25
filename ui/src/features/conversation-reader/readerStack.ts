/**
 * The reader stack's migration for layouts of the user's (P2 WP203): a
 * `layout.kdl` written before the card feed keeps its reader stack — the
 * conversation strip (`thread-view`) above the per-message widgets and
 * `message-view` — and keeps working (`thread-view` without `mode=` is the
 * strip). Nail offers once to turn each such stack into the card feed: one
 * `thread-view mode="cards"` in the strip's place, whose cards show the
 * per-message widgets the stack had, in its order. One `UpdateLayout` (one
 * undo step), so Settings → Layout's undo takes it back.
 */
import type {
  JsonValue,
  LayoutNode,
  LayoutNodeRef,
  LayoutPatch,
  NewWidget,
  ResolvedLayout,
} from "../../api/generated";
import { CARD_WIDGETS } from "../../widgets/ThreadViewModel";

/** UI-state key: `"dismissed"` or `"migrated"` once the user answered. */
export const UI_STATE_READER_OFFER = "conversation-reader.layout-offer";

/** The widget kinds that show the open message: the conversation and the lone message. */
export const THREAD_VIEW = "thread-view";
export const MESSAGE_VIEW = "message-view";

type Widget = Extract<LayoutNode, { type: "widget" }>;

/** A reader stack of the strip era: where it is and its children. */
export interface StripStack {
  window: string;
  breakpoint: number;
  path: number[];
  children: readonly LayoutNode[];
}

function isCardKind(kind: string): boolean {
  return (CARD_WIDGETS as readonly string[]).includes(kind);
}

function isStrip(node: LayoutNode): node is Widget {
  return node.type === "widget" && node.kind === THREAD_VIEW && node.options.mode !== "cards";
}

function isMessageView(node: LayoutNode): boolean {
  return node.type === "widget" && node.kind === MESSAGE_VIEW;
}

function collect(
  node: LayoutNode,
  path: number[],
  at: Omit<StripStack, "path" | "children">,
  out: StripStack[],
): void {
  if (node.type === "widget") return;
  if (node.type === "stack" && node.children.some(isStrip) && node.children.some(isMessageView)) {
    out.push({ ...at, path, children: node.children });
  }
  node.children.forEach((child, index) => {
    collect(child, [...path, index], at, out);
  });
}

/** Every reader stack with the strip above a `message-view`. */
export function stripStacks(layout: ResolvedLayout): StripStack[] {
  const out: StripStack[] = [];
  for (const [window, breakpoints] of Object.entries(layout.windows)) {
    breakpoints.forEach((bp, breakpoint) => {
      collect(bp.root, [], { window, breakpoint }, out);
    });
  }
  return out;
}

/** Whether the layout has a reader stack to migrate. */
export function needsReaderMigration(layout: ResolvedLayout): boolean {
  return stripStacks(layout).length > 0;
}

/** The card feed that replaces a stack's strip and per-message widgets. */
export function feedWidget(stack: StripStack): NewWidget {
  const strip = stack.children.find(isStrip);
  const kinds = stack.children
    .filter((c): c is Widget => c.type === "widget" && isCardKind(c.kind) && c.hidden !== true)
    .map((c) => c.kind);
  const options: Record<string, JsonValue> = { mode: "cards" };
  const all = kinds.length === CARD_WIDGETS.length && kinds.every((k, i) => k === CARD_WIDGETS[i]);
  if (!all) options.card = [...new Set(kinds)].join(" ");
  const view = stack.children.find(isMessageView);
  if (view?.type === "widget" && view.options["mark-read"] === false) options["mark-read"] = false;
  return { kind: THREAD_VIEW, id: strip?.id ?? null, label: strip?.label ?? null, options };
}

/**
 * The patches of one stack: the feed goes in where the strip was, then the
 * strip and the per-message widgets leave (from the end, so the indexes of
 * the ones still to go stay valid; the feed first, so the stack never
 * empties).
 */
export function stackPatches(stack: StripStack): LayoutPatch[] {
  const at = (path: number[]): LayoutNodeRef => ({
    window: stack.window,
    breakpoint: stack.breakpoint,
    path,
  });
  const stripIndex = stack.children.findIndex(isStrip);
  const removed = stack.children
    .map((child, index) => ({ child, index }))
    .filter(({ child }) => (child.type === "widget" && isCardKind(child.kind)) || isStrip(child))
    .map(({ index }) => (index >= stripIndex ? index + 1 : index))
    .sort((a, b) => b - a);
  return [
    {
      op: "add-widget",
      widget: feedWidget(stack),
      to: { kind: "into", parent: at(stack.path), index: stripIndex },
    },
    ...removed.map((index): LayoutPatch => ({
      op: "remove-node",
      node: at([...stack.path, index]),
    })),
  ];
}

/**
 * The whole migration as one edit. Stacks are rewritten from the last one
 * of each breakpoint, so a stack's path stays valid while another one in
 * the same tree changes (nested reader stacks are not expected).
 */
export function readerMigrationPatches(layout: ResolvedLayout): LayoutPatch[] {
  return stripStacks(layout).reverse().flatMap(stackPatches);
}
