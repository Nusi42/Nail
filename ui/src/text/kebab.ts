/**
 * Kebab-case names (`follow-up`, `mail-list2`): lowercase words of letters
 * and digits joined by single hyphens, starting with a letter. Plug-in names
 * and widget kinds use them.
 */
export const KEBAB = /^[a-z][a-z0-9]*(?:-[a-z0-9]+)*$/;
