/**
 * The P1 responsive acceptance standard (ADR 0139, `docs/plan.md` "Wave P1")
 * as a Playwright helper: every view, window and dialog is checked at the
 * standard viewport sizes (and windows at their min-size and resized
 * larger) for
 *
 * - `page-overflow`: the page scrolls horizontally;
 * - `page-scroll`: the page itself scrolls vertically (panes and window
 *   bodies scroll, never the document);
 * - `clipped`: an element cuts its own content off (overflow hidden / clip)
 *   without an ellipsis or a line clamp;
 * - `overflowing`: an element's text sticks out of its box sideways while
 *   the box lets it (overflow visible, e.g. `white-space: nowrap` in a column
 *   that shrank) — the text runs into or under its neighbours;
 * - `offscreen`: a visible control or text sticks out of the viewport
 *   sideways (not inside a horizontally scrolling container);
 * - `overlap`: two controls or text leaves of the same window cover each
 *   other — a text leaf counts with the text that sticks out of it sideways, so
 *   a label running under the select beside it is an overlap;
 * - `misaligned`: the labels of one form column do not share a left edge
 *   (only for the groups a test names in `aligned`);
 * - `undersized-control` (ADR 0160): a text-like control (a text / email /
 *   number / date … input, a textarea, a select, a combo box, a
 *   contenteditable editor, a date / time segment group) left at the browser's
 *   intrinsic width (about 20 characters) although its row offers room: the
 *   free space beside it on its line is at least {@link UNDERSIZED_MIN_FREE}
 *   px and it uses less than {@link UNDERSIZED_RATIO} of the width it could
 *   have, or it stops short of the right edge its column siblings share.
 *   Controls that are small on purpose (a port, minutes, a time, a search box
 *   with its own width) carry `data-width="short" | "medium" | "content"` on
 *   the control or its picker's root; the skin sizes those and the check skips
 *   them.
 *
 * The checks look at visible `[data-part]` elements (and the controls and
 * text leaves among them) inside `scope`. Screenshots go to
 * `<build dir>/screens/p1/<name>-<w>x<h>.png`.
 */
import { mkdirSync } from "node:fs";
import path from "node:path";
import { expect, type Locator, type Page } from "@playwright/test";
import { buildOutputDir } from "./nail";
import { nextFrames } from "./views";

export interface Size {
  width: number;
  height: number;
}

/** The standard widths (px) every package checks. */
export const STANDARD_WIDTHS = [1600, 1280, 1024, 800] as const;
/** The standard heights (px) every package checks. */
export const STANDARD_HEIGHTS = [1000, 700] as const;

/** Every standard width × height, widest and tallest first. */
export const STANDARD_SIZES: readonly Size[] = STANDARD_WIDTHS.flatMap((width) =>
  STANDARD_HEIGHTS.map((height) => ({ width, height })),
);

export type IssueKind =
  | "page-overflow"
  | "page-scroll"
  | "clipped"
  | "overflowing"
  | "offscreen"
  | "overlap"
  | "misaligned"
  | "undersized-control";

/** Free space (px) beside a control on its line that makes it `undersized-control`. */
export const UNDERSIZED_MIN_FREE = 48;
/** A control using less than this share of the width it could have is `undersized-control`. */
export const UNDERSIZED_RATIO = 0.85;
/** A control this far (px) short of its column siblings' shared right edge is `undersized-control`. */
export const UNDERSIZED_EDGE = 24;

export interface ResponsiveIssue {
  kind: IssueKind;
  /** A short description of the element: its `data-part` path and text. */
  element: string;
  /** The other element of an overlap. */
  other?: string;
  /** Numbers that show the problem (overflow in px, rectangles). */
  detail: string;
}

export interface CheckOptions {
  /** Only elements inside this CSS selector (default: the whole document). */
  scope?: string;
  /**
   * CSS selectors of elements (and their subtrees) to leave out, e.g. a
   * known third-party widget. Every entry should say why in the test.
   */
  ignore?: string[];
  /** Kinds not checked (e.g. `page-scroll` on a page that is meant to scroll). */
  skip?: IssueKind[];
  /**
   * Groups whose labels must share a left edge: each entry is a CSS
   * selector of the labels of one form column.
   */
  aligned?: string[];
  /** Tolerance in CSS px (default 1). */
  tolerance?: number;
}

