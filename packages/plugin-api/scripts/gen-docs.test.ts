import { readFileSync } from "node:fs";

import { describe, expect, it } from "vitest";

import { BEGIN, cleanDoc, components, documents, END, rustUiDocs, splice } from "./gen-docs.mjs";

const NODE_TYPES = [
  "text",
  "markdown",
  "input",
  "textarea",
  "select",
  "checkbox",
  "date",
  "contact-picker",
  "folder-picker",
  "message-list",
  "table",
  "button",
  "progress",
  "image",
  "stack",
];

describe("generated user docs", () => {
  it("are up to date (run `npm run docs -w packages/plugin-api` otherwise)", () => {
    for (const [file, body] of documents()) {
      const current = readFileSync(file, "utf8");
      expect(splice(current, body), file).toBe(current);
    }
  });

  it("describe every component of the contract with its value", () => {
    const rust = rustUiDocs();
    expect(Object.keys(rust.descriptions)).toEqual(NODE_TYPES);
    expect(rust.values.date).toMatch(/YYYY-MM-DD/);
    expect(rust.values.table).toMatch(/when `selectable`/);
    const md = components();
    for (const type of NODE_TYPES) expect(md).toContain(`### \`<${type}>\``);
  });

  it("clean rustdoc links and splice between the markers", () => {
    expect(cleanDoc("see [`super::is_plugin_id`]\n and {@link NailApi}")).toBe(
      "see `is_plugin_id` and `NailApi`",
    );
    // List items stay apart; example addresses become code (no bare URLs).
    expect(cleanDoc("Exactly one is set.\n\n* `a` — first;\n- `b` — second.")).toBe(
      "Exactly one is set. • `a` — first; • `b` — second.",
    );
    expect(cleanDoc('e.g. "ada@example.org", not `bob@example.org`')).toBe(
      'e.g. "`ada@example.org`", not `bob@example.org`',
    );
    expect(splice(`a\n${BEGIN}\nold\n${END}\nb`, "new")).toBe(`a\n${BEGIN}\n\nnew\n\n${END}\nb`);
    expect(() => splice("no markers", "x")).toThrow(/markers/);
  });
});
