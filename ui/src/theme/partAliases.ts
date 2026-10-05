/**
 * Renamed parts whose old names a theme may still use (P2, ADR 0148): a
 * part rule written for an old name also styles the new part, for one
 * release after the rename. Keys and values are part-name prefixes: the
 * entry `"thread-view.item": "thread-view.card"` maps `thread-view.item`
 * and every `thread-view.item.*` part (and ancestors in a rule's chain).
 *
 * WP203 (the reader's card feed) fills in the `thread-view.item` →
 * `thread-view.card` entry when the cards replace the strip; the next wave
 * removes it again.
 */
import type { PartRule } from "../api/generated";

export const PART_ALIASES: Readonly<Record<string, string>> = {
  // P2 WP203: the reader's strip became the card feed.
  "thread-view.item": "thread-view.card",
};

/** The part name under its new name, or `null` when no alias covers it. */
export function aliasedPart(
  part: string,
  aliases: Readonly<Record<string, string>> = PART_ALIASES,
): string | null {
  for (const [old, current] of Object.entries(aliases)) {
    if (part === old) return current;
    if (part.startsWith(`${old}.`)) return `${current}${part.slice(old.length)}`;
  }
  return null;
}

/**
 * A copy of `rule` with every old part name (its own and its ancestors')
 * replaced by the new one, or `null` when the rule names no renamed part.
 */
export function aliasedRule<R extends Pick<PartRule, "selector" | "ancestors">>(
  rule: R,
  aliases: Readonly<Record<string, string>> = PART_ALIASES,
): R | null {
  const selector = aliasedPart(rule.selector, aliases);
  const ancestors = rule.ancestors.map((a) => {
    const renamed = aliasedPart(a.selector, aliases);
    return renamed === null ? a : { ...a, selector: renamed };
  });
  if (selector === null && ancestors.every((a, i) => a === rule.ancestors[i])) return null;
  return { ...rule, selector: selector ?? rule.selector, ancestors };
}
