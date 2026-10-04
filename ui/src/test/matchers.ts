/**
 * Vitest's asymmetric matchers, typed as the values they stand for.
 * `expect.stringMatching(…)` returns `any`; nested in an expected object it
 * would make the whole object `any`. These keep the expected objects typed.
 */

/** A string matching `pattern` (`expect.stringMatching`). */
export function stringMatching(pattern: RegExp): string {
  return expect.stringMatching(pattern) as string;
}

/** A string containing `part` (`expect.stringContaining`). */
export function stringContaining(part: string): string {
  return expect.stringContaining(part) as string;
}

/** An object with at least the properties of `subset` (`expect.objectContaining`). */
export function objectContaining<T extends object>(subset: T): T {
  return expect.objectContaining<unknown>(subset) as T;
}

/** An array containing every item of `items` (`expect.arrayContaining`). */
export function arrayContaining<T>(items: readonly T[]): T[] {
  const expected: unknown[] = [...items];
  return expect.arrayContaining(expected) as T[];
}

/** An array containing none of `items` (`expect.not.arrayContaining`). */
export function notArrayContaining<T>(items: readonly T[]): T[] {
  const unexpected: unknown[] = [...items];
  return expect.not.arrayContaining(unexpected) as T[];
}
