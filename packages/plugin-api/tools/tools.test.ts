import { createHash } from "node:crypto";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";

import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { main } from "./cli.mjs";
import {
  checkManifest,
  createZip,
  isNetworkHost,
  isPackagePath,
  isVersion,
  PackError,
  packPlugin,
  readZip,
} from "./index.mjs";

const manifest = {
  id: "follow-up",
  name: "%name%",
  version: "1.0.0",
  apiVersion: "1",
  main: "dist/index.js",
  permissions: ["mail.read", "ui.dialogs", "storage", "notifications"],
  contributes: {
    commands: [{ id: "follow-up", title: "%title%", contexts: ["reader", "list"] }],
    placements: [{ toolbar: "reader", after: "mail.forward", command: "follow-up" }],
    keys: [{ key: "shift+f", command: "follow-up", context: "list" }],
    panels: [{ id: "due", title: "Due" }],
    settings: [{ type: "number", key: "days", title: "Days", default: 1 }],
    locales: { en: "locales/en.ftl" },
  },
};

describe("zip", () => {
  it("round-trips stored and deflated entries", () => {
    const entries = [
      { name: "manifest.json", data: Buffer.from("{}") },
      { name: "dist/index.js", data: Buffer.from("export default {};\n".repeat(200)) },
      { name: "assets/ü.bin", data: new Uint8Array([0, 1, 2, 255]) },
      { name: "empty.txt", data: new Uint8Array() },
    ];
    const zip = createZip(entries);
    const back = readZip(zip);
    expect(back.map((e) => e.name)).toEqual(entries.map((e) => e.name));
    back.forEach((e, i) => expect(Buffer.compare(e.data, Buffer.from(entries[i]!.data))).toBe(0));
    // The repetitive file is compressed.
    expect(zip.length).toBeLessThan(entries[1]!.data.length);
  });

  it("is deterministic", () => {
    const entries = [{ name: "a.txt", data: Buffer.from("hello") }];
    expect(createZip(entries).equals(createZip(entries))).toBe(true);
  });

  it("refuses duplicate names and detects corruption", () => {
    const e = { name: "a", data: Buffer.from("x") };
    expect(() => createZip([e, e])).toThrow(/duplicate/);
    expect(() => readZip(Buffer.from("not a zip at all, definitely not"))).toThrow(/not a zip/);
    const zip = createZip([{ name: "a.txt", data: Buffer.from("hello") }]);
    zip[30 + 5] = (zip[30 + 5] ?? 0) ^ 0xff; // first byte of the stored data
    expect(() => readZip(zip)).toThrow(/CRC/);
  });
});

describe("checkManifest", () => {
  it("accepts a good manifest", () => {
    expect(checkManifest(manifest)).toEqual([]);
  });

  it("reports problems with Nail's issue kinds", () => {
    const bad = {
      id: "Follow_Up",
      name: "",
      version: "1.0",
      apiVersion: "2",
      main: "../index.ts",
      homepage: "http://example.com",
      permissions: [
        "mail.read.body",
        "tasks.write",
        "mail.delete",
        "network:*.com",
        "storage",
        "storage",
      ],
      contributes: {
        commands: [
          { id: "a", title: "A", contexts: ["sidebar"] },
          { id: "a", title: "A" },
        ],
        placements: [
          { toolbar: "reader", command: "b" },
          { toolbar: "x", panel: "y" },
          { panel: "nope" },
        ],
        keys: [{ key: "", command: "a", context: "nowhere" }],
        panels: [{ id: "p", title: "" }],
        settings: [
          { type: "color", key: "c", title: "C" },
          { type: "boolean", key: "b", title: "B" },
          { type: "boolean", key: "b", title: "B" },
        ],
        locales: { english: "en.ftl", de: "locales/de.txt" },
      },
    };
    const kinds = checkManifest(bad).map((i) => `${i.kind} ${i.path}`);
    expect(kinds).toEqual([
      "invalid-id id",
      "invalid-name name",
      "invalid-version version",
      "unsupported-api-version apiVersion",
      "invalid-main main",
      "invalid-homepage homepage",
      "permission-not-in-preview permissions[1]",
      "unknown-permission permissions[2]",
      "invalid-network-domain permissions[3]",
      "duplicate-permission permissions[5]",
      "body-without-read permissions",
      "invalid-command contributes.commands[0].contexts",
      "duplicate-command contributes.commands[1].id",
      "invalid-panel contributes.panels[0]",
      "unknown-command contributes.placements[0].command",
      "invalid-placement contributes.placements[1]",
      "unknown-panel contributes.placements[2].panel",
      "invalid-key contributes.keys[0].key",
      "invalid-key contributes.keys[0].context",
      "invalid-setting contributes.settings[0]",
      "duplicate-setting contributes.settings[2].key",
      "invalid-locale contributes.locales.english",
      "invalid-locale contributes.locales.de",
    ]);
    expect(checkManifest(null)).toEqual([
      { kind: "invalid-json", path: "", detail: "not an object" },
    ]);
  });

  it("mirrors Nail's host patterns, versions and paths", () => {
    for (const ok of ["api.example.com", "*.example.org", "localhost", "127.0.0.1", "a-b.c1.io"]) {
      expect(isNetworkHost(ok), ok).toBe(true);
    }
    for (const bad of [
      "",
      "*",
      "*.com",
      "https://example.com",
      "example.com:443",
      "example.com/path",
      "Example.com",
      "-a.com",
      "a..com",
      "a.b.*.com",
      "1.2.3",
      "[::1]",
      "300.1.1.1",
    ]) {
      expect(isNetworkHost(bad), bad).toBe(false);
    }
    expect(isVersion("1.2.3-beta.1+build.5")).toBe(true);
    expect(isVersion("01.2.3")).toBe(false);
    expect(isPackagePath("dist/index.js")).toBe(true);
    for (const bad of ["/abs.js", "a/../b.js", "a\\b.js", "a//b.js", "./a.js", ""]) {
      expect(isPackagePath(bad), bad).toBe(false);
    }
  });
});

