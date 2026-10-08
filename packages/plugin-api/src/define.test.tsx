/** @jsxImportSource @nail/plugin-api */
import { afterEach, describe, expect, it } from "vitest";

import type { PluginActivationContext } from "./api";
import { definePlugin, toPreview } from "./define";
import { banner, previewImage, veto } from "./extensions";
import type { PluginManifest } from "./generated";
import { nail } from "./nail";
import { createTestHost, testMessage, testOutgoingMessage, type TestHost } from "./testing";

const manifest: PluginManifest = {
  id: "kitchen-sink",
  name: "Kitchen sink",
  version: "1.0.0",
  apiVersion: "1",
  main: "dist/index.js",
  permissions: [
    "mail.read",
    "mail.organize",
    "hooks.arrival",
    "hooks.beforeSend",
    "hooks.open",
    "composer",
    "attachments",
    "background",
    "mcp.expose",
    "notifications",
  ],
  contributes: {
    commands: [{ id: "hello", title: "Hello" }],
    panels: [{ id: "side", title: "Side" }],
    hooks: [{ hook: "arrival" }, { hook: "beforeSend" }, { hook: "open" }],
    composerActions: [{ id: "shout", title: "Shout" }],
    markdownBlocks: [{ id: "note", title: "Note" }],
    attachmentPreviews: [{ id: "text", title: "Text", mimeTypes: ["text/plain"] }],
    attachmentActions: [{ id: "count", title: "Count bytes" }],
    linkActions: [{ id: "open", title: "Open" }],
    jobs: [{ id: "tick", schedule: "@hourly" }],
    mcpTools: [{ id: "echo", description: "Echoes" }],
  },
};

const message = testMessage("m1", { subject: "Hello" });

const outgoing = testOutgoingMessage();

const kitchenSink = () =>
  definePlugin({
    commands: { hello: () => nail.ui.notify("hello") },
    panels: { side: () => <text>side</text> },
    hooks: {
      arrival: () => ({ actions: [{ type: "addTag", tag: "seen-by-plugin" }] }),
      beforeSend: (m) => (m.attachments.length === 0 ? veto("No attachment") : null),
      open: () => ({ banners: [banner("hi", "Hello", { buttons: [{ id: "ok", label: "OK" }] })] }),
      onBannerAction: (action) => nail.ui.notify(`${action.bannerId}/${action.buttonId}`),
    },
    composer: {
      actions: {
        shout: (ctx) =>
          nail.composer.replaceSelection((ctx.composer?.selection.text ?? "").toUpperCase()),
      },
      markdownBlocks: {
        note: (input) => ({ html: `<aside>${input.source}</aside>`, text: input.source }),
      },
    },
    attachments: {
      previews: {
        text: (attachment, bytes) => (
          <text>{`${attachment.name}: ${String(bytes.length)} bytes`}</text>
        ),
      },
      actions: { count: (_ctx, bytes) => nail.ui.notify(`${String(bytes?.length ?? 0)} bytes`) },
    },
    links: { open: (ctx) => nail.ui.notify(ctx.link?.url ?? "") },
    jobs: { tick: (trigger) => nail.ui.notify(`tick ${trigger.kind}`) },
    mcpTools: { echo: (args) => ({ echoed: args }) },
  });

