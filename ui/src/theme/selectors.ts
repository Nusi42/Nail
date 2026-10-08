/**
 * Selector helpers of the CSS-in-TS skins (the shared skin, the feature
 * skins and base CSS): one definition of how a part, a list of parts and a
 * mode are selected.
 */

/** `[data-part="…"]` for one part. */
export const p = (part: string) => `[data-part="${part}"]`;

/** Joined selectors of several parts. */
export const ps = (...parts: string[]) => parts.map(p).join(", ");

/** Joined selectors of several parts, each with the same suffix (`:hover`). */
export const pss = (suffix: string, ...parts: string[]) =>
  parts.map((x) => p(x) + suffix).join(", ");

/** The root selector of the High contrast mode. */
export const HC = ':root[data-nail-mode="high-contrast"]';

/** Selectors of parts under High contrast. */
export const hc = (...parts: string[]) => parts.map((x) => `${HC} ${p(x)}`).join(", ");
