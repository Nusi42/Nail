/**
 * The "CSV preview" example (examples/csv-preview): CSV parsing and the
 * preview renderer against the SDK's test host (a table component).
 */
import path from "node:path";
import { pathToFileURL } from "node:url";

import type { PluginPreview, PluginUiNode } from "@nail/plugin-api";
import { loadCatalogs } from "@nail/plugin-api/tools";
import { afterAll, afterEach, beforeAll, describe, expect, it } from "vitest";

import {
  loadBundle,
  prepareExample,
  readManifest,
  type LoadedPlugin,
  type PreparedExample,
} from "./helpers";

type Csv = typeof import("../../../examples/csv-preview/src/csv");

let example: PreparedExample;
let csv: Csv;
beforeAll(async () => {
  example = prepareExample("csv-preview");
  csv = (await import(pathToFileURL(path.join(example.source, "src", "csv.ts")).href)) as Csv;
});
afterAll(() => example.cleanup());

describe("CSV parsing", () => {
  it("handles quotes, doubled quotes, line breaks in quotes and CRLF", () => {
    const text = '﻿name,quote\r\n"Lovelace, Ada","She said ""hi"""\r\n"multi\nline",x\r\n';
    expect(csv.parseCsv(text, ",")).toEqual({
      rows: [
        ["name", "quote"],
        ["Lovelace, Ada", 'She said "hi"'],
        ["multi\nline", "x"],
      ],
      more: false,
      total: 3,
    });
    expect(csv.parseCsv("a,b\n\nc", ",").rows).toEqual([["a", "b"], [""], ["c"]]);
    expect(csv.parseCsv("", ",").rows).toEqual([]);
    expect(csv.parseCsv("a\nb\nc\nd", ",", 2)).toEqual({
      rows: [["a"], ["b"]],
      more: true,
      total: 4,
    });
  });

  it("detects the delimiter", () => {
    expect(csv.detectDelimiter("a;b;c\n1,5;2;3")).toBe(";");
    expect(csv.detectDelimiter('"x,y";z\n')).toBe(";"); // commas inside quotes do not count
    expect(csv.detectDelimiter("a\tb")).toBe("\t");
    expect(csv.detectDelimiter("a|b|c")).toBe("|");
    expect(csv.detectDelimiter("single")).toBe(",");
    expect(csv.detectDelimiter("a,b", "x.tsv")).toBe("\t");
    expect(csv.detectDelimiter("a,b", "x", "text/tab-separated-values")).toBe("\t");
  });

  it("shapes tables: header, padding, column and cell limits", () => {
    const label = (n: number) => `Column ${n}`;
    const parsed = { rows: [["id", ""], ["1", "a", "extra"], ["2"]], total: 3 };
    expect(csv.toTable(parsed, true, 10, label)).toEqual({
      columns: ["id", "Column 2", "Column 3"],
      rows: [
        ["1", "a", "extra"],
        ["2", "", ""],
      ],
      total: 2,
    });
    expect(csv.toTable(parsed, false, 1, label).rows).toEqual([["id", ""]]);
    const wide = { rows: [Array.from({ length: 60 }, (_, i) => `c${i}`)], total: 1 };
    expect(csv.toTable(wide, false, 10, label).columns).toHaveLength(csv.MAX_COLUMNS);
    const long = { rows: [["x".repeat(600)]], total: 1 };
    const cell = csv.toTable(long, false, 10, label).rows[0]?.[0] ?? "";
    expect(cell).toHaveLength(csv.MAX_CELL);
    expect(cell.endsWith("…")).toBe(true);
    expect(csv.toTable({ rows: [], total: 0 }, true, 10, label)).toEqual({
      columns: [],
      rows: [],
      total: 0,
    });
  });
});

