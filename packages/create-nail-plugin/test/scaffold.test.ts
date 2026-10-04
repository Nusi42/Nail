import { existsSync, mkdirSync, readFileSync, realpathSync, writeFileSync } from "node:fs";
import path from "node:path";

import { readZip } from "@nail/plugin-api/tools";
import { afterEach, describe, expect, it } from "vitest";

import { main } from "../index.mjs";
import {
  fill,
  scaffold,
  ScaffoldError,
  SDK_DIR,
  TEMPLATE_DIR,
  toDisplayName,
  toPluginId,
} from "../scaffold.mjs";
import {
  linkDependencies,
  loadBundle,
  node,
  readManifest,
  SDK,
  tempDir,
  typecheck,
  type LoadedPlugin,
} from "./helpers";

const EXPECTED_FILES = [
  ".gitignore",
  "README.md",
  "locales/en.ftl",
  "manifest.json",
  "package.json",
  "scripts/build.mjs",
  "scripts/pack.mjs",
  "src/index.tsx",
  "tsconfig.json",
];

/** The fields of a package.json these tests read. */
interface PackageJson {
  name?: string;
  devDependencies: Partial<Record<string, string>>;
}

function readPackage(file: string): PackageJson {
  return JSON.parse(readFileSync(file, "utf8")) as PackageJson;
}

const cleanups: (() => void)[] = [];
afterEach(() => {
  for (const c of cleanups.splice(0)) c();
});
function temp(prefix = "nail-scaffold-") {
  const t = tempDir(prefix);
  cleanups.push(t.cleanup);
  return t.dir;
}

describe("names", () => {
  it("derives ids and display names", () => {
    expect(toPluginId("My Plug-in!")).toBe("my-plug-in");
    expect(toPluginId("  Ünïcode Tööl  ")).toBe("unicode-tool");
    expect(toPluginId("42 things")).toBe("things");
    expect(toPluginId("___")).toBe("my-plugin");
    expect(toPluginId("a".repeat(80))).toHaveLength(64);
    expect(toDisplayName("follow-up")).toBe("Follow up");
  });

  it("fills placeholders as string literals in code and raw in text", () => {
    const values = { PLUGIN_NAME: 'Say "hi"', PLUGIN_ID: "say-hi" };
    expect(fill("a.tsx", 'const T = "__PLUGIN_NAME__";', values)).toBe('const T = "Say \\"hi\\"";');
    expect(fill("package.json", '{"name": "nail-plugin-__PLUGIN_ID__"}', values)).toBe(
      '{"name": "nail-plugin-say-hi"}',
    );
    expect(fill("README.md", "# __PLUGIN_NAME__", values)).toBe('# Say "hi"');
  });
});

describe("scaffold", () => {
  it("writes the template with the id, name and SDK", () => {
    const dir = path.join(temp(), "Weather Watch");
    const result = scaffold({ dir, name: 'Weather "Watch"' });
    expect(result.id).toBe("weather-watch");
    expect(result.files).toEqual(EXPECTED_FILES);
    for (const file of EXPECTED_FILES) expect(existsSync(path.join(dir, file))).toBe(true);

    const manifest = readManifest(dir);
    expect(manifest.id).toBe("weather-watch");
    expect(manifest.name).toBe('Weather "Watch"');
    expect(manifest.apiVersion).toBe("1");

    const pkg = readPackage(path.join(dir, "package.json"));
    expect(pkg.name).toBe("nail-plugin-weather-watch");
    const sdkSpec = pkg.devDependencies["@nail/plugin-api"] ?? "";
    expect(sdkSpec.startsWith("file:")).toBe(true);
    expect(realpathSync(path.resolve(dir, sdkSpec.slice(5)))).toBe(realpathSync(SDK_DIR));
    const sdkPkg = readPackage(path.join(SDK_DIR, "package.json"));
    expect(pkg.devDependencies.esbuild).toBe(sdkPkg.devDependencies.esbuild);
    expect(pkg.devDependencies.typescript).toBe(sdkPkg.devDependencies.typescript);

    const source = readFileSync(path.join(dir, "src", "index.tsx"), "utf8");
    expect(source).toContain('const TITLE = "Weather \\"Watch\\"";');
    expect(readFileSync(path.join(dir, "README.md"), "utf8")).toContain('# Weather "Watch"');
    // No placeholder is left anywhere.
    for (const file of EXPECTED_FILES) {
      expect(readFileSync(path.join(dir, file), "utf8"), file).not.toMatch(/__[A-Z_]+__/);
    }
  });

  it("takes an explicit id and SDK spec", () => {
    const dir = temp();
    const result = scaffold({ dir, id: "my-tool", sdk: "^0.1.0" });
    expect(result.name).toBe("My tool");
    const pkg = readPackage(path.join(dir, "package.json"));
    expect(pkg.devDependencies["@nail/plugin-api"]).toBe("^0.1.0");
  });

  it("refuses bad ids, bad names and non-empty folders", () => {
    const dir = temp();
    expect(() => scaffold({ dir, id: "Bad_Id" })).toThrow(ScaffoldError);
    expect(() => scaffold({ dir, id: "ok", name: "two\nlines" })).toThrow(/one line/);
    writeFileSync(path.join(dir, "keep.txt"), "mine");
    expect(() => scaffold({ dir, id: "ok" })).toThrow(/not empty/);
    expect(scaffold({ dir, id: "ok", force: true }).files).toEqual(EXPECTED_FILES);
    expect(readFileSync(path.join(dir, "keep.txt"), "utf8")).toBe("mine");
  });

  it("has a CLI", () => {
    const dir = path.join(temp(), "cli-plugin");
    const out: string[] = [];
    const err: string[] = [];
    const io = { log: (s: string) => out.push(s), error: (s: string) => err.push(s) };
    expect(main([dir, "--name", "CLI plug-in"], io)).toBe(0);
    expect(out[0]).toMatch(/^Created CLI plug-in \(cli-plugin, command template\) in /);
    expect(out).toContain("  npm run package      # -> dist/cli-plugin.nailplugin");
    expect(main([dir], io)).toBe(1);
    expect(err.at(-1)).toMatch(/not empty/);
    expect(main([], io)).toBe(2);
    expect(main([dir, "--bogus"], io)).toBe(2);
    expect(main([dir, "--id"], io)).toBe(2);
  });

  it("ships the template untouched by placeholders only where expected", () => {
    const manifest = readFileSync(path.join(TEMPLATE_DIR, "manifest.json"), "utf8");
    expect((JSON.parse(manifest) as { id?: unknown }).id).toBe("__PLUGIN_ID__");
  });
});