/** The raw facts a page reports; shaped like {@link ResponsiveIssue}. */
type RawIssue = ResponsiveIssue;

/** What the page-side check gets: the options plus the `undersized-control` limits. */
type PageOptions = Required<CheckOptions> & { minFree: number; ratio: number; edge: number };

/**
 * Runs inside the page: collects the issues of the current layout. It is
 * self-contained (serialized by Playwright), so it uses no imports.
 */
function collectIssues(options: PageOptions): RawIssue[] {
  const tol = options.tolerance;
  const issues: RawIssue[] = [];
  const skip = new Set(options.skip);
  const root: ParentNode = options.scope
    ? (document.querySelector(options.scope) ?? document)
    : document;
  const ignored = (el: Element) => options.ignore.some((selector) => el.closest(selector) !== null);

  const describe = (el: Element): string => {
    const parts: string[] = [];
    for (let at: Element | null = el; at && parts.length < 3; at = at.parentElement) {
      const part = at.getAttribute("data-part");
      if (part !== null) parts.unshift(part);
    }
    const tag = el.tagName.toLowerCase();
    const text = el.textContent.replace(/\s+/g, " ").trim().slice(0, 40);
    const label = el.getAttribute("aria-label") ?? "";
    return `${parts.join(" > ") || tag}${el.getAttribute("data-part") === null ? ` ${tag}` : ""}${
      text ? ` "${text}"` : ""
    }${label ? ` [${label}]` : ""}`;
  };
  const fmt = (r: { left: number; top: number; width: number; height: number }) =>
    `${String(Math.round(r.left))},${String(Math.round(r.top))} ${String(Math.round(r.width))}×${String(Math.round(r.height))}`;

  const doc = document.documentElement;
  // The body may hide its overflow (the app does): its scroll size still shows content
  // wider or taller than the viewport, which is then cut off instead of scrolling.
  const pageWidth = Math.max(doc.scrollWidth, document.body.scrollWidth);
  const pageHeight = Math.max(doc.scrollHeight, document.body.scrollHeight);
  if (!skip.has("page-overflow") && pageWidth > doc.clientWidth + tol) {
    issues.push({
      kind: "page-overflow",
      element: "document",
      detail: `scrollWidth ${String(pageWidth)} > ${String(doc.clientWidth)}`,
    });
  }
  if (!skip.has("page-scroll") && pageHeight > doc.clientHeight + tol) {
    issues.push({
      kind: "page-scroll",
      element: "document",
      detail: `scrollHeight ${String(pageHeight)} > ${String(doc.clientHeight)}`,
    });
  }

  const visible = (el: Element) => {
    if (!(el instanceof HTMLElement) && !(el instanceof SVGElement)) return false;
    if (!el.checkVisibility({ checkOpacity: true, checkVisibilityCSS: true })) return false;
    const r = el.getBoundingClientRect();
    // Visually hidden text for screen readers is 1 × 1 px and clipped on purpose.
    return r.width > 1 && r.height > 1;
  };
  const clips = (style: CSSStyleDeclaration, axis: "x" | "y") => {
    const value = axis === "x" ? style.overflowX : style.overflowY;
    return value !== "visible";
  };
  interface Box {
    left: number;
    top: number;
    right: number;
    bottom: number;
    width: number;
    height: number;
  }
  /** `r` cut by every clipping element from `from` up (its visible part). */
  const clipRect = (r: Omit<Box, "width" | "height">, from: Element | null): Box => {
    let { left, top, right, bottom } = r;
    for (let at = from; at; at = at.parentElement) {
      const style = getComputedStyle(at);
      if (clips(style, "x") || clips(style, "y")) {
        const a = at.getBoundingClientRect();
        if (clips(style, "x")) {
          left = Math.max(left, a.left);
          right = Math.min(right, a.right);
        }
        if (clips(style, "y")) {
          top = Math.max(top, a.top);
          bottom = Math.min(bottom, a.bottom);
        }
      }
      if (style.position === "fixed") break;
    }
    return { left, top, right, bottom, width: right - left, height: bottom - top };
  };
  /** The element's rectangle cut by every clipping ancestor (its visible part). */
  const visibleRect = (el: Element) => clipRect(el.getBoundingClientRect(), el.parentElement);
  /**
   * The visible rectangles of the element's text: its text nodes, also
   * inside plain descendants, but not inside a descendant `[data-part]`
   * (that one is checked itself). Each is cut by its clipping ancestors.
   */
  const textRects = (el: Element): Box[] => {
    const rects: Box[] = [];
    const walker = document.createTreeWalker(el, NodeFilter.SHOW_TEXT);
    for (let node = walker.nextNode(); node; node = walker.nextNode()) {
      const parent = node.parentElement;
      if (!parent || (node.textContent ?? "").trim() === "") continue;
      const owner = parent.closest("[data-part]");
      if (owner !== null && owner !== el && el.contains(owner)) continue;
      const range = document.createRange();
      range.selectNodeContents(node);
      for (const r of Array.from(range.getClientRects())) {
        const cut = clipRect(r, parent);
        if (cut.width > tol && cut.height > tol) rects.push(cut);
      }
    }
    return rects;
  };
  /** The smallest rectangle around `boxes`. */
  const union = (boxes: readonly Box[]): Box | undefined => {
    if (boxes.length === 0) return undefined;
    const left = Math.min(...boxes.map((b) => b.left));
    const top = Math.min(...boxes.map((b) => b.top));
    const right = Math.max(...boxes.map((b) => b.right));
    const bottom = Math.max(...boxes.map((b) => b.bottom));
    return { left, top, right, bottom, width: right - left, height: bottom - top };
  };
  /**
   * A leaf's visible box widened by the text that sticks out of it
   * sideways. Only sideways: a text line's glyph box is a little taller
   * than its line (ascent + descent > line-height), so stacked lines would
   * "overlap" by a pixel or two that nobody can see.
   */
  const withSpill = (el: Element): Box => {
    const box = visibleRect(el);
    const text = union(textRects(el));
    if (!text) return box;
    const left = Math.min(box.left, text.left);
    const right = Math.max(box.right, text.right);
    return { ...box, left, right, width: right - left };
  };
  const scrollsSideways = (el: Element) => {
    for (let at = el.parentElement; at && at !== doc; at = at.parentElement) {
      const x = getComputedStyle(at).overflowX;
      if (x === "auto" || x === "scroll") return true;
    }
    return false;
  };
  const hasOwnText = (el: Element) =>
    Array.from(el.childNodes).some(
      (node) => node.nodeType === Node.TEXT_NODE && (node.textContent ?? "").trim() !== "",
    );
  const isControl = (el: Element) =>
    el.matches(
      'button, a[href], input:not([type="hidden"]), select, textarea, [role="button"], [role="tab"], [role="checkbox"], [role="switch"], [role="radio"], [role="menuitem"], [role="option"], [role="slider"], [role="combobox"]',
    );

  const parts = Array.from(root.querySelectorAll("[data-part]")).filter(
    (el) => visible(el) && !ignored(el),
  );

  // Clipped and off-screen elements.
  for (const el of parts) {
    const style = getComputedStyle(el);
    const lineClamp = style.getPropertyValue("-webkit-line-clamp");
    const clampsLines = lineClamp !== "" && lineClamp !== "none";
    if (!skip.has("clipped")) {
      const ellipsis = style.textOverflow === "ellipsis";
      const xClipped =
        (style.overflowX === "hidden" || style.overflowX === "clip") &&
        el.scrollWidth > el.clientWidth + tol &&
        !ellipsis;
      const yClipped =
        (style.overflowY === "hidden" || style.overflowY === "clip") &&
        el.scrollHeight > el.clientHeight + tol &&
        !clampsLines &&
        hasOwnText(el);
      if (xClipped || yClipped) {
        issues.push({
          kind: "clipped",
          element: describe(el),
          detail: xClipped
            ? `content ${String(el.scrollWidth)}px wide in ${String(el.clientWidth)}px`
            : `content ${String(el.scrollHeight)}px high in ${String(el.clientHeight)}px`,
        });
      }
    }
    if (
      !skip.has("overflowing") &&
      style.overflowX === "visible" &&
      style.textOverflow !== "ellipsis"
    ) {
      // Text wider than its box that the box does not cut: it runs into the
      // neighbours (or under them, when they paint later).
      const box = el.getBoundingClientRect();
      const text = union(textRects(el));
      if (text && (text.right > box.right + tol || text.left < box.left - tol)) {
        issues.push({
          kind: "overflowing",
          element: describe(el),
          detail: `text ${fmt(text)} outside its box ${fmt(box)}`,
        });
      }
    }
    if (!skip.has("offscreen") && (isControl(el) || hasOwnText(el)) && !scrollsSideways(el)) {
      const r = el.getBoundingClientRect();
      if (r.right > window.innerWidth + tol || r.left < -tol) {
        issues.push({
          kind: "offscreen",
          element: describe(el),
          detail: `${fmt(r)} in a ${String(window.innerWidth)}px viewport`,
        });
      }
    }
  }

  // Overlapping controls / text leaves of the same window.
  if (!skip.has("overlap")) {
    const leaves = Array.from(root.querySelectorAll("*")).filter(
      (el) =>
        (isControl(el) || (el.hasAttribute("data-part") && hasOwnText(el))) &&
        visible(el) &&
        !ignored(el),
    );
    const boxes = leaves
      .map((el) => ({
        el,
        rect: withSpill(el),
        layer: el.closest(
          '[data-part="window"], [role="menu"], [role="listbox"], [role="tooltip"]',
        ),
      }))
      .filter((b) => b.rect.width > tol && b.rect.height > tol);
    for (let i = 0; i < boxes.length; i += 1) {
      const a = boxes[i];
      if (a === undefined) continue;
      for (let j = i + 1; j < boxes.length; j += 1) {
        const b = boxes[j];
        if (b === undefined) continue;
        if (b.layer !== a.layer) continue;
        if (a.el.contains(b.el) || b.el.contains(a.el)) continue;
        const w = Math.min(a.rect.right, b.rect.right) - Math.max(a.rect.left, b.rect.left);
        const h = Math.min(a.rect.bottom, b.rect.bottom) - Math.max(a.rect.top, b.rect.top);
        if (w > tol && h > tol) {
          issues.push({
            kind: "overlap",
            element: describe(a.el),
            other: describe(b.el),
            detail: `${fmt(a.rect)} and ${fmt(b.rect)} share ${String(Math.round(w))}×${String(Math.round(h))}px`,
          });
        }
      }
    }
  }

  // Label columns.
  if (!skip.has("misaligned")) {
    for (const selector of options.aligned) {
      const labels = Array.from(root.querySelectorAll(selector)).filter(visible);
      const lefts = labels.map((el) => el.getBoundingClientRect().left);
      const first = lefts[0];
      if (first === undefined) continue;
      labels.forEach((el, index) => {
        const left = lefts[index] ?? first;
        if (Math.abs(left - first) > tol) {
          issues.push({
            kind: "misaligned",
            element: describe(el),
            detail: `left ${String(Math.round(left))} ≠ ${String(Math.round(first))} (${selector})`,
          });
        }
      });
    }
  }

  // Controls left at the browser's intrinsic width although their row has room.
  if (!skip.has("undersized-control")) {
    issues.push(...undersizedControls());
  }
  return issues;

  /** The `undersized-control` issues (see the module comment). */
  function undersizedControls(): RawIssue[] {
    const found: RawIssue[] = [];
    // Layout, not visibility: a field scrolled out of its pane still has its row.
    const layoutRect = (r: DOMRect): Box => ({
      left: r.left,
      top: r.top,
      right: r.right,
      bottom: r.bottom,
      width: r.width,
      height: r.height,
    });
    const layoutBox = (el: Element) => layoutRect(el.getBoundingClientRect());
    const editable =
      '[contenteditable=""], [contenteditable="true"], [contenteditable="plaintext-only"]';
    const candidates = Array.from(
      root.querySelectorAll(
        [
          'input:not([type="checkbox"], [type="radio"], [type="range"], [type="color"], [type="file"], [type="hidden"], [type="button"], [type="submit"], [type="reset"], [type="image"])',
          "textarea",
          "select",
          '[role="combobox"]',
          editable,
          // A date / time field drawn as segments (react-aria's DateInput).
          '[role="group"]:has(> [role="spinbutton"])',
        ].join(", "),
      ),
    ).filter(
      (el) =>
        visible(el) &&
        !ignored(el) &&
        el.closest("[data-width]") === null &&
        // The segments of a date / time field: the field (their group) is checked.
        !el.matches('[role="spinbutton"]') &&
        // A library's hidden form proxy (react-aria's date input), not shown to anyone.
        el.closest('[aria-hidden="true"]') === null &&
        // An editor's inner editable nodes; a combo box's own input inside its root.
        el.parentElement?.closest(`${editable}, [role="combobox"]`) == null,
    );
    if (candidates.length === 0) return found;

    /** What occupies a line: controls (their box), text (its glyphs), pictures. */
    interface Occupant {
      el: Element;
      box: Box;
    }
    const occupants: Occupant[] = [];
    for (const el of Array.from(root.querySelectorAll("*"))) {
      if (!visible(el) || ignored(el)) continue;
      if (isControl(el) || el.matches("img, svg, canvas, video, [role='img']")) {
        if (el.parentElement?.closest("svg") != null) continue;
        occupants.push({ el, box: layoutRect(el.getBoundingClientRect()) });
        continue;
      }
      for (const node of Array.from(el.childNodes)) {
        if (node.nodeType !== Node.TEXT_NODE || (node.textContent ?? "").trim() === "") continue;
        const range = document.createRange();
        range.selectNodeContents(node);
        const text = union(Array.from(range.getClientRects()).map(layoutRect));
        if (text && text.width > tol && text.height > tol) occupants.push({ el, box: text });
      }
    }
    /** Occupants of `within` on the horizontal band of `box` (half its height at least). */
    const onBand = (box: Box, within: Element, except: Element) =>
      occupants.filter((o) => {
        if (!within.contains(o.el) || except.contains(o.el) || o.el.contains(except)) return false;
        const shared = Math.min(o.box.bottom, box.bottom) - Math.max(o.box.top, box.top);
        return shared >= Math.min(box.height, o.box.height) / 2;
      });
    /** `el`'s content box (inside border, padding and scroll bar). */
    const contentBox = (el: Element) => {
      const r = el.getBoundingClientRect();
      const s = getComputedStyle(el);
      const left = r.left + parseFloat(s.borderLeftWidth) + parseFloat(s.paddingLeft);
      const right =
        r.left + parseFloat(s.borderLeftWidth) + el.clientWidth - parseFloat(s.paddingRight);
      return { left, right };
    };
    /** Whether `el`'s width comes from its content rather than from its container. */
    const shrinkWraps = (el: Element) => {
      const s = getComputedStyle(el);
      if (s.display.startsWith("inline") || s.float !== "none") return true;
      if (s.position === "absolute" || s.position === "fixed") return true;
      const parent = el.parentElement;
      if (!parent) return false;
      const ps = getComputedStyle(parent);
      if (ps.display === "flex" || ps.display === "inline-flex") {
        if (ps.flexDirection.startsWith("row")) return parseFloat(s.flexGrow) === 0;
        const align = s.alignSelf === "auto" ? ps.alignItems : s.alignSelf;
        return !["normal", "stretch"].includes(align);
      }
      if (ps.display === "grid" || ps.display === "inline-grid") {
        const justify = s.justifySelf === "auto" ? ps.justifyItems : s.justifySelf;
        return !["normal", "stretch", "legacy", "auto"].includes(justify);
      }
      return false;
    };
    /** Layers that bound the walk up from a control. */
    const boundary =
      '[data-part="window"], [data-part="view-host.view"], [role="dialog"], [role="menu"], [role="listbox"], body';

    for (const control of candidates) {
      const own = layoutBox(control);
      if (own.width <= tol) continue;
      // Rule 1: free room beside the control (or the content-sized box around it).
      let item: Element = control;
      let reason: string | null = null;
      for (let depth = 0; depth < 8; depth += 1) {
        const parent = item.parentElement;
        if (!parent || item.matches(boundary)) break;
        if (shrinkWraps(parent) && !parent.matches(boundary)) {
          item = parent;
          continue;
        }
        const ps = getComputedStyle(parent);
        const s = getComputedStyle(item);
        const flexRow =
          (ps.display === "flex" || ps.display === "inline-flex") &&
          ps.flexDirection.startsWith("row");
        // Stretched by its grid area, or growing in its flex row: it takes what it gets.
        const grid = ps.display === "grid" || ps.display === "inline-grid";
        if (grid && !shrinkWraps(item)) break;
        if (flexRow && parseFloat(s.flexGrow) > 0) break;
        if (!shrinkWraps(item) && !flexRow && !item.matches("input, select, textarea")) break;
        const box = layoutBox(item);
        const room = contentBox(parent);
        const middle = (box.left + box.right) / 2;
        const band = onBand(box, parent, item);
        const next = Math.min(
          room.right,
          ...band
            .filter((o) => (o.box.left + o.box.right) / 2 > middle)
            .map((o) => Math.max(o.box.left, box.right)),
        );
        const previous = Math.max(
          room.left,
          ...band
            .filter((o) => (o.box.left + o.box.right) / 2 <= middle)
            .map((o) => Math.min(o.box.right, box.left)),
        );
        const right = Math.max(0, next - box.right);
        const free = right > tol ? right : Math.max(0, box.left - previous);
        if (free >= options.minFree && own.width < options.ratio * (own.width + free)) {
          reason = `${String(Math.round(own.width))}px wide with ${String(Math.round(free))}px free beside it in ${describe(parent)}`;
        }
        break;
      }
      // Rule 2: short of the right edge its column siblings share.
      if (reason === null) {
        const layer = control.closest(
          'form, fieldset, [role="form"], [data-part="card"], [data-part="form.section"], [data-part="window"], [data-part="view-host.view"]',
        );
        const column = candidates.filter((other) => {
          if (other === control) return false;
          if (!(layer ?? document.body).contains(other)) return false;
          return Math.abs(layoutBox(other).left - own.left) <= 2;
        });
        const rights = column.map((other) => layoutBox(other).right);
        const shared = rights.filter((r) => rights.filter((x) => Math.abs(x - r) <= 2).length >= 2);
        const edge = shared.length > 0 ? Math.max(...shared) : undefined;
        if (edge !== undefined && edge - own.right >= options.edge) {
          const band = onBand(own, layer ?? document.body, control).filter(
            (o) => o.box.left < edge && o.box.right > own.right + tol,
          );
          if (band.length === 0) {
            reason = `ends at ${String(Math.round(own.right))}, its column's controls at ${String(Math.round(edge))}`;
          }
        }
      }
      if (reason !== null) {
        found.push({ kind: "undersized-control", element: describe(control), detail: reason });
      }
    }
    return found;
  }
}

