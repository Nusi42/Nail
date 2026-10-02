import { describe, expect, it } from "vitest";
import type { AttachmentInfo, PluginInfo } from "../../api/generated";
import {
  applyComposerEdit,
  attachmentActionsFor,
  blockFence,
  composerActionsFor,
  contributionItems,
  contributionMessageId,
  hasGrant,
  hookMessageId,
  hostMatches,
  isWritePermission,
  linkActionsFor,
  linkHost,
  markdownBlockOwner,
  matchesFile,
  parseExtensionId,
  previewRenderersFor,
} from "./extensions";
import {
  demoExtensionManifests,
  digestManifest,
  filePreviewManifest,
  linkGuardManifest,
} from "./extensionsSeed";
import { NEEDS_READ, requiresRead, setGranted } from "./grants";
import { permissionMessageId, runtimeIds } from "./messages";
import { cpuLevel, cpuPercent } from "./Activity";
import { previewImageSrc } from "./AttachmentPreview";

function plugin(manifest: PluginInfo["manifest"], patch: Partial<PluginInfo> = {}): PluginInfo {
  return {
    id: manifest.id,
    manifest,
    source: { kind: "dev", folder: "/x" },
    enabled: true,
    state: "running",
    grants: (manifest.permissions ?? []).map((permission) => ({ permission, accounts: null })),
    storage: { keys: 0, used_bytes: 0, quota_bytes: 1 },
    package_sha256: null,
    installed_at: "2026-10-02T08:00:00Z",
    updated_at: "2026-10-02T08:00:00Z",
    error: null,
    ...patch,
  };
}

const csv: AttachmentInfo = {
  index: 1,
  filename: "Q3.CSV",
  mime: "text/plain",
  size: 10,
  inline: false,
};
const pdf: AttachmentInfo = {
  index: 0,
  filename: "r.pdf",
  mime: "application/pdf",
  size: 6e6,
  inline: false,
};

describe("extension matching", () => {
  it("matches files by MIME pattern or extension, like the core", () => {
    expect(matchesFile(["text/csv"], [], "TEXT/CSV", null)).toBe(true);
    expect(matchesFile(["image/*"], [], "image/png", null)).toBe(true);
    expect(matchesFile(["image/*"], [], "imagex/png", null)).toBe(false);
    expect(matchesFile([], ["csv"], "text/plain", "a.b.CSV")).toBe(true);
    expect(matchesFile([], ["csv"], "text/plain", "csv")).toBe(false);
  });

  it("offers preview renderers of running, granted plug-ins within their size", () => {
    const p = plugin(filePreviewManifest());
    expect(previewRenderersFor([p], csv).map((o) => o.id)).toEqual(["file-preview.csv"]);
    // The PDF renderer takes at most 5 MB.
    expect(previewRenderersFor([p], pdf)).toEqual([]);
    expect(previewRenderersFor([p], { ...pdf, size: 1000 }).map((o) => o.title)).toEqual([
      "First page",
    ]);
    expect(previewRenderersFor([plugin(filePreviewManifest(), { state: "crashed" })], csv)).toEqual(
      [],
    );
    expect(previewRenderersFor([plugin(filePreviewManifest(), { grants: [] })], csv)).toEqual([]);
    const limited = plugin(filePreviewManifest(), {
      grants: [{ permission: "attachments", accounts: ["acc-work"] }],
    });
    expect(previewRenderersFor([limited], csv, "acc-personal")).toEqual([]);
    expect(previewRenderersFor([limited], csv, "acc-work")).toHaveLength(1);
    expect(hasGrant(limited, "attachments")).toBe(true);
  });

  it("offers attachment actions that apply (no types = every attachment)", () => {
    const p = plugin(filePreviewManifest());
    expect(attachmentActionsFor([p], csv).map((o) => o.id)).toEqual(["file-preview.summary"]);
    expect(attachmentActionsFor([p], pdf)).toEqual([]);
  });

  it("matches link hosts like network permissions", () => {
    expect(linkHost("https://Docs.Example.com/x")).toBe("docs.example.com");
    expect(linkHost("mailto:ada@example.org")).toBe("");
    expect(linkHost("javascript:alert(1)")).toBeNull();
    expect(linkHost("not a url")).toBeNull();
    expect(hostMatches("*.example.com", "docs.example.com")).toBe(true);
    expect(hostMatches("*.example.com", "example.com")).toBe(false);
    expect(hostMatches("*.example.com", "badexample.com")).toBe(false);
    expect(hostMatches("jira.example.com", "jira.example.com.")).toBe(true);
    const guard = plugin(linkGuardManifest());
    expect(linkActionsFor([guard], "https://x.example/a").map((o) => o.title)).toEqual([
      "Check link reputation",
    ]);
    expect(linkActionsFor([guard], "file:///c:/x")).toEqual([]);
    const jira = plugin({
      ...linkGuardManifest(),
      contributes: { linkActions: [{ id: "open", title: "Open", hosts: ["*.atlassian.net"] }] },
    });
    expect(linkActionsFor([jira], "https://acme.atlassian.net/browse/X-1")).toHaveLength(1);
    expect(linkActionsFor([jira], "https://example.org")).toHaveLength(0);
  });

  it("finds composer actions and Markdown blocks and builds fences", () => {
    const plugins = demoExtensionManifests().map((m) => plugin(m));
    expect(composerActionsFor(plugins).map((o) => o.id)).toEqual([
      "snippets.greeting",
      "snippets.shout",
    ]);
    const block = markdownBlockOwner(plugins, " snippets.ticket ");
    expect(block?.pluginName).toBe("Snippets");
    expect(markdownBlockOwner(plugins, "python")).toBeNull();
    expect(blockFence({ id: "a.b", template: "x" })).toBe("```a.b\nx\n```\n");
    expect(blockFence({ id: "a.b", template: "```\ncode\n```" })).toBe(
      "````a.b\n```\ncode\n```\n````\n",
    );
    expect(parseExtensionId("snippets.ticket")).toEqual({ plugin: "snippets", name: "ticket" });
    expect(parseExtensionId("nodot")).toBeNull();
  });

  it("lists every contribution of a manifest with a message id", () => {
    const items = contributionItems(digestManifest());
    expect(items.map((i) => i.kind)).toEqual(["job", "job", "mcpTool"]);
    expect(items[2]).toMatchObject({
      id: "plugin-install-mcp-tool",
      args: { tool: "digest__digest" },
    });
    expect(contributionMessageId("attachmentPreview")).toBe("plugin-install-attachment-preview");
    expect(hookMessageId("beforeSend")).toBe("plugin-hook-before-send");
    for (const m of demoExtensionManifests()) {
      for (const item of contributionItems(m)) expect(runtimeIds()).toContain(item.id);
    }
  });
});