describe("a scaffolded plug-in", () => {
  let loaded: LoadedPlugin | undefined;
  afterEach(() => {
    loaded?.restore();
    loaded = undefined;
  });

  it("type-checks, builds, packs and runs", async () => {
    const dir = path.join(temp(), "hello");
    mkdirSync(dir);
    scaffold({ dir, id: "hello-plugin", name: "Hello" });
    linkDependencies(dir);

    typecheck(dir);
    const build = node(dir, "scripts/build.mjs");
    expect(build).toBe("");
    const bundle = readFileSync(path.join(dir, "dist", "index.js"), "utf8");
    // One self-contained ES module: no imports or requires left, no Node APIs.
    expect(bundle).not.toMatch(/^\s*import\s/m);
    expect(bundle).not.toMatch(/\brequire\(/);
    expect(bundle).toMatch(/export\s*\{\s*\w+ as default\s*\}/);

    const packed = node(dir, "scripts/pack.mjs");
    expect(packed).toMatch(/hello-plugin\.nailplugin/);
    const zip = readZip(readFileSync(path.join(dir, "dist", "hello-plugin.nailplugin")));
    expect(zip.map((e) => e.name)).toEqual(["manifest.json", "dist/index.js", "locales/en.ftl"]);

    const manifest = readManifest(dir);
    loaded = await loadBundle(path.join(dir, "dist", "index.js"), {
      manifest,
      dialog: (spec) => {
        expect(spec.title).toBe("Hello");
        return { button: "ok", values: { greeting: "Servus" } };
      },
    });
    expect(loaded.host.commandNames()).toEqual(["hello"]);
    await loaded.host.runCommand("hello", {
      surface: "reader",
      selection: { messageIds: ["m1", "m2"] },
    });
    expect(loaded.host.dialogs[0]?.body).toEqual([
      {
        type: "stack",
        children: [
          { type: "text", value: "Hello from a Nail plug-in!" },
          { type: "text", style: "muted", value: "Messages selected: 2" },
          { type: "input", id: "greeting", label: "Your greeting", default: "Hi", required: true },
        ],
      },
    ]);
    expect(loaded.host.notifications).toEqual([{ title: "Servus", level: "success" }]);
    expect(loaded.host.logs).toEqual([{ level: "info", message: 'greeted {"selected":2}' }]);
  });

  it("reports a missing build when packing", () => {
    const dir = temp();
    scaffold({ dir, id: "unbuilt", force: true });
    linkDependencies(dir);
    expect(() => node(dir, "scripts/pack.mjs")).toThrow(/build the plug-in first/);
  });

  it("links the SDK of this repository", () => {
    expect(realpathSync(SDK)).toBe(realpathSync(SDK_DIR));
  });
});