/** The issues of the page's current layout (no resizing). */
export async function layoutIssues(
  page: Page,
  options: CheckOptions = {},
): Promise<ResponsiveIssue[]> {
  return page.evaluate(collectIssues, {
    scope: options.scope ?? "",
    ignore: options.ignore ?? [],
    skip: options.skip ?? [],
    aligned: options.aligned ?? [],
    tolerance: options.tolerance ?? 1,
    minFree: UNDERSIZED_MIN_FREE,
    ratio: UNDERSIZED_RATIO,
    edge: UNDERSIZED_EDGE,
  });
}

/** A readable list of issues for an assertion message. */
export function formatIssues(issues: readonly ResponsiveIssue[]): string {
  return issues
    .map(
      (i) => `${i.kind}: ${i.element}${i.other === undefined ? "" : ` / ${i.other}`} (${i.detail})`,
    )
    .join("\n");
}

/** `<build dir>/screens/p1/<name>-<w>x<h>.png` (the directory is created). */
export function screenshotPath(name: string, size: Size): string {
  const dir = buildOutputDir(path.join("screens", "p1"));
  mkdirSync(dir, { recursive: true });
  const safe = name.replace(/[^\w.-]+/g, "-");
  return path.join(dir, `${safe}-${String(size.width)}x${String(size.height)}.png`);
}

