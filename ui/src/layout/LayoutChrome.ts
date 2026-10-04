/**
 * The layout chrome (M5d, ADR 0131): a hook the layout renderer calls for
 * every node it renders, so the layout editor (WP181) can draw pane headers,
 * drag handles, dock / swap targets and the hide / show eye around the
 * window's nodes without the renderer knowing about editing.
 *
 * Outside customize mode there is no provider: hidden nodes
 * (`LayoutNode.hidden`, `hidden=#true` in `layout.kdl`) are skipped and
 * every node renders as it is. The layout editor provides
 * {@link LayoutChromeContext} with `showHidden: true` (hidden nodes render,
 * so they can be shown again) and a `renderNode` that wraps a node's
 * content.
 *
 * WP181 (customize-layout mode): the editor registers its chrome for the
 * one window it edits ({@link setWindowChrome}); `WindowView` provides a
 * registered chrome around that window and lets it frame the whole window
 * (`renderWindow`: the editor's banner and palette). A chrome may name its
 * `window`: windows nested inside it (the view host's window inside the
 * main window) then render plain.
 */
import { createContext, useContext, useSyncExternalStore, type ReactNode } from "react";
import type { LayoutNode, LayoutNodeRef } from "../api/generated";

/** What the renderer asks the chrome. */
export interface LayoutChrome {
  /** Render hidden nodes too (customize mode). */
  showHidden: boolean;
  /**
   * Wraps the rendered `content` of `node` (at `ref`); the identity outside
   * customize mode.
   */
  renderNode: (node: LayoutNode, ref: LayoutNodeRef, content: ReactNode) => ReactNode;
  /**
   * The window this chrome is for; nested windows of other names render
   * with {@link PLAIN_CHROME}. Unset: every window below the provider.
   */
  window?: string;
  /**
   * Wraps a whole window (its shown breakpoint `breakpoint`); the identity
   * when unset.
   */
  renderWindow?: (window: string, breakpoint: number, content: ReactNode) => ReactNode;
}

/** No chrome: hidden nodes are skipped, nodes render as they are. */
export const PLAIN_CHROME: LayoutChrome = {
  showHidden: false,
  renderNode: (_node, _ref, content) => content,
};

export const LayoutChromeContext = createContext<LayoutChrome>(PLAIN_CHROME);

/** The chrome of the nearest provider ({@link PLAIN_CHROME} without one). */
export function useLayoutChrome(): LayoutChrome {
  return useContext(LayoutChromeContext);
}

/**
 * The node reference of a renderer path: `"<window>/<breakpoint>/<i>/<j>…"`
 * (the paths `WindowView` gives its nodes). `null` for a path that is not
 * one (e.g. a settings section's nodes).
 */
export function nodeRefOfPath(path: string): LayoutNodeRef | null {
  const [window, breakpoint, ...rest] = path.split("/");
  if (window === undefined || window === "" || breakpoint === undefined) return null;
  const numbers = [breakpoint, ...rest].map((part) => (/^\d+$/.test(part) ? Number(part) : NaN));
  if (numbers.some((n) => Number.isNaN(n))) return null;
  const [bp = 0, ...indexes] = numbers;
  return { window, breakpoint: bp, path: indexes };
}

/** Whether a node renders: shown, or hidden while the chrome shows hidden nodes. */
export function rendersNode(node: LayoutNode, chrome: LayoutChrome): boolean {
  return node.hidden !== true || chrome.showHidden;
}

/**
 * The chrome a window renders with: the one registered for it, else the
 * inherited one unless that names another window.
 */
export function chromeForWindow(
  window: string,
  registered: LayoutChrome | null,
  inherited: LayoutChrome,
): LayoutChrome {
  if (registered) return registered;
  return inherited.window === undefined || inherited.window === window ? inherited : PLAIN_CHROME;
}

// --- Chromes registered per window (the layout editor) ----------------------

const registeredChromes = new Map<string, LayoutChrome>();
const chromeListeners = new Set<() => void>();

/** Registers (or with `null` removes) the chrome of window `window`. */
export function setWindowChrome(window: string, chrome: LayoutChrome | null): void {
  if (chrome === null) {
    if (!registeredChromes.delete(window)) return;
  } else {
    if (registeredChromes.get(window) === chrome) return;
    registeredChromes.set(window, chrome);
  }
  for (const listener of [...chromeListeners]) listener();
}

/** The chrome registered for window `window`, `null` without one. */
export function registeredWindowChrome(window: string): LayoutChrome | null {
  return registeredChromes.get(window) ?? null;
}

function subscribeChromes(listener: () => void): () => void {
  chromeListeners.add(listener);
  return () => {
    chromeListeners.delete(listener);
  };
}

/** {@link registeredWindowChrome}, re-rendering when it changes. */
export function useWindowChrome(window: string): LayoutChrome | null {
  return useSyncExternalStore(
    subscribeChromes,
    () => registeredWindowChrome(window),
    () => null,
  );
}
