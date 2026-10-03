/**
 * Pure parts of the layout engine: breakpoint selection, size parsing, the
 * split-resize algorithm and default stack sizes.
 */
import type { Breakpoint, LayoutNode } from "../api/generated";

/** First breakpoint whose (inclusive) bounds contain `width`, else the first one. */
export function selectBreakpoint(
  breakpoints: readonly Breakpoint[],
  width: number,
): Breakpoint | undefined {
  return (
    breakpoints.find(
      (bp) =>
        (bp.min_width === null || width >= bp.min_width) &&
        (bp.max_width === null || width <= bp.max_width),
    ) ?? breakpoints[0]
  );
}

/** Index of the selected breakpoint (see `selectBreakpoint`), or -1. */
export function selectBreakpointIndex(breakpoints: readonly Breakpoint[], width: number): number {
  const bp = selectBreakpoint(breakpoints, width);
  return bp ? breakpoints.indexOf(bp) : -1;
}

const SIZE_RE = /^(\d+(?:\.\d+)?)(px|fr|%|em|rem|ch|vw|vh)$/;

export interface ParsedSize {
  value: number;
  unit: string;
}

/** Parses a CSS track size from the layout (`"240px"`, `"1.4fr"`, `"30%"`). */
export function parseSize(size: string | null | undefined): ParsedSize | null {
  const match = SIZE_RE.exec((size ?? "").trim());
  if (!match) return null;
  return { value: Number(match[1]), unit: match[2] as string };
}

/** A size is valid for CSS output when it parses or is `auto`. */
export function isValidSize(size: string): boolean {
  return size === "auto" || parseSize(size) !== null;
}

function px(size: string | null | undefined): number | null {
  const parsed = parseSize(size);
  return parsed?.unit === "px" ? parsed.value : null;
}

/** `flex` shorthand for a split pane of the given size. */
export function flexForSize(size: string | undefined): string {
  if (size === undefined || size === "auto") return "1 1 auto";
  const parsed = parseSize(size);
  if (!parsed) return "1 1 0px";
  if (parsed.unit === "fr") return `${parsed.value} 1 0px`;
  return `0 0 ${size}`;
}

/** Grid track for a stack child (`1fr` may shrink below its content). */
export function trackForSize(size: string): string {
  const parsed = parseSize(size);
  if (parsed?.unit === "fr") return `minmax(0, ${size})`;
  return isValidSize(size) ? size : "auto";
}

/**
 * The grid rows of a stack for what its children actually rendered. A
 * widget may render nothing (a banner without a reason to show, the config
 * errors while all files are valid); its track must not go to the next
 * child, or a bar gets the `1fr` row and the flexible child an `auto` one.
 *
 * `marks` lists the stack's DOM children in order: the index of the visible
 * child a slot marker stands before, or `null` for a rendered element.
 * The first element after a marker takes that child's track; further
 * elements of the same child (a fragment) get `auto`.
 */
export function renderedStackTracks(
  tracks: readonly string[],
  marks: readonly (number | null)[],
): string[] {
  const out: string[] = [];
  let slot: number | null = null;
  let used = false;
  for (const mark of marks) {
    if (mark !== null) {
      slot = mark;
      used = false;
      continue;
    }
    out.push(slot !== null && !used ? (tracks[slot] ?? "auto") : "auto");
    used = true;
  }
  return out;
}

const BAR_WIDGETS = new Set(["toolbar", "status-bar", "account-switcher", "text"]);

/** Default stack size of a child: `auto` for bar-like widgets, else `1fr`. */
export function defaultStackSize(node: LayoutNode): string {
  return node.type === "widget" && BAR_WIDGETS.has(node.kind) ? "auto" : "1fr";
}

function round(value: number): number {
  return Math.round(value * 1000) / 1000;
}

export interface ResizeInput {
  /** Current sizes, one per (visible) child; `[]` = all `1fr`. */
  sizes: readonly string[];
  /** Measured pixel sizes of the children (0 when unknown). */
  measured: readonly number[];
  /** Divider index: between child `index` and `index + 1`. */
  index: number;
  /** Pixels to move the divider (positive = towards the end). */
  delta: number;
  minSizes?: readonly (string | null)[];
  maxSizes?: readonly (string | null)[];
}

/**
 * Moves a split divider. Fixed-size panes (px, %, …) become pixel sizes; when
 * both neighbours are fractions, their `fr` weights are redistributed so the
 * split stays fluid. Movement is clamped by pixel min / max sizes.
 */
export function resizeSplit(input: ResizeInput): string[] {
  const count = input.measured.length;
  const sizes = Array.from({ length: count }, (_, i) => input.sizes[i] ?? "1fr");
  const i = input.index;
  const j = i + 1;
  if (i < 0 || j >= count) return sizes;

  const effective = (k: number) => {
    const measured = input.measured[k] ?? 0;
    return measured > 0 ? measured : (px(sizes[k]) ?? 0);
  };
  const wi = effective(i);
  const wj = effective(j);
  let delta = input.delta;

  const minI = px(input.minSizes?.[i]) ?? 0;
  const maxI = px(input.maxSizes?.[i]);
  const minJ = px(input.minSizes?.[j]) ?? 0;
  const maxJ = px(input.maxSizes?.[j]);
  delta = Math.max(delta, minI - wi);
  if (maxI !== null) delta = Math.min(delta, maxI - wi);
  if (wj > 0) {
    delta = Math.min(delta, wj - minJ);
    if (maxJ !== null) delta = Math.max(delta, wj - maxJ);
  }
  if (delta === 0) return sizes;

  const fi = parseSize(sizes[i]);
  const fj = parseSize(sizes[j]);
  if (fi?.unit !== "fr") {
    sizes[i] = `${round(Math.max(0, wi + delta))}px`;
  } else if (fj?.unit !== "fr") {
    sizes[j] = `${round(Math.max(0, wj - delta))}px`;
  } else {
    const total = wi + wj;
    if (total <= 0) return sizes;
    const weights = fi.value + fj.value;
    const newI = round((weights * (wi + delta)) / total);
    sizes[i] = `${newI}fr`;
    sizes[j] = `${round(weights - newI)}fr`;
  }
  return sizes;
}