export interface SweepOptions extends CheckOptions {
  /** Sizes to check (default {@link STANDARD_SIZES}). */
  sizes?: readonly Size[];
  /**
   * Brings the page into the state to check after each resize (open the
   * dialog, select the message, …); runs once per size.
   */
  prepare?: (page: Page, size: Size) => Promise<void>;
  /** Whether to save a screenshot per size (default true). */
  screenshot?: boolean;
}

export interface SizeReport {
  size: Size;
  issues: ResponsiveIssue[];
  screenshot: string | null;
}

/**
 * Resizes the viewport to every size, prepares the page, collects the
 * issues and saves a screenshot. Returns the report per size; use
 * {@link expectResponsive} to assert there are none.
 */
export async function checkResponsive(
  page: Page,
  name: string,
  options: SweepOptions = {},
): Promise<SizeReport[]> {
  const reports: SizeReport[] = [];
  for (const size of options.sizes ?? STANDARD_SIZES) {
    await page.setViewportSize(size);
    if (options.prepare) await options.prepare(page, size);
    // Let layout effects (measured toolbars, virtual lists) settle.
    await nextFrames(page);
    const issues = await layoutIssues(page, options);
    let shot: string | null = null;
    if (options.screenshot ?? true) {
      shot = screenshotPath(name, size);
      await page.screenshot({ path: shot });
    }
    reports.push({ size, issues, screenshot: shot });
  }
  return reports;
}

