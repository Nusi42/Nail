// Pins the few Rust <-> TypeScript facts that cannot be generated from
// nail-api because their Rust side lives in a crate the bindings do not see
// (nail-config's KDL readers, nail-compose): each pair must list the same
// values in the same order, so either side changing alone fails (wave Q2,
// SSOT / ODR / DRY, ADR 0171). scripts/duplicates/allow.json names this file
// as the pinning test of each pair (`pinnedBy`, keyed like the duplicate
// check reports them: the constant's name, or the sorted list items).
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import { lex } from "../lib/duplicates/sources.mjs";
import { constDefs, statementAt } from "../lib/duplicates/ssot.mjs";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");

/**
 * Where one side of a pin is: a named constant, (`after`) the first
 * bracketed list after an anchor text (an inline Rust list), or
 * (`widgetKinds`) the kinds of a nail-config widgets module (the first
 * argument of each `widget(` call).
 *
 * @typedef {{ file: string, name?: string, after?: string, widgetKinds?: true }} Side
 * @typedef {{ key: string, rust: Side, ts: Side }} Pin
 */

/** @type {Pin[]} */
export const PINS = [
  {
    key: "SIGNATURE_SEPARATOR",
    rust: { file: "crates/nail-compose/src/signature.rs", name: "SIGNATURE_SEPARATOR" },
    ts: { file: "ui/src/state/compose.ts", name: "SIGNATURE_SEPARATOR" },
  },
  {
    key: "BAR_WIDGETS",
    rust: { file: "crates/nail-config/src/layout_edit.rs", name: "BAR_WIDGETS" },
    ts: { file: "ui/src/layout/engine.ts", name: "BAR_WIDGETS" },
  },
  {
    key: "account-switcher, status-bar, text, toolbar",
    rust: { file: "crates/nail-config/src/layout_edit.rs", name: "BAR_WIDGETS" },
    ts: { file: "ui/src/layout/engine.ts", name: "BAR_WIDGETS" },
  },
  {
    key: "TOKEN_MODES",
    rust: { file: "crates/nail-config/src/theme_edit.rs", name: "TOKEN_MODES" },
    ts: { file: "ui/src/features/appearance/tokens.ts", name: "TOKEN_MODES" },
  },
  {
    key: "compact, dark, high-contrast, print",
    rust: { file: "crates/nail-config/src/theme_edit.rs", name: "TOKEN_MODES" },
    ts: { file: "ui/src/features/appearance/tokens.ts", name: "TOKEN_MODES" },
  },
  {
    key: "compact, dark, high-contrast, light, print",
    rust: { file: "crates/nail-config/src/css.rs", name: "MODES" },
    ts: { file: "ui/src/features/appearance/themes.ts", name: "VARIANTS" },
  },
  {
    key: "font-size, radius, size, space",
    rust: { file: "crates/nail-config/src/css.rs", name: "PX_TOKEN_CATEGORIES" },
    ts: { file: "ui/src/features/appearance/tokens.ts", name: "PX_CATEGORIES" },
  },
  {
    key: "CARD_WIDGETS",
    rust: { file: "crates/nail-config/src/widgets/core.rs", name: "CARD_WIDGETS" },
    ts: { file: "ui/src/widgets/ThreadViewModel.ts", name: "CARD_WIDGETS" },
  },
  {
    key: "junk-verdict, message-view, remote-content, security-message, security-warnings, sync-message-body",
    rust: { file: "crates/nail-config/src/widgets/core.rs", name: "CARD_WIDGETS" },
    ts: { file: "ui/src/widgets/ThreadViewModel.ts", name: "CARD_WIDGETS" },
  },
  {
    key: "agenda, day, free-time, month, multi-day, week, work-week, year",
    rust: { file: "crates/nail-config/src/widgets/pim.rs", name: "VIEWS" },
    ts: { file: "ui/src/features/pim/time.ts", name: "VIEW_KINDS" },
  },
  {
    key: "due, manual, priority, title",
    rust: { file: "crates/nail-config/src/widgets/pim.rs", after: '("sort", OneOf(' },
    ts: { file: "ui/src/features/pim/TaskList.tsx", name: "TASK_SORTS" },
  },
  {
    // Each nail-config widgets module lists its own feature's widgets
    // (`WIDGETS`, one fact per feature); the Exchange UI names its kinds.
    key: "WIDGETS",
    rust: { file: "crates/nail-config/src/widgets/exchange.rs", widgetKinds: true },
    ts: { file: "ui/src/features/exchange/names.ts", name: "WIDGETS" },
  },
];

/**
 * The string literals of some code, in order.
 *
 * @param {string} code
 * @param {boolean} rust
 * @returns {string[]}
 */
export function strings(code, rust) {
  return lex(code, { rust })
    .filter((seg) => seg.kind === "string")
    .map((seg) => seg.text);
}

/**
 * The text of the constant `name` after its `=` (up to its `;`).
 *
 * @param {string} file
 * @param {string} text
 * @param {string | undefined} name
 */
function definition(file, text, name) {
  const def = constDefs({ path: file, text }).find((d) => d.name === name);
  assert.ok(def, `${file}: no constant ${String(name)}`);
  const start = text
    .split("\n")
    .slice(0, def.line - 1)
    .join("\n").length;
  return statementAt(text, text.indexOf("=", start) + 1);
}

/**
 * The values of one side, in order (the order is part of the fact: a menu's
 * or a feed's order). A TypeScript list may name string constants of its
 * own file (`[FOO_WIDGET, …]`).
 *
 * @param {Side} side
 * @returns {string[]}
 */
export function sideValues(side) {
  const text = fs.readFileSync(path.join(root, side.file), "utf8");
  const rust = side.file.endsWith(".rs");
  if (side.widgetKinds === true) {
    return [...text.matchAll(/\bwidget\(\s*"([^"]+)"/g)].map((m) => m[1] ?? "");
  }
  if (side.after !== undefined) {
    const at = text.indexOf(side.after);
    assert.ok(at >= 0, `${side.file}: "${side.after}" not found`);
    const open = text.indexOf("[", at);
    const close = text.indexOf("]", open);
    return strings(text.slice(open, close + 1), rust);
  }
  const body = definition(side.file, text, side.name);
  if (rust) return strings(body, rust);
  return [...body.matchAll(/"([^"\\]*)"|\b([A-Z][A-Z0-9_]*)\b/g)].map((m) =>
    m[2] === undefined
      ? (m[1] ?? "")
      : (strings(definition(side.file, text, m[2]), false)[0] ?? ""),
  );
}

for (const pin of PINS) {
  test(`pinned: ${pin.key}`, () => {
    const rust = sideValues(pin.rust);
    assert.ok(rust.length > 0, `${pin.rust.file}: no values found`);
    assert.deepEqual(
      sideValues(pin.ts),
      rust,
      `${pin.ts.file} and ${pin.rust.file} must list the same values`,
    );
  });
}

test("values are read in order, constants of the same file resolved", () => {
  assert.deepEqual(strings('const X = ["b", "a"]; // "c"', false), ["b", "a"]);
  assert.deepEqual(strings('const X: &[&str] = &["b", "a"];', true), ["b", "a"]);
  assert.deepEqual(
    sideValues({ file: "ui/src/features/exchange/names.ts", name: "WIDGETS" })[0],
    "focused-tabs",
  );
});
