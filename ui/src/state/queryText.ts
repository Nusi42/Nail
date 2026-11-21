/**
 * The search query language's value quoting, the one rule every place that
 * writes a query value follows (the search field's values, label views):
 * quoted with `\` escapes when the value holds spaces, quotes, brackets or
 * backslashes, as the core's parser reads it.
 */

/** A value as the query language reads it. */
export function searchValueText(value: string): string {
  return /[\s()"\\]/.test(value) ? `"${value.replace(/["\\]/g, "\\$&")}"` : value;
}
