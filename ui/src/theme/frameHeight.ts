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

/** `nail_api::remote_content::FRAME_HEIGHT_MESSAGE`. */
export const FRAME_HEIGHT_MESSAGE = "nail-frame-height";
/** `nail_api::remote_content::RENDER_PARENT_PARAM`. */
export const RENDER_PARENT_PARAM = "parent";
/** `nail_api::remote_content::MAX_FRAME_HEIGHT` (CSS px). */
export const MAX_FRAME_HEIGHT = 200_000;

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
 * The height a sandbox-origin frame posted, or `null` unless the message
 * comes from `frame`'s own window on `sandboxOrigin` and has the contract's
 * shape.
 */
export function frameHeightOf(
  event: Pick<MessageEvent, "data" | "origin" | "source">,
  frame: Window | null,
  sandboxOrigin: string,
): number | null {
  if (frame === null || event.source !== frame || event.origin !== sandboxOrigin) return null;
  const data: unknown = event.data;
  if (typeof data !== "object" || data === null) return null;
  const { type, height } = data as { type?: unknown; height?: unknown };
  if (type !== FRAME_HEIGHT_MESSAGE || typeof height !== "number") return null;
  return clampFrameHeight(height);
}
