/**
 * Returns `value`, or fails the test when it is `null` or `undefined`.
 *
 * Tests use it instead of the non-null assertion operator, so a missing
 * element or record fails with a message instead of a later `TypeError`.
 */
export function must<T>(value: T | null | undefined, what = "value"): T {
  if (value === null || value === undefined)
    throw new Error(`expected a ${what}, got ${String(value)}`);
  return value;
}
