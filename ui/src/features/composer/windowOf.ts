/**
 * Which composer a component belongs to (M5c, several composers), without
 * the composer's flows: the security toggles and the program-draft notice
 * (other features, imported by the composer) read their own window's
 * composer through these, so no import cycle reaches the composer's flows.
 */
import type { ComposerState, UiState } from "../../state/uiStore";
import { useWindowInstance } from "../../state/windows";
import { COMPOSER_KIND, isComposerInstance } from "./store";

/** The envelope of a composer window (`null` = none). */
export function composerOf(
  state: Pick<UiState, "composer" | "composers">,
  window: string,
): ComposerState | null {
  if (!isComposerInstance(window)) return state.composer;
  return Object.hasOwn(state.composers, window) ? (state.composers[window] ?? null) : null;
}

/**
 * The composer window a component belongs to: its own window when it is
 * rendered in a `composer:<key>` window, else the plain `composer` (whose
 * state is the focused composer, ADR 0118).
 */
export function useComposerWindowOf(): string {
  const instance = useWindowInstance();
  return instance !== null && isComposerInstance(instance.id) ? instance.id : COMPOSER_KIND;
}