describe("packPlugin and the CLI", () => {
  let dir: string;
  beforeEach(() => {
    dir = mkdtempSync(path.join(tmpdir(), "nail-pack-"));
    writeFileSync(path.join(dir, "manifest.json"), JSON.stringify(manifest, null, 2));
    mkdirSync(path.join(dir, "dist"));
    mkdirSync(path.join(dir, "locales"));
    mkdirSync(path.join(dir, "assets", "img"), { recursive: true });
    mkdirSync(path.join(dir, "src"));
    writeFileSync(path.join(dir, "dist", "index.js"), "export default {};\n");
    writeFileSync(path.join(dir, "dist", "index.js.map"), "{}");
    writeFileSync(path.join(dir, "locales", "en.ftl"), "name = Follow up\ntitle = Follow up…\n");
    writeFileSync(path.join(dir, "assets", "img", "logo.png"), Buffer.from([137, 80, 78, 71]));
    writeFileSync(path.join(dir, "src", "index.tsx"), "// source, not packed\n");
  });
  afterEach(() => rmSync(dir, { recursive: true, force: true }));

  it("packs manifest, main, locales and assets only", () => {
    const result = packPlugin({ dir });
    expect(result.file).toBe(path.join(dir, "dist", "follow-up.nailplugin"));
    expect(result.files).toEqual([
      "manifest.json",
      "assets/img/logo.png",
      "dist/index.js",
      "locales/en.ftl",
    ]);
    const bytes = readFileSync(result.file);
    expect(result.size).toBe(bytes.length);
    expect(result.sha256).toBe(createHash("sha256").update(bytes).digest("hex"));
    const entries = readZip(bytes);
    expect(entries.map((e) => e.name)).toEqual(result.files);
    expect(JSON.parse(entries[0]!.data.toString("utf8")).id).toBe("follow-up");

    // Packing again (the old archive sits in dist/) gives the same bytes.
    const again = packPlugin({ dir, out: path.join(dir, "other.nailplugin") });
    expect(again.sha256).toBe(result.sha256);
  });

  it("refuses an unbuilt plug-in or a bad manifest", () => {
    rmSync(path.join(dir, "dist", "index.js"));
    expect(() => packPlugin({ dir })).toThrow(/build the plug-in first/);
    writeFileSync(path.join(dir, "manifest.json"), JSON.stringify({ ...manifest, id: "Bad" }));
    try {
      packPlugin({ dir });
      expect.unreachable();
    } catch (error) {
      expect(error).toBeInstanceOf(PackError);
      expect((error as PackError).issues.map((i) => i.kind)).toEqual(["invalid-id"]);
    }
    writeFileSync(path.join(dir, "manifest.json"), "{ nope");
    expect(() => packPlugin({ dir })).toThrow(/not valid JSON/);
    rmSync(path.join(dir, "manifest.json"));
    expect(() => packPlugin({ dir })).toThrow(/no manifest.json/);
  });

  it("runs check and pack from the command line", () => {
    const out: string[] = [];
    const err: string[] = [];
    const io = { log: (s: string) => out.push(s), error: (s: string) => err.push(s) };
    expect(main(["check", dir], io)).toBe(0);
    expect(out[0]).toBe("follow-up 1.0.0: manifest.json looks fine");
    const target = path.join(dir, "out", "x.nailplugin");
    expect(main(["pack", dir, "--out", target], io)).toBe(0);
    expect(readZip(readFileSync(target)).length).toBe(4);
    expect(out[2]).toMatch(/^ {2}4 files, \d+ bytes, sha256 [0-9a-f]{64}$/);
    expect(main(["pack", path.join(dir, "missing")], io)).toBe(1);
    expect(err.at(-1)).toMatch(/no manifest.json/);
    expect(main(["frobnicate"], io)).toBe(2);
    expect(main(["pack", "--bogus"], io)).toBe(2);
  });
});
