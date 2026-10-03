/**
 * The window registrations of the features (`appRegistry.windows`, M5c ADR
 * 0116) for the window layer: `resolveWindowSpec` merges them between the
 * defaults and the layout's `window` properties. The app provides the
 * merged registry; without a provider every window starts from
 * `DEFAULT_WINDOW_SPEC` (non-modal), so a confirmation is modal only where
 * its feature's `{ confirmation: true }` reaches the layer.
 */
import { createContext, useContext } from "react";
import type { UiState } from "../state/uiStore";
import type { WindowRegistry } from "../state/windows";

export const WindowSpecsContext = createContext<WindowRegistry>({});

/** The window registrations in effect. */
export function useWindowSpecs(): WindowRegistry {
  return useContext(WindowSpecsContext);
}

/**
 * The title of a window from the UI state (e.g. a composer's subject),
 * `null` = the kind's own `window-<kind>` text.
 */
export type WindowTitleFn = (state: UiState, id: string) => string | null;

/** Window titles by kind (`Feature.windowTitles`, merged as `appRegistry.windowTitles`). */
export type WindowTitleRegistry = Readonly<Record<string, WindowTitleFn>>;

/** The features' window titles for the window layer and pop-out pages. */
export const WindowTitlesContext = createContext<WindowTitleRegistry>({});

/** The window titles in effect. */
export function useWindowTitles(): WindowTitleRegistry {
  return useContext(WindowTitlesContext);
}