describe("permissions", () => {
  it("knows writes, labels and what needs reading mail", () => {
    expect(isWritePermission("mail.send")).toBe(true);
    expect(isWritePermission("hooks.beforeSend")).toBe(false);
    expect(permissionMessageId("hooks.beforeSend")).toBe("plugin-permission-hooks-before-send");
    expect(permissionMessageId("mcp.expose")).toBe("plugin-permission-mcp-expose");
    expect(requiresRead("hooks.open")).toBe(true);
    expect(NEEDS_READ).toContain("mail.organize");
    // Granting a dependent permission needs mail.read; denying mail.read drops them.
    expect(setGranted([], "hooks.open", true)).toEqual([]);
    const both = setGranted(setGranted([], "mail.read", true), "mail.organize", true);
    expect(both.map((g) => g.permission)).toEqual(["mail.read", "mail.organize"]);
    expect(setGranted(both, "mail.read", false)).toEqual([]);
  });
});

describe("composer edits", () => {
  it("inserts at the cursor, replaces the selection or everything", () => {
    const sel = { start: 6, end: 11 };
    expect(applyComposerEdit("Hello world!", sel, { op: "insert", text: "X" })).toEqual({
      text: "Hello worldX!",
      cursor: 12,
    });
    expect(
      applyComposerEdit("Hello world!", sel, { op: "replace-selection", text: "Ada" }),
    ).toEqual({ text: "Hello Ada!", cursor: 9 });
    expect(
      applyComposerEdit("Hello", { start: 9, end: 2 }, { op: "replace-selection", text: "" }),
    ).toEqual({ text: "He", cursor: 2 });
    expect(applyComposerEdit("Hello", sel, { op: "replace-all", text: "Bye" })).toEqual({
      text: "Bye",
      cursor: 3,
    });
  });
});

describe("activity and previews", () => {
  it("computes the CPU meter", () => {
    expect(cpuPercent(30_000, 120_000)).toBe(25);
    expect(cpuPercent(500_000, 120_000)).toBe(100);
    expect(cpuPercent(1, 0)).toBe(0);
    expect(cpuLevel(10)).toBe("ok");
    expect(cpuLevel(80)).toBe("high");
    expect(cpuLevel(100)).toBe("exhausted");
  });

  it("shows only raster images, never SVG or broken data", () => {
    expect(previewImageSrc({ kind: "image", mimeType: "image/png", data: "iVBORw0K" })).toBe(
      "data:image/png;base64,iVBORw0K",
    );
    expect(
      previewImageSrc({ kind: "image", mimeType: "image/svg+xml", data: "PHN2Zz4=" }),
    ).toBeNull();
    expect(previewImageSrc({ kind: "image", mimeType: "image/png", data: 'a"b' })).toBeNull();
  });
});
