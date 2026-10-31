/**
 * How a mail frame reports its content height (P3, ADR 0164; slot of
 * WP-card-height). A sandboxed frame without scripts could not tell the
 * reader how tall its message is, so HTML cards kept a 40vh minimum and the
 * reader scrolled twice (P1, P2). The contract's design:
 *
 * - **Local frames** (`srcdoc`: desktop, `nail serve` on loopback):
 *   `sandbox="allow-same-origin"` and nothing else — no `allow-scripts`,
 *   ever ({@link SRCDOC_SANDBOX}). The document stays script-free (the
 *   sandbox, its own `script-src`-less CSP and the page's CSP) but is no
 *   longer an opaque origin, so the reader reads
 *   `contentDocument.documentElement.scrollHeight` after `load`, after
 *   each image's `load` / `error` (capture listeners the parent adds) and
 *   when the frame's width changes, and sizes the frame to it.
 * - **Remote frames** (`render_url` on the sandbox origin): cross-origin,
 *   so the sandbox document carries one fixed script (hash-pinned in its
 *   CSP; the mail's own scripts are stripped by the sanitizer and blocked by
 *   the hash) that posts `{type: FRAME_HEIGHT_MESSAGE, height}` to the app
 *   origin named in the `parent` query parameter ({@link RENDER_PARENT_PARAM},
 *   checked against `frame-ancestors`). The frame keeps an opaque origin
 *   (`sandbox allow-scripts` without `allow-same-origin`,
 *   {@link REMOTE_SANDBOX}): the script reads no cookies or storage. The
 *   reader trusts only {@link frameHeightOf}.
 *
 * Heights are clamped to `[0, MAX_FRAME_HEIGHT]`; a taller document scrolls
 * inside its frame (wide mail keeps its horizontal scrollbar: owner
 * decision 2026-10-05).
 */
import { FRAME_HEIGHT_MESSAGE, MAX_FRAME_HEIGHT } from "../api/generated";
export { FRAME_HEIGHT_MESSAGE, MAX_FRAME_HEIGHT, RENDER_PARENT_PARAM } from "../api/generated";

/** The `sandbox` attribute of a local (`srcdoc`) mail frame. Never add `allow-scripts`. */
export const SRCDOC_SANDBOX = "allow-same-origin";
/** The `sandbox` attribute of a remote (sandbox-origin) mail frame. Never add `allow-same-origin`. */
export const REMOTE_SANDBOX = "allow-scripts";

/** A height clamped to what the reader sizes a frame to. */
export function clampFrameHeight(height: number): number {
  if (!Number.isFinite(height) || height < 0) return 0;
  return Math.min(Math.ceil(height), MAX_FRAME_HEIGHT);
}

/**
 * The origin a sandboxed document without `allow-same-origin` posts from:
 * the opaque origin serializes as `"null"`. The sandbox-origin frame is
 * such a document (the CSP's and the iframe's `sandbox` lack
 * `allow-same-origin`), so its messages arrive with this origin; the
 * source window (the reader's own frame, which no other document can
 * impersonate) is what identifies them.
 */
export const OPAQUE_ORIGIN = "null";

/** A message event as the reader's frames see it. */
export type FrameMessageEvent = Pick<MessageEvent, "data" | "origin" | "source">;

/**
 * The fields of a message a sandbox-origin frame posted with `type`, or
 * `null` unless it comes from `frame`'s own window — on `sandboxOrigin`, or
 * the opaque origin the sandbox gives that document ({@link OPAQUE_ORIGIN})
 * — and is an object of that type. Shared by the height and the link
 * messages (ADR 0206).
 */
export function framePosted(
  event: FrameMessageEvent,
  frame: Window | null,
  sandboxOrigin: string,
  type: string,
): Record<string, unknown> | null {
  if (frame === null || event.source !== frame) return null;
  if (event.origin !== sandboxOrigin && event.origin !== OPAQUE_ORIGIN) return null;
  const data: unknown = event.data;
  if (typeof data !== "object" || data === null) return null;
  const fields = data as Record<string, unknown>;
  return fields.type === type ? fields : null;
}

