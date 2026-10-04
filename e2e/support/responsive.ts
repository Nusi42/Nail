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
 * - `offscreen`: a visible control or text sticks out of the viewport
 *   sideways (not inside a horizontally scrolling container);
 * - `overlap`: two controls or text leaves of the same window cover each
 *   other;
 * - `misaligned`: the labels of one form column do not share a left edge
 *   (only for the groups a test names in `aligned`).
 *
 * The checks look at visible `[data-part]` elements (and the controls and
 * text leaves among them) inside `scope`. Screenshots go to
 * `<build dir>/screens/p1/<name>-<w>x<h>.png`.
 */
import { mkdirSync } from "node:fs";
import path from "node:path";
import { expect, type Locator, type Page } from "@playwright/test";
import { buildOutputDir } from "./nail";

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
  "page-overflow" | "page-scroll" | "clipped" | "offscreen" | "overlap" | "misaligned";

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

/**
 * Runs inside the page: collects the issues of the current layout. It is
 * self-contained (serialized by Playwright), so it uses no imports.
 */
function collectIssues(options: Required<CheckOptions>): RawIssue[] {
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
      text ? ` "${text}"` : label ? ` [${label}]` : ""
    }`;
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
  /** The element's rectangle cut by every clipping ancestor (its visible part). */
  const visibleRect = (el: Element) => {
    const r = el.getBoundingClientRect();
    let { left, top, right, bottom } = r;
    for (let at = el.parentElement; at; at = at.parentElement) {
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
        rect: visibleRect(el),
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
  return issues;
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
    await page.evaluate(
      () =>
        new Promise<void>((resolve) => {
          requestAnimationFrame(() => {
            requestAnimationFrame(() => {
              resolve();
            });
          });
        }),
    );
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
