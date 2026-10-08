// The interop crate (scripts/interop) is its own Cargo workspace, so it
// cannot inherit `[workspace.lints]`: it repeats them in `[lints]`. This test
// pins the copy to the root (wave Q2, SSOT / ODR / DRY, ADR 0171): every root
// lint has the same level there, the only deviation being stricter.
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");

/**
 * Lints that may be stricter in the interop crate: name → its level there.
 *
 * @type {Map<string, string>}
 */
const STRICTER = new Map([["unsafe_code", "forbid"]]);

/**
 * `key = value` lines of a TOML table (comments and blank lines skipped).
 *
 * @param {string} text
 * @param {string} table e.g. `workspace.lints.rust`
 * @returns {Map<string, string>}
 */
export function tableEntries(text, table) {
  /** @type {Map<string, string>} */
  const out = new Map();
  let inside = false;
  for (const raw of text.split(/\r?\n/)) {
    const line = raw.trim();
    if (line.startsWith("[")) {
      inside = line === `[${table}]`;
      continue;
    }
    if (!inside || line === "" || line.startsWith("#")) continue;
    const at = line.indexOf("=");
    if (at > 0) out.set(line.slice(0, at).trim(), line.slice(at + 1).trim());
  }
  return out;
}

/** @param {string} file */
const read = (file) => fs.readFileSync(path.join(root, file), "utf8");

for (const group of ["rust", "clippy"]) {
  test(`interop [lints.${group}] repeats [workspace.lints.${group}]`, () => {
    const workspace = tableEntries(read("Cargo.toml"), `workspace.lints.${group}`);
    const interop = tableEntries(read("scripts/interop/Cargo.toml"), `lints.${group}`);
    assert.ok(workspace.size > 3);
    const expected = new Map(
      [...workspace].map(([name, level]) => [
        name,
        STRICTER.has(name) ? `"${STRICTER.get(name) ?? ""}"` : level,
      ]),
    );
    assert.deepEqual(interop, expected);
  });
}

test("tableEntries reads one table", () => {
  const text = '[a]\nx = "1"\n# c\n[b]\ny = 2\n';
  assert.deepEqual([...tableEntries(text, "a")], [["x", '"1"']]);
});