/**
 * The height a sandbox-origin frame posted, or `null` unless the message
 * comes from `frame`'s own window ({@link framePosted}) and has the
 * contract's shape.
 */
export function frameHeightOf(
  event: FrameMessageEvent,
  frame: Window | null,
  sandboxOrigin: string,
): number | null {
  const height = framePosted(event, frame, sandboxOrigin, FRAME_HEIGHT_MESSAGE)?.height;
  return typeof height === "number" ? clampFrameHeight(height) : null;
}

/**
 * What the measurement reads of a mail document (its root element and its
 * window). The same arithmetic runs in the sandbox route's height script
 * (`crates/nail-server/src/sandbox.rs`, `HEIGHT_SCRIPT`).
 */
export interface FrameMetrics {
  /** `documentElement.clientHeight`: the frame's viewport without a horizontal scrollbar. */
  clientHeight: number;
  /** `documentElement.scrollHeight`: the viewport, or the content when it overflows. */
  scrollHeight: number;
  /** `documentElement.offsetHeight`: the root's own height (content-sized, unless the mail sets one). */
  offsetHeight: number;
  /** `window.innerHeight`: the viewport with its scrollbar. */
  innerHeight: number;
}

/**
 * The frame height that shows the whole document: its overflow when it
 * overflows the frame, else its root's height (so a frame can shrink when
 * the content does, e.g. after a width change), plus a horizontal
 * scrollbar's thickness (wide mail keeps its scrollbar under the last line).
 */
export function contentHeight(m: FrameMetrics): number {
  const content = m.scrollHeight > m.clientHeight ? m.scrollHeight : m.offsetHeight;
  return clampFrameHeight(content + Math.max(0, m.innerHeight - m.clientHeight));
}

/** {@link FrameMetrics} of a loaded document, `null` before it has a root. */
export function documentMetrics(doc: Document): FrameMetrics | null {
  const root = doc.documentElement as HTMLElement | null;
  const view = doc.defaultView;
  if (root === null || view === null) return null;
  return {
    clientHeight: root.clientHeight,
    scrollHeight: root.scrollHeight,
    offsetHeight: root.offsetHeight,
    innerHeight: view.innerHeight,
  };
}

/** Growths by the same amount in a row that count as a runaway (`100vh` + padding). */
const RUNAWAY_STEPS = 40;

/**
 * Keeps a frame's measured height from oscillating or running away. Mail
 * whose size depends on the frame's own height (`min-height: 100vh` plus a
 * margin; content that overflows the root by an absolute box) would make
 * "size the frame to the content" a loop:
 *
 * - **Flip-flop** (A → B → A): the frame settles at the larger of the two
 *   and does not shrink below it again.
 * - **Runaway** (growing by the same step again and again): growth stops.
 *
 * {@link reset} starts over — a new document, or a new frame width.
 */
export class FrameHeightGuard {
  private current: number | null = null;
  private shrunkFrom: number | null = null;
  private floor = 0;
  private ceiling = Number.POSITIVE_INFINITY;
  private lastStep = 0;
  private sameSteps = 0;

  /** Starts over (keeps nothing of the previous document or width). */
  reset(): void {
    this.shrunkFrom = null;
    this.floor = 0;
    this.ceiling = Number.POSITIVE_INFINITY;
    this.lastStep = 0;
    this.sameSteps = 0;
  }

  /** The height to apply for a measured `height`, or `null` to keep the current one. */
  accept(height: number): number | null {
    let next = clampFrameHeight(height);
    next = Math.min(Math.max(next, this.floor), this.ceiling);
    const current = this.current;
    if (current === next) return null;
    if (current !== null && next < current) {
      this.shrunkFrom = current;
      this.lastStep = 0;
      this.sameSteps = 0;
    } else if (current !== null) {
      if (this.shrunkFrom === next) {
        // Back where it was before shrinking: a flip-flop; stay here.
        this.floor = next;
      }
      const step = next - current;
      this.sameSteps = step === this.lastStep ? this.sameSteps + 1 : 1;
      this.lastStep = step;
      if (this.sameSteps >= RUNAWAY_STEPS) {
        this.ceiling = current;
        return null;
      }
    }
    this.current = next;
    return next;
  }
}
