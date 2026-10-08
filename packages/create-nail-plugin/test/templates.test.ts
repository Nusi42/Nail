/**
 * The hook, composer and attachment templates (M4a): each scaffolds,
 * passes the manifest check, type-checks, bundles, packs and behaves as
 * its comments say against the SDK's test host.
 */
import { readFileSync } from "node:fs";
import path from "node:path";

import type { PluginMessage, PluginOutgoingMessage } from "@nail/plugin-api";
import { testMessage, testOutgoingMessage } from "@nail/plugin-api/testing";
import { checkManifest } from "@nail/plugin-api/tools";
import { afterAll, beforeAll, describe, expect, it } from "vitest";

import { main } from "../index.mjs";
import { scaffold, ScaffoldError, TEMPLATES } from "../scaffold.mjs";
import {
  buildAndPack,
  linkDependencies,
  pluginLoader,
  readManifest,
  tempDir,
  typecheck,
} from "./helpers";

const root = tempDir("nail-templates-");
afterAll(() => {
  root.cleanup();
});

/** Scaffolds, type-checks, builds and packs a template; returns its folder. */
function build(template: string): string {
  const dir = path.join(root.dir, template);
  const result = scaffold({ dir, template, id: `my-${template}`, name: `My ${template}` });
  expect(result.template).toBe(template);
  expect(checkManifest(readManifest(dir))).toEqual([]);
  linkDependencies(dir);
  typecheck(dir);
  expect(buildAndPack(dir, `my-${template}`)).toEqual([
    "manifest.json",
    "dist/index.js",
    "locales/en.ftl",
  ]);
  // Every %id% of the manifest is in the catalog.
  const catalog = readFileSync(path.join(dir, "locales", "en.ftl"), "utf8");
  for (const t of JSON.stringify(readManifest(dir)).match(/%[a-z-]+%/g) ?? []) {
    expect(catalog, t).toMatch(new RegExp(`^${t.slice(1, -1)} = `, "m"));
  }
  return dir;
}

const message = (email: string): PluginMessage =>
  testMessage("m1", { subject: "Hello", from: { name: "Sender", email } });

describe("templates", () => {
  it("are offered by name and refuse unknown ones", () => {
    expect(TEMPLATES).toEqual(["command", "hook", "composer", "attachment"]);
    expect(() => scaffold({ dir: path.join(root.dir, "x"), template: "theme" })).toThrow(
      ScaffoldError,
    );
    const out: string[] = [];
    const err: string[] = [];
    const io = { log: (s: string) => out.push(s), error: (s: string) => err.push(s) };
    expect(main([path.join(root.dir, "cli-hook"), "--template", "hook"], io)).toBe(0);
    expect(out[0]).toMatch(/\(cli-hook, hook template\)/);
    expect(main([path.join(root.dir, "cli-x"), "--template", "nope"], io)).toBe(1);
    expect(err.at(-1)).toMatch(/unknown template "nope"/);
  });
});

describe("the hook template", () => {
  let dir = "";
  beforeAll(() => {
    dir = build("hook");
  });
  const load = pluginLoader(() => dir);

  const outgoing = (subject: string): PluginOutgoingMessage =>
    testOutgoingMessage({ from: { name: null, email: "me@example.com" }, subject });

  it("vetoes a message without a subject", async () => {
    const host = await load();
    expect(host.hookNames().sort()).toEqual(["beforeSend", "open"]);
    expect(await host.runBeforeSend(outgoing("  "))).toEqual({
      outcome: "vetoed",
      result: { veto: { reason: "This message has no subject. Add one, or send it anyway." } },
      problems: [],
    });
    expect((await host.runBeforeSend(outgoing("Report"))).outcome).toBe("ok");
  });

  it("marks mail from outside the domain", async () => {
    const host = await load({ settings: { "internal-domain": "example.com" } });
    const external = await host.runOpen(message("ada@other.example"));
    expect(external.result).toEqual({
      badges: [
        {
          text: "External",
          level: "warning",
          tooltip: "Sent from other.example, outside example.com",
        },
      ],
    });
    expect((await host.runOpen(message("bob@example.com"))).result).toBeUndefined();
    expect((await host.runOpen(message("eve@mail.example.com"))).result).toBeUndefined();
  });
});

describe("the composer template", () => {
  let dir = "";
  beforeAll(() => {
    dir = build("composer");
  });
  const load = pluginLoader(() => dir);

  it("inserts the date and upper-cases the selection", async () => {
    const host = await load();
    expect(host.extensionNames().composerActions).toEqual(["insert-date", "upper-case"]);
    await host.runComposerAction("insert-date", { draftId: "d1" });
    expect(host.composerEdits[0]?.edit.text).toMatch(/^\d{4}-\d{2}-\d{2}$/);
    await host.runComposerAction("upper-case", {
      draftId: "d1",
      selection: { text: "make it loud", start: 0, end: 12 },
    });
    expect(host.composerEdits[1]).toEqual({
      draftId: "d1",
      edit: { op: "replace-selection", text: "MAKE IT LOUD" },
    });
    // No selection: nothing to do.
    await host.runComposerAction("upper-case", { draftId: "d1" });
    expect(host.composerEdits).toHaveLength(2);
  });

  it("renders callout blocks with escaped HTML", async () => {
    const host = await load();
    expect(await host.renderMarkdownBlock("callout", " a < b \n", "send")).toEqual({
      html: "<blockquote><p><strong>Note:</strong> a &lt; b</p></blockquote>",
      text: "Note: a < b",
    });
  });
});

describe("the attachment template", () => {
  let dir = "";
  beforeAll(() => {
    dir = build("attachment");
  });
  const load = pluginLoader(() => dir);

  it("previews text files as a table", async () => {
    const host = await load();
    const preview = await host.renderPreview(
      "text",
      { name: "notes.txt", mimeType: "text/plain" },
      "first line\nsecond line\n",
    );
    expect(preview).toEqual({
      kind: "tree",
      tree: [
        {
          type: "stack",
          children: [
            { type: "text", style: "strong", value: "2 lines, 4 words" },
            {
              type: "table",
              columns: [{ label: "Line" }, { label: "Text" }],
              rows: [
                ["1", "first line"],
                ["2", "second line"],
              ],
            },
          ],
        },
      ],
    });
    const long = Array.from({ length: 50 }, (_, i) => `line ${String(i)}`).join("\n");
    const big = await host.renderPreview("text", { name: "a.log" }, long);
    const stack = big.kind === "tree" ? big.tree[0] : undefined;
    expect(stack?.type === "stack" && stack.children.at(-1)).toEqual({
      type: "text",
      style: "muted",
      value: "The first 40 lines are shown.",
    });
    await expect(
      host.renderPreview("text", { name: "huge.txt" }, new Uint8Array(1_048_577)),
    ).rejects.toMatchObject({ code: "too_large" });
  });

  it("counts words in an attachment", async () => {
    const host = await load();
    await host.runAttachmentAction("count-words", { name: "a.txt" }, "one two three");
    expect(host.notifications).toEqual([{ title: "“a.txt” has 3 words." }]);
  });
});
