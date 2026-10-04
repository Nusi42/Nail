/**
 * Pop-out windows (M5c WP173, ADR 0116): the rules without React — which
 * page this is, what the main page does when a window pops out or comes
 * back, the context a pop-out reads, and the preparers windows register to
 * get ready before they pop out (the composer saves its draft and hands
 * over `{draft}`).
 */
import type { QueryClient } from "@tanstack/react-query";
import { NailError, type NailClient } from "../../api/client";
import type { JsonValue, PopOut, PopOutChange, Response, WindowSpec } from "../../api/generated";
import type { UiStore } from "../../state/uiStore";
import { applyPopOuts } from "../../state/windowState";
import { parsePopOutRoute, windowKind } from "../../state/windows";

/** Action ids. */
export const POP_OUT_ACTION = "popout.open";
export const RETURN_ACTION = "popout.return";

/**
 * The window id this page shows when it is a pop-out (`#/window/<id>`),
 * `null` for the main page.
 */
export function popOutPageWindow(hash?: string): string | null {
  const current = hash ?? (typeof window === "undefined" ? "" : window.location.hash);
  return parsePopOutRoute(current);
}

/** Whether this page is a pop-out window. */
export function isPopOutPage(hash?: string): boolean {
  return popOutPageWindow(hash) !== null;
}

/**
 * What the main page does with its in-page window when a pop-out changes:
 * it left the page (`opened` → close it there), it came back (`returned` →
 * open it there); moves, focus and closes leave the page alone.
 */
export function pageEffectOf(change: PopOutChange): "close" | "open" | null {
  switch (change) {
    case "opened":
      return "close";
    case "returned":
      return "open";
    default:
      return null;
  }
}

/** The context a pop-out was opened with, `null` when none or not open. */
export function popOutContextOf(popOuts: readonly PopOut[], window: string): JsonValue | null {
  return popOuts.find((p) => p.window === window)?.context ?? null;
}

/** Whether `window` is popped out. */
export function isPoppedOut(popOuts: readonly PopOut[], window: string): boolean {
  return popOuts.some((p) => p.window === window);
}

/** What a preparer may use. */
export interface PopOutPrepareContext {
  client: NailClient;
  store: UiStore;
  queryClient: QueryClient;
}

/**
 * Runs before a window of a kind pops out: saves what only this page holds
 * (e.g. the composer's draft — every webview has its own UI store) and
 * returns the context the pop-out reads (`PopOut.context`, at most 16 KiB),
 * `null` / `undefined` = none. Throwing keeps the window in the page.
 */
export type PopOutPreparer = (
  window: string,
  ctx: PopOutPrepareContext,
) => Promise<JsonValue | null | undefined> | JsonValue | null | undefined;

const preparers = new Map<string, PopOutPreparer>();

/**
 * Registers the preparer of a window kind (one per kind; a later one
 * replaces it). Returns the unregistration.
 */
export function registerPopOutPreparer(kind: string, preparer: PopOutPreparer): () => void {
  preparers.set(kind, preparer);
  return () => {
    if (preparers.get(kind) === preparer) preparers.delete(kind);
  };
}

/** The preparer of a window (by id or kind). */
export function preparerOf(window: string): PopOutPreparer | undefined {
  return preparers.get(windowKind(window));
}

/** The answer's pop-out list, or the error it carries. */
function popOutsOf(response: Response): readonly PopOut[] {
  if (response.type === "PopOuts") return response.pop_outs;
  if (response.type === "Error") {
    throw new NailError(response.code, response.message, response.localized ?? null);
  }
  throw new NailError("internal", `unexpected ${response.type}`);
}

/**
 * Pops `window` out (main page): runs the kind's preparer, asks the core
 * (`PopOutWindow` with the window's title, the context and the spec's
 * first and smallest size) and takes the window off the page. The window
 * stays in the page when anything fails.
 */
export async function popOutWindow(
  ctx: PopOutPrepareContext,
  window: string,
  title: string,
  spec?: Pick<WindowSpec, "size" | "min_size">,
): Promise<readonly PopOut[]> {
  const prepare = preparerOf(window);
  const context = prepare ? await prepare(window, ctx) : undefined;
  const popOuts = popOutsOf(
    await ctx.client.call({
      type: "PopOutWindow",
      window,
      title,
      ...(context === undefined || context === null ? {} : { context }),
      ...(spec ? { size: spec.size, min_size: spec.min_size } : {}),
    }),
  );
  applyPopOuts(ctx.queryClient, popOuts);
  ctx.store.getState().closeWindow(window);
  return popOuts;
}

/** Puts a popped-out window back into the main page (`ReturnPopOut`). */
export async function returnPopOut(
  ctx: Pick<PopOutPrepareContext, "client" | "queryClient">,
  window: string,
): Promise<readonly PopOut[]> {
  const popOuts = popOutsOf(await ctx.client.call({ type: "ReturnPopOut", window }));
  applyPopOuts(ctx.queryClient, popOuts);
  return popOuts;
}

/** Raises a popped-out window (`FocusPopOut`). */
export async function focusPopOut(
  ctx: Pick<PopOutPrepareContext, "client" | "queryClient">,
  window: string,
): Promise<readonly PopOut[]> {
  const popOuts = popOutsOf(await ctx.client.call({ type: "FocusPopOut", window }));
  applyPopOuts(ctx.queryClient, popOuts);
  return popOuts;
}

/**
 * A window id no pop-out ever has: `FocusPopOut` of it tells whether this
 * core has pop-outs (`conflict` = the desktop app; `unsupported` without a
 * desktop host, `forbidden` for remote sessions) without side effects.
 */
export const PROBE_WINDOW = "popout-probe";

/** Whether the answer to the probe says pop-outs are available. */
export function probeSaysAvailable(response: Response): boolean {
  return response.type === "Error" && response.code === "conflict";
}

/** The Fluent id of a failed pop-out request's message. */
export function errorMessageId(error: unknown): string {
  if (typeof error === "object" && error !== null) {
    const localized = (error as { localized?: { id?: unknown } | null }).localized;
    if (localized && typeof localized.id === "string") return localized.id;
  }
  return "popout-failed";
}

/** The Fluent id of a window's title (`window-<kind>`, like the window layer). */
export function windowTitleId(window: string): string {
  return `window-${windowKind(window)}`;
}
