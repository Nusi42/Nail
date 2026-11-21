/**
 * The multi-value mode of the shared pickers (R3, ADR 0228): the pure
 * rules for the chosen values, kept apart from the `Picker` so they are
 * tested on their own. Values are compared case-insensitively (an address
 * typed in capitals is the same address), and the first spelling stays.
 */

/** The characters that end a typed value in multi-value mode (besides Enter). */
export const VALUE_SEPARATORS = [",", ";"] as const;

function same(a: string, b: string): boolean {
  return a.toLowerCase() === b.toLowerCase();
}

/** Whether `values` holds `value`. */
export function hasValue(values: readonly string[], value: string): boolean {
  return values.some((v) => same(v, value));
}

/** `values` with each of `added` (trimmed, non-empty) appended unless it is already there. */
export function withValues(values: readonly string[], added: readonly string[]): string[] {
  const out = [...values];
  for (const raw of added) {
    const value = raw.trim();
    if (value !== "" && !hasValue(out, value)) out.push(value);
  }
  return out;
}

/** `values` without `value`. */
export function withoutValue(values: readonly string[], value: string): string[] {
  return values.filter((v) => !same(v, value));
}

/** `values` with `value` added, or removed when it is already there (picking a chosen option). */
export function toggledValue(values: readonly string[], value: string): string[] {
  return hasValue(values, value) ? withoutValue(values, value) : withValues(values, [value]);
}

/**
 * Typed (or pasted) text split at the separators: the finished values
 * before the last separator and the rest still being typed.
 */
export function splitTyped(text: string): { done: string[]; rest: string } {
  const parts = text.split(new RegExp(`[${VALUE_SEPARATORS.join("")}]`));
  const rest = parts.pop() ?? "";
  return { done: parts.map((p) => p.trim()).filter((p) => p !== ""), rest };
}