describe("definePlugin with declared handlers", () => {
  let host: TestHost;
  let restore = () => {
    /* no-op */
  };
  afterEach(() => {
    restore();
  });

  async function load(options: Parameters<typeof createTestHost>[0] = {}) {
    host = createTestHost({ manifest, messages: [message], ...options });
    restore = host.install();
    await kitchenSink().activate(host.activationContext());
    return host;
  }

  it("registers every handler under its manifest name before activate", async () => {
    let seenInActivate: string[] = [];
    host = createTestHost({ manifest });
    restore = host.install();
    await definePlugin({
      commands: {
        hello: () => {
          /* no-op */
        },
      },
      jobs: {
        tick: () => {
          /* no-op */
        },
      },
      activate() {
        seenInActivate = [...host.commandNames(), ...host.extensionNames().jobs];
      },
    }).activate(host.activationContext());
    expect(seenInActivate).toEqual(["hello", "tick"]);

    await load();
    expect(host.commandNames()).toEqual(["hello"]);
    expect(host.panelNames()).toEqual(["side"]);
    expect(host.hookNames().sort()).toEqual(["arrival", "beforeSend", "open"]);
    expect(host.extensionNames()).toEqual({
      composerActions: ["shout"],
      markdownBlocks: ["note"],
      attachmentPreviews: ["text"],
      attachmentActions: ["count"],
      linkActions: ["open"],
      jobs: ["tick"],
      mcpTools: ["echo"],
    });
  });

  it("runs the hooks", async () => {
    await load();
    const arrival = await host.runArrival(message);
    expect(arrival.outcome).toBe("ok");
    expect(host.messages.get("m1")?.tags).toEqual(["seen-by-plugin"]);
    const send = await host.runBeforeSend(outgoing);
    expect(send).toEqual({
      outcome: "vetoed",
      result: { veto: { reason: "No attachment" } },
      problems: [],
    });
    const withFile = await host.runBeforeSend({
      ...outgoing,
      attachments: [{ name: "a.pdf", mimeType: "application/pdf", size: 3 }],
    });
    expect(withFile).toEqual({ outcome: "ok", problems: [] });
    const open = await host.runOpen(message);
    expect(open.result?.banners?.[0]?.id).toBe("hi");
    await host.clickBanner({ messageId: "m1", bannerId: "hi", buttonId: "ok" });
    expect(host.notifications.at(-1)).toEqual({ title: "hi/ok" });
  });

  it("runs composer actions, blocks, previews, actions, jobs and tools", async () => {
    await load();
    await host.runComposerAction("shout", {
      draftId: "d7",
      selection: { text: "quiet", start: 0, end: 5 },
    });
    expect(host.composerEdits).toEqual([
      { draftId: "d7", edit: { op: "replace-selection", text: "QUIET" } },
    ]);
    expect(await host.renderMarkdownBlock("note", "Mind the gap", "send")).toEqual({
      html: "<aside>Mind the gap</aside>",
      text: "Mind the gap",
    });
    expect(
      await host.renderPreview("text", { name: "a.txt", mimeType: "text/plain" }, "abc"),
    ).toEqual({
      kind: "tree",
      tree: [{ type: "text", value: "a.txt: 3 bytes" }],
    });
    await host.runAttachmentAction("count", { name: "a.bin" }, new Uint8Array(9));
    await host.runLinkAction("open", { url: "https://example.org/x" });
    expect(await host.runJob("tick")).toEqual({ outcome: "ok" });
    expect(host.notifications.map((n) => n.title)).toEqual([
      "9 bytes",
      "https://example.org/x",
      "tick manual",
    ]);
    expect(await host.callMcpTool("echo", { q: 1 })).toEqual({ echoed: { q: 1 } });
  });

  it("explains a host without the M4a registries", async () => {
    host = createTestHost({ manifest });
    // An M2d host: neither ctx nor the global nail has the registries.
    const { hooks, attachments, links, jobs, mcp, ...api } = host.api;
    // The test host has them; the M2d host below does not.
    expect([hooks, attachments, links, jobs, mcp].every((r) => typeof r === "object")).toBe(true);
    const g = globalThis as { nail?: unknown };
    const previous = g.nail;
    g.nail = api;
    restore = () => {
      g.nail = previous;
    };
    const { hooks: ctxHooks, ...rest } = host.activationContext();
    expect(ctxHooks).toBeDefined();
    const old = rest as PluginActivationContext;
    await expect(
      (async () => definePlugin({ hooks: { beforeSend: () => null } }).activate(old))(),
    ).rejects.toMatchObject({ code: "unsupported", message: /ctx\.hooks/ });
    // Plug-ins that do not use them still load.
    await definePlugin({
      commands: {
        hello: () => {
          /* no-op */
        },
      },
    }).activate(old);
    expect(host.commandNames()).toEqual(["hello"]);
  });

  it("normalizes preview answers", () => {
    const image = previewImage("image/png", "iVBO");
    expect(toPreview(image)).toBe(image);
    expect(toPreview(["a", <text>b</text>])).toEqual({
      kind: "tree",
      tree: [
        { type: "text", value: "a" },
        { type: "text", value: "b" },
      ],
    });
  });
});
