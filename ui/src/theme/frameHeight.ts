/**
 * What a mail frame reports to the reader (P3, ADR 0164; links ADR 0206;
 * ADR 0212): its content height, the hovered or clicked link and its
 * context menu. Both frames — the local one (`srcdoc`: desktop, `nail
 * serve` on loopback) and the remote UI's sandbox origin — are sandboxed
 * with scripts and an **opaque** origin ({@link MAIL_FRAME_SANDBOX}, never
 * `allow-same-origin`): nothing in the document can reach the app. The
 * document runs exactly one script, `FRAME_SCRIPT` (hash-pinned in its CSP
 * and the app's; `crates/nail-api/src/mail_frame.rs`), which posts these
 * messages to the app origin named by its root's `FRAME_PARENT_ATTRIBUTE`.
 * The reader trusts a message only from its own frame's window
 * ({@link framePosted}).
 *
 * Before ADR 0212 the local frame was same-origin without scripts and the
 * reader read and listened on its document; WebKit (macOS, Linux) never
 * runs such listeners, so links and the text menu did nothing there.
 *
 * Heights are clamped to `[0, MAX_FRAME_HEIGHT]`; a taller document scrolls
 * inside its frame (wide mail keeps its horizontal scrollbar: owner
 * decision 2026-10-05).
 */
import { FRAME_HEIGHT_MESSAGE, FRAME_MENU_MESSAGE, MAX_FRAME_HEIGHT } from "../api/generated";
export { FRAME_HEIGHT_MESSAGE, MAX_FRAME_HEIGHT, RENDER_PARENT_PARAM } from "../api/generated";

/** The `sandbox` attribute of every mail frame. Never add `allow-same-origin`. */
export const MAIL_FRAME_SANDBOX = "allow-scripts";

/** A height clamped to what the reader sizes a frame to. */
export function clampFrameHeight(height: number): number {
  if (!Number.isFinite(height) || height < 0) return 0;
  return Math.min(Math.ceil(height), MAX_FRAME_HEIGHT);
}

/**
 * The origin a sandboxed document without `allow-same-origin` posts from:
 * the opaque origin serializes as `"null"`. Every mail frame is such a
 * document, so its messages arrive with this origin; the source window
 * (the reader's own frame, which no other document can impersonate) is
 * what identifies them.
 */
export const OPAQUE_ORIGIN = "null";

/** A message event as the reader's frames see it. */
export type FrameMessageEvent = Pick<MessageEvent, "data" | "origin" | "source">;

/**
 * The fields of a message a mail frame posted with `type`, or `null`
 * unless it comes from `frame`'s own window — on `sandboxOrigin`, or the
 * opaque origin the sandbox gives that document ({@link OPAQUE_ORIGIN}) —
 * and is an object of that type. Shared by the height, link and menu
 * messages.
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
 * The height a mail frame posted, or `null` unless the message
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

/** A context menu a mail frame asked for (ADR 0212). */
export interface FrameMenu {
  /** The point in the frame's viewport (`0, 0` from the keyboard). */
  x: number;
  y: number;
  /** `MouseEvent.button` of the `contextmenu` event. */
  button: number;
  /** The frame's selected text. */
  text: string;
}

/**
 * The context menu a mail frame posted, or `null` unless the message comes
 * from `frame`'s own window ({@link framePosted}) and has the contract's
 * shape.
 */
export function frameMenuOf(
  event: FrameMessageEvent,
  frame: Window | null,
  sandboxOrigin: string,
): FrameMenu | null {
  const posted = framePosted(event, frame, sandboxOrigin, FRAME_MENU_MESSAGE);
  if (posted === null) return null;
  const { x, y, button, text } = posted;
  if (typeof x !== "number" || typeof y !== "number" || typeof button !== "number") return null;
  if (typeof text !== "string") return null;
  return { x, y, button, text };
}

/**
 * The DOM event a mail frame's `<iframe>` element dispatches (bubbling)
 * when its document asked for a context menu; the menu host opens the text
 * menu for it. `detail` is a {@link FrameMenu}.
 */
export const FRAME_MENU_EVENT = "nail-frame-menu";

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