describe("the preview renderer", () => {
  let loaded: LoadedPlugin | undefined;
  afterEach(() => {
    loaded?.restore();
    loaded = undefined;
  });

  async function load(options: Parameters<typeof loadBundle>[1] = {}) {
    loaded = await loadBundle(example.bundle, {
      manifest: readManifest(example.source),
      catalogs: loadCatalogs(example.source),
      ...options,
    });
    return loaded.host;
  }

  const children = (preview: PluginPreview) =>
    preview.kind === "tree"
      ? (preview.tree[0] as Extract<PluginUiNode, { type: "stack" }>).children
      : [];

  it("renders a CSV attachment as a table", async () => {
    const host = await load();
    expect(host.extensionNames().attachmentPreviews).toEqual(["table"]);
    const preview = await host.renderPreview(
      "table",
      { name: "people.csv", mimeType: "text/csv" },
      "Name,City\nAda,London\nGrace,New York\n",
    );
    expect(preview).toEqual({
      kind: "tree",
      tree: [
        {
          type: "stack",
          children: [
            { type: "text", style: "muted", value: "2 rows, 2 columns" },
            {
              type: "table",
              columns: [{ label: "Name" }, { label: "City" }],
              rows: [
                ["Ada", "London"],
                ["Grace", "New York"],
              ],
            },
          ],
        },
      ],
    });
  });

  it("reads Windows-1252 exports with semicolons and tab-separated files", async () => {
    const host = await load();
    // "Straße;Größe\nHauptstraße;3" in Windows-1252.
    const latin1 = Uint8Array.from([
      0x53, 0x74, 0x72, 0x61, 0xdf, 0x65, 0x3b, 0x47, 0x72, 0xf6, 0xdf, 0x65, 0x0a, 0x48, 0x61,
      0x75, 0x70, 0x74, 0x73, 0x74, 0x72, 0x61, 0xdf, 0x65, 0x3b, 0x33,
    ]);
    const table = children(await host.renderPreview("table", { name: "export.csv" }, latin1))[1];
    expect(table).toEqual({
      type: "table",
      columns: [{ label: "Straße" }, { label: "Größe" }],
      rows: [["Hauptstraße", "3"]],
    });
    const tsv = children(await host.renderPreview("table", { name: "a.tsv" }, "a,1\tb\nc\td"))[1];
    expect(tsv).toMatchObject({ columns: [{ label: "a,1" }, { label: "b" }], rows: [["c", "d"]] });
  });

  it("follows the settings and says when rows are left out", async () => {
    const lines = Array.from({ length: 30 }, (_, i) => `${i},v${i}`).join("\n");
    const host = await load({ settings: { "header-row": false, "max-rows": 10 } });
    const parts = children(await host.renderPreview("table", { name: "n.csv" }, lines));
    expect(parts[0]).toEqual({ type: "text", style: "muted", value: "30 rows, 2 columns" });
    expect(parts[1]).toMatchObject({ columns: [{ label: "Column 1" }, { label: "Column 2" }] });
    expect((parts[1] as Extract<PluginUiNode, { type: "table" }>).rows).toHaveLength(10);
    expect(parts[2]).toEqual({
      type: "text",
      style: "muted",
      value: "The first 10 rows are shown.",
    });
  });

  it("keeps within the table component's limits", async () => {
    const lines = Array.from({ length: 2000 }, (_, i) => `${i}`).join("\n");
    const host = await load({ settings: { "max-rows": 100_000 } });
    const parts = children(await host.renderPreview("table", { name: "big.csv" }, `n\n${lines}`));
    expect((parts[1] as Extract<PluginUiNode, { type: "table" }>).rows).toHaveLength(499);
    expect(parts[0]).toMatchObject({ value: "2000 rows, one column" });
  });

  it("explains an empty file and refuses files over its limit", async () => {
    const host = await load();
    expect(await host.renderPreview("table", { name: "empty.csv" }, "")).toEqual({
      kind: "tree",
      tree: [{ type: "text", style: "muted", value: "The file is empty." }],
    });
    await expect(
      host.renderPreview("table", { name: "huge.csv" }, new Uint8Array(2 * 1024 * 1024 + 1)),
    ).rejects.toMatchObject({ code: "too_large" });
  });

  it("needs the attachments permission", async () => {
    const host = await load({ permissions: [] });
    await expect(host.renderPreview("table", { name: "a.csv" }, "a")).rejects.toMatchObject({
      code: "forbidden",
    });
  });
});
