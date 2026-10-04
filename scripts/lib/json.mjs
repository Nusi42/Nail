/**
 * Typed JSON parsing for the Node scripts. `JSON.parse` returns `any`, which
 * the type-aware lint rules (`no-unsafe-*`) reject wherever it flows; these
 * helpers return `unknown`, which the caller narrows or asserts to the shape
 * it documents with a JSDoc type cast.
 */
import { readFileSync } from "node:fs";

/**
 * @param {string} text
 * @returns {unknown}
 */
export function parseJson(text) {
  return JSON.parse(text);
}

/**
 * Reads and parses a UTF-8 JSON file.
 *
 * @param {import("node:fs").PathLike} file
 * @returns {unknown}
 */
export function readJson(file) {
  return JSON.parse(readFileSync(file, "utf8"));
}
