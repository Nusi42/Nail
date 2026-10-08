// @vitest-environment node
/**
 * One English text per meaning (wave Q2, ADR 0171): generic labels live
 * once in `locales/en/common.ftl` (`common-*`); an id that a contract, the
 * layout or a template must keep (`palette-action-…`, `rules-role-…`, a
 * core id) is a reference `id = { other-id }` to the one definition.
 */
import { readFileSync, readdirSync } from "node:fs";
import { join, resolve } from "node:path";
import { must } from "../test/must";
import { SRC_DIR, sourceFiles } from "../test/sourceFiles";
import { FluentBundle, FluentResource, type FluentVariable } from "@fluent/bundle";
import type { Message, Pattern } from "@fluent/bundle/esm/ast";

const localesDir = resolve(process.cwd(), "..", "locales");

function catalog(locale: string): Map<string, string> {
  const out = new Map<string, string>();
  const dir = join(localesDir, locale);
  for (const file of readdirSync(dir).filter((f) => f.endsWith(".ftl"))) {
    out.set(file, readFileSync(join(dir, file), "utf8"));
  }
  return out;
}

/** The id a message's value only references (`a = { b }`), if it does. */
function referenceOf(value: Pattern | null): string | null {
  if (value === null || typeof value === "string" || value.length !== 1) return null;
  const [element] = value;
  if (typeof element === "string" || element?.type !== "mesg") return null;
  return element.attr === null ? element.name : `${element.name}.${element.attr}`;
}

const files = catalog("en");
const bundle = new FluentBundle("en", { useIsolating: false });
const messages = new Map<string, Message>();
for (const text of files.values()) {
  const resource = new FluentResource(text);
  expect(bundle.addResource(resource)).toEqual([]);
  for (const entry of resource.body) if (!entry.id.startsWith("-")) messages.set(entry.id, entry);
}

const VARIABLES = [
  ...new Set(
    [...files.values()].flatMap((t) =>
      [...t.matchAll(/\$([a-zA-Z_][\w-]*)/g)].map((m) => must(m[1])),
    ),
  ),
];

/** Formats a message (or `message.attribute`) with sample arguments. */
function format(id: string): string {
  const [name = "", attr] = id.split(".");
  const message = bundle.getMessage(name);
  const pattern = attr === undefined ? message?.value : message?.attributes[attr];
  if (pattern == null) throw new Error(`no ${id}`);
  // Every variable any catalog uses, as a number (plural selectors pick
  // `other`, plain placeables print it).
  const args: Record<string, FluentVariable> = Object.fromEntries(
    VARIABLES.map((name) => [name, 2]),
  );
  const errors: Error[] = [];
  const text = bundle.formatPattern(pattern, args, errors);
  expect(errors, id).toEqual([]);
  return text;
}

/**
 * Catalogs Rust tests render on their own (nail-itip's `itip.ftl` harness,
 * nail-pgp's `core-crypto.ftl` checks): their texts stay definitions there,
 * and `common.ftl` refers to them instead of the other way round.
 */
const STANDALONE_CATALOGS = ["itip.ftl", "core-crypto.ftl"];
const standaloneIds = new Set(
  STANDALONE_CATALOGS.flatMap((file) =>
    new FluentResource(must(files.get(file))).body.map((e) => e.id),
  ),
);

describe("shared texts", () => {
  const references = [...messages.values()].flatMap((m) => {
    const out: [string, string][] = [];
    const value = referenceOf(m.value);
    if (value !== null) out.push([m.id, value]);
    for (const [attr, pattern] of Object.entries(m.attributes)) {
      const target = referenceOf(pattern);
      if (target !== null) out.push([`${m.id}.${attr}`, target]);
    }
    return out;
  });

  it("finds the references to the one definition", () => {
    expect(references.length).toBeGreaterThan(300);
  });

  it("points every reference at a definition, never at another reference", () => {
    const targets = new Map(references);
    for (const [id, target] of references) {
      expect(messages.has(must(target.split(".")[0])), `${id} -> ${target}`).toBe(true);
      const next = targets.get(target);
      if (next === undefined) continue;
      // The one exception: a common text whose definition stays in a catalog
      // Rust tests load on their own (see STANDALONE_CATALOGS).
      expect(
        standaloneIds.has(next) && target.startsWith("common-"),
        `${id} -> ${target} -> ${next}`,
      ).toBe(true);
    }
  });

  it("keeps the texts of standalone catalogs in them, and common.ftl refers to them", () => {
    const common = new FluentResource(must(files.get("common.ftl")));
    for (const entry of common.body) {
      const target = referenceOf(entry.value);
      if (target !== null) expect(standaloneIds.has(target), `${entry.id} -> ${target}`).toBe(true);
    }
    for (const file of STANDALONE_CATALOGS) {
      const own = new FluentResource(must(files.get(file)));
      for (const entry of own.body) {
        expect(referenceOf(entry.value), `${file}: ${entry.id}`).toBeNull();
      }
    }
  });

  it("renders a reference exactly like its definition", () => {
    for (const [id, target] of references) expect(format(id), id).toBe(format(target));
  });

  it("defines common texts only in common.ftl, each used somewhere", () => {
    const common = new FluentResource(must(files.get("common.ftl")));
    const ids = common.body.map((e) => e.id);
    expect(ids.length).toBeGreaterThan(100);
    expect(ids.filter((id) => !id.startsWith("common-"))).toEqual([]);
    const others = [...files].filter(([file]) => file !== "common.ftl").map(([, t]) => t);
    expect(others.flatMap((t) => [...t.matchAll(/^(common-[\w-]+)\s*=/gm)])).toEqual([]);
    const code = sourceFiles(SRC_DIR, { include: /\.tsx?$/ })
      .map((f) => readFileSync(f, "utf8"))
      .join("\n");
    const ftl = others.join("\n");
    const unused = ids.filter(
      (id) => !code.includes(`"${id}"`) && !new RegExp(`\\{\\s*${id}\\s*\\}`).test(ftl),
    );
    expect(unused).toEqual([]);
  });

  it("keeps the German common catalog in step (empty until translated)", () => {
    expect(catalog("de").has("common.ftl")).toBe(true);
  });
});
