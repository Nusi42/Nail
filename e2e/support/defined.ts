/**
 * `value`, or an error naming `what` when it is missing (`null` or
 * `undefined`): for values a test has already checked or that the page must
 * have, without a non-null assertion that would turn a miss into a confusing
 * `TypeError` later.
 */
export function defined<T>(value: T | null | undefined, what: string): T {
  if (value === null || value === undefined) throw new Error(`${what} is missing`);
  return value;
}
