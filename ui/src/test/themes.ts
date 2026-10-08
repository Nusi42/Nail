/** The built-in themes and densities the accessibility sweeps cover. */
import { BUILTIN_THEMES, DENSITIES } from "../features/appearance/model";

export { DENSITIES };

/** The built-in themes by id. */
export const THEME_IDS = BUILTIN_THEMES.map(([id]) => id);

/** Every built-in theme in every density. */
export const THEME_CASES = BUILTIN_THEMES.flatMap(([id]) => DENSITIES.map((d) => [id, d] as const));

/** A sweep covers exactly the five built-in themes (a new one must be added to it). */
export function itCoversTheBuiltInThemes(): void {
  it("covers the five built-in themes", () => {
    expect(THEME_IDS).toEqual(["frost", "sage", "graphite", "nocturne", "high-contrast"]);
  });
}
