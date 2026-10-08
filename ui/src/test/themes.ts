/**
 * The theme matrix of the accessibility tests (wave Q2, DRY): every
 * built-in theme in every density, from the generated contract, resolved
 * as the mock core resolves it. `themes.test.ts` pins the built-in list.
 */
import { BUILTIN_THEMES, DENSITY_VALUES, type Density } from "../api/generated";
import { defaultTheme } from "../api/mockData";
import { resolveMockTheme } from "../features/appearance/themes";

/** The ids of the built-in themes. */
export const BUILTIN_THEME_IDS = BUILTIN_THEMES.map(([id]) => id);

/** Every built-in theme in every density: [theme id, density]. */
export const THEME_DENSITY_CASES = BUILTIN_THEMES.flatMap(([id]) =>
  DENSITY_VALUES.map((density) => [id, density] as const),
);

/** The built-in theme `id` in `density`, without overrides. */
export const builtinTheme = (id: string, density: Density = "comfortable") =>
  resolveMockTheme(defaultTheme(), id, { density, overrides: {} });