/** {@link checkResponsive}, failing with every issue per size when there are any. */
export async function expectResponsive(
  page: Page,
  name: string,
  options: SweepOptions = {},
): Promise<SizeReport[]> {
  const reports = await checkResponsive(page, name, options);
  const failing = reports
    .filter((r) => r.issues.length > 0)
    .map(
      (r) =>
        `${name} at ${String(r.size.width)}×${String(r.size.height)}:\n${formatIssues(r.issues)}`,
    );
  expect(failing, failing.join("\n\n")).toEqual([]);
  return reports;
}

/**
 * Resizes a window-manager window (`[data-part="window"]`) by dragging its
 * bottom-right handle so its size becomes `size`; the window manager clamps
 * to the window's min-size and the viewport. `{ width: 1, height: 1 }`
 * shrinks it to its min-size. Returns the size it ended at.
 */
export async function resizeWindow(page: Page, frame: Locator, size: Size): Promise<Size> {
  const handle = frame.locator(':scope > [data-part="window.handle"][data-edge="se"]');
  const box = await frame.boundingBox();
  const grip = await handle.boundingBox();
  if (!box || !grip) throw new Error("the window or its resize handle is not visible");
  const startX = grip.x + grip.width / 2;
  const startY = grip.y + grip.height / 2;
  await page.mouse.move(startX, startY);
  await page.mouse.down();
  await page.mouse.move(
    startX + (size.width - box.width) / 2,
    startY + (size.height - box.height) / 2,
    {
      steps: 4,
    },
  );
  await page.mouse.move(startX + size.width - box.width, startY + size.height - box.height, {
    steps: 4,
  });
  await page.mouse.up();
  const after = await frame.boundingBox();
  if (!after) throw new Error("the window disappeared while resizing");
  return { width: Math.round(after.width), height: Math.round(after.height) };
}

/** Expects no layout issue in the elements whose description names one of `parts`. */
export async function expectNoIssuesIn(page: Page, parts: readonly string[]): Promise<void> {
  const issues = (await layoutIssues(page)).filter((i) => parts.some((p) => i.element.includes(p)));
  expect(issues, formatIssues(issues)).toEqual([]);
}

/** In the narrow layout (one pane at a time): shows the message list's tab. */
export async function showListTab(page: Page): Promise<void> {
  const tab = page.getByRole("tab", { name: "Messages", exact: true });
  if (await tab.isVisible()) await tab.click();
}
