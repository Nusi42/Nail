#!/usr/bin/env node
/**
 * Generates the reference parts of the plug-in user docs from the SDK's
 * TypeScript types (which are themselves generated from the Rust contract):
 *
 *   userDocs/en/plugins/api-reference.md   between the GENERATED markers
 *   userDocs/en/plugins/components.md      between the GENERATED markers
 *
 *   node packages/plugin-api/scripts/gen-docs.mjs          write
 *   node packages/plugin-api/scripts/gen-docs.mjs --check  fail when stale
 *
 * The prose around the markers is written by hand.
 */
import { readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import ts from "typescript";

const here = path.dirname(fileURLToPath(import.meta.url));
const SDK = path.resolve(here, "..");
const REPO = path.resolve(SDK, "..", "..");
const DOCS = path.join(REPO, "userDocs", "en", "plugins");

export const BEGIN = "<!-- BEGIN GENERATED (npm run docs -w packages/plugin-api) -->";
export const END = "<!-- END GENERATED -->";

/** Paragraph separator of the generated Markdown. */
const NL2 = "\n\n";

/** Sections of the API reference: [heading, intro, [file, name][]]. */
const API_SECTIONS = [
  [
    "Plug-in entry point",
    "The default export of a plug-in's bundle, and what `activate` receives.",
    [
      ["define.ts", "definePlugin"],
      ["define.ts", "PluginSetup"],
      ["define.ts", "PluginContext"],
      ["api.ts", "PluginCommandHandler"],
      ["api.ts", "PluginIdentity"],
    ],
  ],
  [
    "The `nail` object",
    'Import it with `import { nail } from "@nail/plugin-api"`. It wraps the host\'s global `nail` and accepts JSX where the host wants component trees.',
    [
      ["nail.ts", "SdkNail"],
      ["api.ts", "NailMailApi"],
      ["api.ts", "PluginMailSearchOptions"],
      ["nail.ts", "SdkUiApi"],
      ["nail.ts", "DialogSpec"],
      ["api.ts", "PluginDialogOptions"],
      ["api.ts", "PluginUiAction"],
      ["nail.ts", "PanelProvider"],
      ["nail.ts", "PanelRender"],
      ["api.ts", "NailCommandsApi"],
      ["api.ts", "NailStorageApi"],
      ["api.ts", "PluginStorageUsage"],
      ["api.ts", "NailHttpApi"],
      ["api.ts", "PluginHttpRequest"],
      ["api.ts", "PluginHttpMethod"],
      ["api.ts", "PluginHttpResponse"],
      ["api.ts", "NailSettingsApi"],
      ["api.ts", "NailLogApi"],
    ],
  ],
  [
    "Errors",
    "Rejected calls carry a `code`.",
    [
      ["api.ts", "PluginApiError"],
      ["api.ts", "PluginApiErrorCode"],
      ["nail.ts", "isPluginApiError"],
    ],
  ],
  [
    "Command context and selection",
    "",
    [
      ["generated/PluginCommandContext.ts", "PluginCommandContext"],
      ["generated/PluginCommandSurface.ts", "PluginCommandSurface"],
      ["generated/PluginSelection.ts", "PluginSelection"],
    ],
  ],
  [
    "Dialogs and notifications",
    "",
    [
      ["generated/PluginDialogSpec.ts", "PluginDialogSpec"],
      ["generated/PluginDialogButton.ts", "PluginDialogButton"],
      ["generated/PluginDialogSize.ts", "PluginDialogSize"],
      ["generated/PluginDialogResult.ts", "PluginDialogResult"],
      ["generated/PluginNotificationSpec.ts", "PluginNotificationSpec"],
      ["generated/PluginNotificationLevel.ts", "PluginNotificationLevel"],
    ],
  ],
  [
    "Mail",
    "What `nail.mail` returns.",
    [
      ["generated/PluginMessage.ts", "PluginMessage"],
      ["generated/PluginMessageBody.ts", "PluginMessageBody"],
      ["generated/PluginAttachment.ts", "PluginAttachment"],
      ["generated/Address.ts", "Address"],
      ["generated/Flags.ts", "Flags"],
      ["generated/MessageId.ts", "MessageId"],
      ["generated/ThreadId.ts", "ThreadId"],
      ["generated/AccountId.ts", "AccountId"],
      ["generated/FolderId.ts", "FolderId"],
      ["generated/serde_json/JsonValue.ts", "JsonValue"],
    ],
  ],
  [
    "JSX and trees",
    "",
    [
      ["jsx-runtime.ts", "toNodes"],
      ["jsx-runtime.ts", "UiChild"],
      ["tree.ts", "checkTree"],
      ["tree.ts", "TreeIssue"],
    ],
  ],
  [
    "Testing plug-ins",
    "From `@nail/plugin-api/testing`.",
    [
      ["testing.ts", "createTestHost"],
      ["testing.ts", "TestHostOptions"],
      ["testing.ts", "TestHost"],
    ],
  ],
];

/** Manifest types, for the manifest reference's tables. */
const MANIFEST_TYPES = [
  ["generated/PluginManifest.ts", "PluginManifest"],
  ["generated/PluginContributions.ts", "PluginContributions"],
  ["generated/PluginCommandDef.ts", "PluginCommandDef"],
  ["generated/PluginPlacement.ts", "PluginPlacement"],
  ["generated/PluginKeyDef.ts", "PluginKeyDef"],
  ["generated/PluginPanelDef.ts", "PluginPanelDef"],
  ["generated/PluginSettingDef.ts", "PluginSettingDef"],
  ["generated/PluginSelectOption.ts", "PluginSelectOption"],
  ["generated/PluginPermission.ts", "PluginPermission"],
];

/** @typedef {[file: string, name: string]} Item */
/** @typedef {[heading: string, intro: string, items: Item[]]} Section */
/** @typedef {import("typescript").Node} Node */
/** @typedef {import("typescript").SourceFile} SourceFile */
/** @typedef {import("typescript").TypeElement} TypeElement */
/** @typedef {import("typescript").Program} Program */

/** @returns {Program} */
function program() {
  const sections = /** @type {Section[]} */ (API_SECTIONS);
  const items = /** @type {Item[]} */ ([...sections.flatMap((s) => s[2]), ...MANIFEST_TYPES]);
  const files = [
    ...new Set(items.map(([f]) => path.join(SDK, "src", f))),
    path.join(SDK, "src", "generated", "PluginUiNode.ts"),
  ];
  return ts.createProgram(files, {
    target: ts.ScriptTarget.ES2022,
    module: ts.ModuleKind.ESNext,
    moduleResolution: ts.ModuleResolutionKind.Bundler,
    jsx: ts.JsxEmit.ReactJSX,
    strict: true,
    noEmit: true,
    types: [],
  });
}

/**
 * Cleans a doc comment: rustdoc links, line wraps.
 * @param {string} text
 * @returns {string}
 */
export function cleanDoc(text) {
  return text
    .replace(/\[`([^`\]]+)`\]/g, (_, /** @type {string} */ name) => `\`${name.split("::").pop()}\``)
    .replace(/\{@link ([^}]+)\}/g, "`$1`")
    .replace(/\s*\n\s*/g, " ")
    .trim();
}

