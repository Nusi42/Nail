/**
 * Names of the toolbar customizer (M5d WP182): its action, its window kind
 * and the window id per toolbar (`toolbar-customizer:<toolbar>`). Kept apart
 * from the components so the toolbar widget can use them without loading
 * the customizer.
 */
import { isWindowInstance, parseWindowId, windowId } from "../../state/windows";

/** Customizes the toolbar the context menu was opened on (`defaults/layout.kdl`). */
export const TOOLBAR_CUSTOMIZE_ACTION = "toolbar.customize";

/** The customizer's window kind (`window-toolbar-customizer` is its title). */
export const TOOLBAR_CUSTOMIZER_KIND = "toolbar-customizer";

/** The customizer's root part (the window's content). */
export const CUSTOMIZER_PART = "toolbar-customizer";

/** The settings card's root part. */
export const SETTINGS_PART = "toolbar-customizer-settings";

/**
 * The customizer's window for `toolbar`: one window per toolbar; a name
 * that is no valid instance key (or none) opens the window without one,
 * which lets the user pick the toolbar.
 */
export function customizerWindow(toolbar: string | null): string {
  return toolbar !== null && isWindowInstance(toolbar)
    ? windowId(TOOLBAR_CUSTOMIZER_KIND, toolbar)
    : TOOLBAR_CUSTOMIZER_KIND;
}

/** The toolbar a customizer window was opened for, `null` = none named. */
export function toolbarOfWindow(id: string): string | null {
  const parts = parseWindowId(id);
  return parts?.kind === TOOLBAR_CUSTOMIZER_KIND ? parts.instance : null;
}
