/**
 * Empty text treated like missing text: a contact without a display name
 * (`""` or `null`) falls back to its address. Use these with `??` instead
 * of `||`, which would also skip other falsy values such as `0`.
 */

/** `text`, or `undefined` when it is empty, `null` or `undefined`. */
export function nonEmpty(text: string | null | undefined): string | undefined {
  return text === null || text === undefined || text === "" ? undefined : text;
}

/** Whether `text` has at least one character. */
export function hasText(text: string | null | undefined): text is string {
  return nonEmpty(text) !== undefined;
}