/**
 * For table cells: escape pipes, collapse whitespace.
 * @param {string} text
 */
function cell(text) {
  return text.replace(/\s+/g, " ").replace(/\|/g, "\\|").trim();
}

/**
 * The cleaned JSDoc summary of a node.
 * @param {Node} node
 * @param {SourceFile} sf
 */
function docOf(node, sf) {
  const ranges = ts.getLeadingCommentRanges(sf.text, node.getFullStart()) ?? [];
  const docs = ranges
    .map((r) => sf.text.slice(r.pos, r.end))
    .filter((c) => c.startsWith("/**"))
    .map((c) =>
      c
        .replace(/^\/\*\*/, "")
        .replace(/\*\/$/, "")
        .split("\n")
        .map((l) => l.replace(/^\s*\* ?/, ""))
        .join("\n"),
    );
  const text = docs.at(-1) ?? "";
  // Drop code examples and tags from the summary.
  return cleanDoc(text.replace(/```[\s\S]*?```/g, "").replace(/^@\w+.*$/gm, ""));
}

/**
 * A type's source text on one line, without comments.
 * @param {Node | undefined} node
 * @param {SourceFile} sf
 */
function typeText(node, sf) {
  if (!node) return "";
  return node
    .getText(sf)
    .replace(/\/\*[\s\S]*?\*\//g, "")
    .replace(/\s+/g, " ")
    .replace(/\{ /g, "{ ")
    .trim();
}

/**
 * The type of a property signature (or none).
 * @param {TypeElement | undefined} m
 * @returns {Node | undefined}
 */
function memberType(m) {
  return m && ts.isPropertySignature(m) ? m.type : undefined;
}

/**
 * A Markdown table of members.
 * @param {readonly TypeElement[]} members
 * @param {SourceFile} sf
 */
function membersTable(members, sf) {
  const rows = [];
  for (const m of members) {
    if (!m.name) continue;
    const name = m.name.getText(sf).replace(/^"|"$/g, "");
    let type;
    if (ts.isMethodSignature(m)) {
      const params = m.parameters.map((p) => p.getText(sf)).join(", ");
      const generics = m.typeParameters
        ? `<${m.typeParameters.map((t) => t.getText(sf)).join(", ")}>`
        : "";
      type = `${generics}(${params}) => ${typeText(m.type, sf)}`;
    } else {
      type = typeText(memberType(m), sf);
    }
    const optional = m.questionToken ? "?" : "";
    rows.push(`| \`${name}${optional}\` | \`${cell(type)}\` | ${cell(docOf(m, sf))} |`);
  }
  if (rows.length === 0) return "";
  return ["| Member | Type | Description |", "| --- | --- | --- |", ...rows].join("\n");
}

/**
 * One declaration as a Markdown section.
 * @param {Node} decl
 * @param {SourceFile} sf
 * @param {string} name
 * @param {number} level heading level
 */
function renderDeclaration(decl, sf, name, level) {
  const heading = `${"#".repeat(level)} \`${name}\``;
  const doc = docOf(decl, sf);
  const parts = [heading];
  if (doc) parts.push(doc);
  if (ts.isInterfaceDeclaration(decl)) {
    const ext = decl.heritageClauses?.map((h) => h.getText(sf)).join(" ");
    if (ext) parts.push(`\`${ext}\``);
    const table = membersTable(decl.members, sf);
    if (table) parts.push(table);
  } else if (ts.isTypeAliasDeclaration(decl)) {
    const type = decl.type;
    if (ts.isTypeLiteralNode(type)) {
      parts.push(membersTable(type.members, sf));
    } else if (ts.isUnionTypeNode(type) && type.types.every(ts.isTypeLiteralNode)) {
      for (const member of type.types) {
        const tag = member.members.find((m) => m.name?.getText(sf) === '"type"');
        const label = tag ? typeText(memberType(tag), sf) : "variant";
        parts.push(`**\`type: ${label}\`**`);
        parts.push(
          membersTable(
            member.members.filter((m) => m !== tag),
            sf,
          ),
        );
      }
    } else {
      parts.push("```ts\ntype " + name + " = " + typeText(type, sf) + ";\n```");
    }
  } else if (ts.isFunctionDeclaration(decl)) {
    const sig = decl
      .getText(sf)
      .replace(/\{[\s\S]*$/, "")
      .replace(/^export\s+/, "")
      .replace(/\s+/g, " ")
      .trim();
    parts.push("```ts\n" + sig + "\n```");
  } else if (ts.isVariableStatement(decl)) {
    parts.push(
      "```ts\n" + (decl.getText(sf).split("=")[0] ?? "").replace(/^export\s+/, "").trim() + "\n```",
    );
  }
  return parts.filter(Boolean).join("\n\n");
}

/**
 * Finds an exported declaration.
 * @param {Program} prog
 * @param {string} file under src/
 * @param {string} name
 * @returns {{ decl: import("typescript").Statement, sf: SourceFile }}
 */
function findDeclaration(prog, file, name) {
  const sf = prog.getSourceFile(path.join(SDK, "src", file));
  if (!sf) throw new Error(`source file not found: ${file}`);
  for (const stmt of sf.statements) {
    if (
      (ts.isInterfaceDeclaration(stmt) ||
        ts.isTypeAliasDeclaration(stmt) ||
        ts.isFunctionDeclaration(stmt)) &&
      stmt.name?.text === name
    ) {
      return { decl: stmt, sf };
    }
    if (
      ts.isVariableStatement(stmt) &&
      stmt.declarationList.declarations.some((d) => d.name.getText(sf) === name)
    ) {
      return { decl: stmt, sf };
    }
  }
  throw new Error(`declaration ${name} not found in ${file}`);
}

/** The generated body of api-reference.md. */
export function apiReference(prog = program()) {
  const out = [];
  for (const [heading, intro, items] of /** @type {Section[]} */ (API_SECTIONS)) {
    out.push(`## ${heading}`);
    if (intro) out.push(intro);
    for (const [file, name] of items) {
      const { decl, sf } = findDeclaration(prog, file, name);
      out.push(renderDeclaration(decl, sf, name, 3));
    }
  }
  out.push("## Manifest types");
  out.push("The types of `manifest.json` (see the manifest reference for the rules).");
  for (const [file, name] of /** @type {Item[]} */ (MANIFEST_TYPES)) {
    const { decl, sf } = findDeclaration(prog, file, name);
    out.push(renderDeclaration(decl, sf, name, 3));
  }
  return out.join("\n\n");
}

/** The Rust source of the component set (for the per-component descriptions and values). */
const UI_RS = path.join(REPO, "crates", "nail-api", "src", "plugins", "ui.rs");

/**
 * `ContactPicker` → `contact-picker`.
 * @param {string} name
 */
function kebab(name) {
  return name.replace(
    /[A-Z]/g,
    (/** @type {string} */ c, /** @type {number} */ i) => (i === 0 ? "" : "-") + c.toLowerCase(),
  );
}

/**
 * Reads the doc comment of every `PluginUiNode` variant and the module's
 * table of input values from `ui.rs`.
 * @returns {{ descriptions: Record<string, string>, values: Record<string, string> }}
 */
export function rustUiDocs(source = readFileSync(UI_RS, "utf8")) {
  const lines = source.split(/\r?\n/);
  const start = lines.findIndex((l) => l.startsWith("pub enum PluginUiNode"));
  if (start < 0) throw new Error("PluginUiNode not found in ui.rs");
  /** @type {Record<string, string>} */
  const descriptions = {};
  let doc = [];
  for (const line of lines.slice(start + 1)) {
    if (line === "}") break;
    const d = /^ {4}\/\/\/ ?(.*)$/.exec(line);
    const v = /^ {4}([A-Z][A-Za-z]*) \{/.exec(line);
    if (d) doc.push(d[1] ?? "");
    else if (v) {
      descriptions[kebab(v[1] ?? "")] = cleanDoc(doc.join("\n"));
      doc = [];
    } else if (!/^ {4}#\[/.test(line)) doc = [];
  }
  /** @type {Record<string, string>} */
  const values = {};
  for (const line of lines) {
    const row = /^\/\/! \| (`[^|]+?) \| (.+) \|$/.exec(line);
    if (!row) continue;
    const value = (row[2] ?? "").trim();
    for (const node of (row[1] ?? "").matchAll(/`([a-z-]+)`( \(`selectable`\))?/g)) {
      values[node[1] ?? ""] = node[2] ? `${value} (when \`selectable\`)` : value;
    }
  }
  return { descriptions, values };
}

/** The generated body of components.md. */
export function components(prog = program(), rust = rustUiDocs()) {
  const { decl, sf } = findDeclaration(prog, "generated/PluginUiNode.ts", "PluginUiNode");
  if (!ts.isTypeAliasDeclaration(decl)) throw new Error("PluginUiNode is not a type alias");
  const union = decl.type;
  if (!ts.isUnionTypeNode(union)) throw new Error("PluginUiNode is not a union");
  const out = [];
  for (const member of union.types) {
    if (!ts.isTypeLiteralNode(member)) continue;
    const tag = member.members.find((m) => m.name?.getText(sf) === '"type"');
    const type = typeText(memberType(tag), sf).replace(/"/g, "");
    const description = rust.descriptions[type];
    if (!description) throw new Error(`no description of ${type} in ui.rs`);
    out.push(`### \`<${type}>\``);
    out.push(description);
    if (type === "text" || type === "markdown") {
      out.push(
        `In JSX the \`value\` can be the element's text: \`<${type}>Hello {name}</${type}>\`.`,
      );
    } else if (type === "stack") {
      out.push("In JSX its `children` are the nested elements; strings become `<text>` nodes.");
    }
    const value = rust.values[type];
    if (value) out.push(`Value in dialog results and UI actions: ${value}.`);
    out.push(
      membersTable(
        member.members.filter((m) => m !== tag),
        sf,
      ),
    );
  }
  return out.join(NL2);
}

/**
 * Replaces the generated part of a document.
 * @param {string} doc
 * @param {string} body
 */
export function splice(doc, body) {
  const start = doc.indexOf(BEGIN);
  const end = doc.indexOf(END);
  if (start < 0 || end < start) throw new Error("GENERATED markers not found");
  return `${doc.slice(0, start + BEGIN.length)}\n\n${body}\n\n${doc.slice(end)}`;
}

/**
 * The documents and their generated bodies.
 * @returns {[file: string, body: string][]}
 */
export function documents(prog = program()) {
  return [
    [path.join(DOCS, "api-reference.md"), apiReference(prog)],
    [path.join(DOCS, "components.md"), components(prog)],
  ];
}

function main() {
  const check = process.argv.includes("--check");
  let stale = 0;
  for (const [file, body] of documents()) {
    const current = readFileSync(file, "utf8");
    const next = splice(current, body);
    if (next === current) continue;
    if (check) {
      console.error(
        `${path.relative(REPO, file)} is stale: run npm run docs -w packages/plugin-api`,
      );
      stale += 1;
    } else {
      writeFileSync(file, next);
      console.log(`wrote ${path.relative(REPO, file)}`);
    }
  }
  process.exitCode = stale > 0 ? 1 : 0;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main();
}
