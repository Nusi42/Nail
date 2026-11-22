/**
 * Equality of plain data (settings values, action arguments, editor
 * states): equal when their JSON is equal. The one definition every dirty
 * check and comparison of such values uses.
 */
export function sameJson(a: unknown, b: unknown): boolean {
  return JSON.stringify(a) === JSON.stringify(b);
}
