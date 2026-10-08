/**
 * Pins the typed runtime API against the host (wave Q2, SSOT / ODR / DRY):
 * `NailApi` in `api.ts` types the global `nail` object that the host's
 * `crates/nail-plugins/src/prelude.js` installs. The two cannot be generated
 * from one another (the prelude is plain JavaScript run in QuickJS, the
 * types are the SDK's documentation), so this test compares their members:
 * every namespace of `nail` and every member of each namespace must exist on
 * both sides.
 */
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import ts from "typescript";
import { describe, expect, it } from "vitest";

const here = dirname(fileURLToPath(import.meta.url));
const preludePath = join(here, "../../../crates/nail-plugins/src/prelude.js");
const apiPath = join(here, "api.ts");

function parse(path: string): ts.SourceFile {
  return ts.createSourceFile(path, readFileSync(path, "utf8"), ts.ScriptTarget.Latest, true);
}

function nameOf(name: ts.PropertyName): string {
  return ts.isIdentifier(name) || ts.isStringLiteral(name) ? name.text : name.getText();
}

/** Every `const <name> = <initializer>` of a file. */
function constants(file: ts.SourceFile): Map<string, ts.Expression> {
  const out = new Map<string, ts.Expression>();
  const visit = (node: ts.Node) => {
    if (ts.isVariableDeclaration(node) && ts.isIdentifier(node.name) && node.initializer) {
      out.set(node.name.text, node.initializer);
    }
    ts.forEachChild(node, visit);
  };
  visit(file);
  return out;
}

/** The object literal an expression stands for (through `freeze(...)` and constants). */
function objectOf(
  expr: ts.Expression | undefined,
  consts: Map<string, ts.Expression>,
): ts.ObjectLiteralExpression | null {
  if (expr === undefined) return null;
  if (ts.isObjectLiteralExpression(expr)) return expr;
  if (ts.isCallExpression(expr)) return objectOf(expr.arguments[0], consts);
  if (ts.isIdentifier(expr)) return objectOf(consts.get(expr.text), consts);
  return null;
}

/** `namespace -> member names` of the prelude's `nail` object (`null` = not an object). */
function preludeMembers(): Map<string, string[] | null> {
  const file = parse(preludePath);
  const consts = constants(file);
  const nail = objectOf(consts.get("nail"), consts);
  if (nail === null) throw new Error("prelude.js: no `const nail = { … }`");
  const out = new Map<string, string[] | null>();
  for (const prop of nail.properties) {
    if (prop.name === undefined) continue;
    const value = ts.isPropertyAssignment(prop) ? prop.initializer : undefined;
    const inner = objectOf(value, consts);
    out.set(
      nameOf(prop.name),
      inner === null
        ? null
        : inner.properties.flatMap((p) => (p.name === undefined ? [] : [nameOf(p.name)])).sort(),
    );
  }
  return out;
}

/** `namespace -> member names` of `NailApi` (`null` = not an interface of api.ts). */
function apiMembers(): Map<string, string[] | null> {
  const file = parse(apiPath);
  const interfaces = new Map<string, ts.InterfaceDeclaration>();
  ts.forEachChild(file, (node) => {
    if (ts.isInterfaceDeclaration(node)) interfaces.set(node.name.text, node);
  });
  const membersOf = (decl: ts.InterfaceDeclaration): string[] => {
    const own = decl.members.flatMap((m) => (m.name === undefined ? [] : [nameOf(m.name)]));
    const inherited = (decl.heritageClauses ?? []).flatMap((clause) =>
      clause.types.flatMap((t) => {
        const base = interfaces.get(t.expression.getText());
        return base === undefined ? [] : membersOf(base);
      }),
    );
    return [...own, ...inherited];
  };
  const api = interfaces.get("NailApi");
  if (api === undefined) throw new Error("api.ts: no `interface NailApi`");
  const out = new Map<string, string[] | null>();
  for (const member of api.members) {
    if (!ts.isPropertySignature(member)) continue;
    const type = member.type;
    const target =
      type !== undefined && ts.isTypeReferenceNode(type)
        ? interfaces.get(type.typeName.getText())
        : undefined;
    out.set(
      nameOf(member.name),
      target === undefined ? null : [...new Set(membersOf(target))].sort(),
    );
  }
  return out;
}

describe("the typed runtime API (api.ts) and the host's prelude.js", () => {
  const prelude = preludeMembers();
  const api = apiMembers();

  it("have the same namespaces", () => {
    expect([...api.keys()].sort()).toEqual([...prelude.keys()].sort());
    expect(api.size).toBeGreaterThan(10);
  });

  for (const [namespace, members] of api) {
    if (members === null) continue;
    it(`nail.${namespace} has the same members`, () => {
      expect(members).toEqual(prelude.get(namespace));
    });
  }
});
