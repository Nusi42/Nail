/**
 * Text measured the way the core measures it: in Unicode code points
 * (Rust `str::chars`), so a limit checked here (a name of at most 128
 * characters, a PIN of 6–8 digits) agrees with the core's check. Not
 * grapheme clusters: an emoji with a skin tone counts as two, as in Rust.
 */

/** The code points of `text` (Rust `text.chars()`). */
export function chars(text: string): string[] {
  return Array.from(text);
}

/** The number of code points of `text` (Rust `text.chars().count()`). */
export function charCount(text: string): number {
  return chars(text).length;
}
