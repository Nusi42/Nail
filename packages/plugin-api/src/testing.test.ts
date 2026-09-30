import { afterEach, describe, expect, it } from "vitest";

import type { PluginMessage } from "./generated";
import { createTestHost } from "./testing";
import { checkTree, MAX_UI_DEPTH, MAX_UI_NODES } from "./tree";
import type { PluginUiNode } from "./generated";

const base: Omit<PluginMessage, "id" | "uri" | "accountId" | "date" | "encrypted"> = {
  folderId: "INBOX",
  subject: "s",
  to: [],
  cc: [],
  flags: { seen: false, flagged: false, answered: false, draft: false },
  tags: [],
  hasAttachments: false,
};
const msg = (id: string, accountId: string, date: string, encrypted = false): PluginMessage => ({
  ...base,
  id,
  uri: `nail://messages/${id}`,
  accountId,
  date,
  encrypted,
});

describe("test host permissions", () => {
  it("rejects every capability without its permission", async () => {
    const host = createTestHost({ permissions: [], messages: [msg("m1", "work", "2026-01-01")] });
    const { api } = host;
    const calls: (() => Promise<unknown>)[] = [
      () => api.mail.get("m1"),
      () => api.mail.getBody("m1"),
      () => api.mail.search("x"),
      () => api.mail.thread("t1"),
      () => api.ui.showDialog({ title: "t", body: [] }),
      () => api.ui.notify("n"),
      () => api.storage.get("k"),
      () => api.storage.set("k", 1),
      () => api.storage.keys(),
      () => api.storage.usage(),
      () => api.http.fetch("https://api.example.com/x"),
    ];
    for (const call of calls) {
      await expect(call()).rejects.toMatchObject({ code: "forbidden" });
    }
    // Commands, panels, settings and the log need no permission.
    api.commands.register("c", () => {});
    api.ui.registerPanel("p", { render: () => [] });
    api.log.info("ok");
    expect(api.settings.all()).toEqual({});
  });

  it("never gives bodies of end-to-end encrypted mail", async () => {
    const host = createTestHost({
      messages: [msg("plain", "work", "2026-01-01"), msg("secret", "work", "2026-01-02", true)],
      bodies: {
        plain: { id: "plain", text: "hello", attachments: [] },
        secret: { id: "secret", text: "decrypted", attachments: [] },
      },
    });
    expect((await host.api.mail.getBody("plain")).text).toBe("hello");
    await expect(host.api.mail.getBody("secret")).rejects.toMatchObject({ code: "forbidden" });
    await expect(host.api.mail.get("missing")).rejects.toMatchObject({ code: "not_found" });
  });

  it("searches mail newest first and lists threads oldest first", async () => {
    const host = createTestHost({
      messages: [
        { ...msg("a", "work", "2026-01-01"), subject: "Report draft", threadId: "t1" },
        { ...msg("b", "home", "2026-01-03"), subject: "Holiday" },
        { ...msg("c", "work", "2026-01-02"), subject: "Re: report draft", threadId: "t1" },
      ],
    });
    expect((await host.api.mail.search("")).map((m) => m.id)).toEqual(["b", "c", "a"]);
    expect((await host.api.mail.search("report", { limit: 1 })).map((m) => m.id)).toEqual(["c"]);
    expect((await host.api.mail.thread("t1")).map((m) => m.id)).toEqual(["a", "c"]);
  });

  it("searches message text only with mail.read.body", async () => {
    const messages = [{ ...msg("a", "work", "2026-01-01"), subject: "Report" }];
    const headers = createTestHost({ permissions: ["mail.read"], messages });
    expect((await headers.api.mail.search("report")).map((m) => m.id)).toEqual(["a"]);
    await expect(headers.api.mail.search("body:otp")).rejects.toMatchObject({ code: "forbidden" });
    await expect(headers.api.mail.search("x -filename:pdf")).rejects.toMatchObject({
      code: "forbidden",
    });
    const bodies = createTestHost({ permissions: ["mail.read", "mail.read.body"], messages });
    await expect(bodies.api.mail.search("body:otp")).resolves.toEqual([]);
  });

  it("allows network access only to declared https hosts", async () => {
    const host = createTestHost({
      permissions: ["network:api.example.com", "network:*.example.org"],
      fetch: (url) => ({ body: JSON.stringify({ url }) }),
    });
    const res = await host.api.http.fetch("https://api.example.com/v1");
    expect(res.ok).toBe(true);
    expect(await res.json()).toEqual({ url: "https://api.example.com/v1" });
    expect((await host.api.http.fetch("https://a.b.example.org/")).status).toBe(200);
    for (const url of ["https://example.org/", "https://x.api.example.com/", "https://evil.com/"]) {
      await expect(host.api.http.fetch(url)).rejects.toMatchObject({ code: "forbidden" });
    }
    await expect(host.api.http.fetch("http://api.example.com/")).rejects.toMatchObject({
      code: "invalid_request",
    });
    await expect(host.api.http.fetch("not a url")).rejects.toMatchObject({
      code: "invalid_request",
    });
  });
});

describe("test host storage", () => {
  it("stores copies, lists keys and enforces the quota", async () => {
    const host = createTestHost({ storageQuota: 40 });
    const value = { n: 1 };
    await host.api.storage.set("r:1", value);
    value.n = 2;
    expect(await host.api.storage.get("r:1")).toEqual({ n: 1 });
    await host.api.storage.set("other", true);
    expect(await host.api.storage.keys("r:")).toEqual(["r:1"]);
    expect(await host.api.storage.keys()).toEqual(["other", "r:1"]);
    await expect(host.api.storage.set("big", "x".repeat(100))).rejects.toMatchObject({
      code: "too_large",
    });
    expect(await host.api.storage.get("big")).toBeNull();
    const usage = await host.api.storage.usage();
    expect(usage).toMatchObject({ keys: 2, quotaBytes: 40 });
    expect(usage.usedBytes).toBeGreaterThan(0);
    expect(await host.api.storage.delete("other")).toBe(true);
    expect(await host.api.storage.delete("other")).toBe(false);
    await host.api.storage.clear();
    expect(await host.api.storage.keys()).toEqual([]);
  });
});

describe("test host registrations", () => {
  let restore: (() => void) | undefined;
  afterEach(() => restore?.());

  it("refuses double and undeclared registrations", () => {
    const host = createTestHost({
      manifest: {
        id: "p",
        name: "P",
        version: "1.0.0",
        apiVersion: "1",
        main: "index.js",
        contributes: { commands: [{ id: "x", title: "X" }], panels: [{ id: "side", title: "S" }] },
      },
    });
    host.api.commands.register("x", () => {});
    expect(() => host.api.ui.registerCommand("x", () => {})).toThrow(/is registered/);
    expect(() => host.api.commands.register("y", () => {})).toThrow(/not in the manifest/);
    host.api.ui.registerPanel("side", () => []);
    expect(() => host.api.ui.registerPanel("other", () => [])).toThrow(/not in the manifest/);
    expect(host.commandNames()).toEqual(["x"]);
    expect(host.panelNames()).toEqual(["side"]);
    expect(host.api.plugin).toEqual({ id: "p", version: "1.0.0" });
  });

  it("delivers setting changes", () => {
    const host = createTestHost({ settings: { days: 1 } });
    const seen: unknown[] = [];
    host.api.settings.onChange((values) => seen.push(values));
    host.changeSettings({ days: 3 });
    expect(host.api.settings.get("days")).toBe(3);
    expect(host.api.settings.get("missing")).toBeUndefined();
    expect(seen).toEqual([{ days: 3 }]);
  });

  it("runs commands with a full context and routes panel clicks", async () => {
    const host = createTestHost();
    const seen: unknown[] = [];
    host.api.commands.register("c", (ctx) => {
      seen.push(ctx);
    });
    await host.runCommand("c", { surface: "reader", selection: { messageIds: ["m1"] } });
    expect(seen[0]).toEqual({ surface: "reader", selection: { messageIds: ["m1"] } });
    await host.runCommand("c");
    expect(seen[1]).toEqual({ surface: "global", selection: { messageIds: [] } });
    await expect(host.runCommand("missing")).rejects.toMatchObject({ code: "not_found" });

    host.api.ui.registerPanel("p", {
      render: () => [{ type: "button", id: "go", label: "Go" }],
      onAction: (action) => {
        seen.push(action);
      },
    });
    expect(await host.renderPanel("p")).toHaveLength(1);
    await host.clickPanelButton("p", "go", { x: 1 });
    expect(seen[2]).toEqual({ componentId: "go", values: { x: 1 } });
    await host.api.ui.updatePanel("p", [{ type: "text", value: "later" }]);
    expect(host.panelUpdates).toEqual([{ name: "p", tree: [{ type: "text", value: "later" }] }]);
    await expect(host.api.ui.updatePanel("nope", [])).rejects.toMatchObject({ code: "not_found" });
    await expect(host.renderPanel("nope")).rejects.toMatchObject({ code: "not_found" });
  });

  it("checks notifications like the host", async () => {
    const host = createTestHost();
    for (const bad of ["", "   ", "x".repeat(201)]) {
      await expect(host.api.ui.notify(bad)).rejects.toMatchObject({ code: "invalid_request" });
    }
    await expect(host.api.ui.notify({ title: "t", body: "x".repeat(2001) })).rejects.toMatchObject({
      code: "invalid_request",
    });
    await host.api.ui.notify({ title: "t", body: "b", level: "warning" });
    expect(host.notifications).toEqual([{ title: "t", body: "b", level: "warning" }]);
  });

  it("checks dialog trees like the host", async () => {
    const host = createTestHost();
    await expect(
      host.api.ui.showDialog({
        title: "t",
        body: [
          { type: "input", id: "a", label: "A" },
          { type: "input", id: "a", label: "B" },
        ],
      }),
    ).rejects.toMatchObject({ code: "invalid_request" });
    expect(await host.api.ui.showDialog({ title: "t", body: [] })).toEqual({
      button: null,
      values: {},
    });
  });

  it("installs and restores the global", () => {
    const g = globalThis as { nail?: unknown };
    expect(g.nail).toBeUndefined();
    const host = createTestHost();
    restore = host.install();
    expect(g.nail).toBe(host.api);
    restore();
    restore = undefined;
    expect(g.nail).toBeUndefined();
  });
});

describe("checkTree", () => {
  it("accepts a well-formed tree", () => {
    const tree: PluginUiNode[] = [
      { type: "text", value: "hi" },
      {
        type: "stack",
        children: [
          { type: "date", id: "due", label: "Due" },
          { type: "image", src: "data:image/png;base64,AA==", alt: "" },
          { type: "image", src: "assets/logo.png", alt: "Logo" },
        ],
      },
    ];
    expect(checkTree(tree)).toEqual([]);
  });

  it("reports the common mistakes with paths", () => {
    const tree = [
      { type: "input", id: "", label: "No id" },
      { type: "checkbox", id: "x", label: "X" },
      { type: "checkbox", id: "x", label: "Again" },
      {
        type: "select",
        id: "s",
        label: "S",
        options: [{ value: "a", label: "A" }],
        default: ["b"],
      },
      { type: "table", columns: [{ label: "A" }], rows: [["1", "2"]], selectable: true },
      { type: "image", src: "https://example.com/x.png", alt: "" },
      { type: "image", src: "data:image/svg+xml;base64,AA==", alt: "" },
      { type: "progress", value: 120 },
    ] as PluginUiNode[];
    expect(checkTree(tree).map((i) => `${i.path}: ${i.message}`)).toEqual([
      "body[0]: <input> needs an id",
      'body[2]: duplicate id "x"',
      'body[3]: default "b" is not an option',
      "body[4]: a selectable <table> needs an id",
      "body[4].rows[0]: row length differs from columns",
      "body[5]: image src must be a PNG, JPEG, GIF or WebP data: URI",
      "body[6]: image src must be a PNG, JPEG, GIF or WebP data: URI",
      "body[7]: progress value must be 0–100",
    ]);
  });

  it("limits depth and size", () => {
    let deep: PluginUiNode = { type: "text", value: "x" };
    for (let i = 0; i < MAX_UI_DEPTH; i += 1) deep = { type: "stack", children: [deep] };
    expect(checkTree([deep]).map((i) => i.message)).toEqual([`nested deeper than ${MAX_UI_DEPTH}`]);
    const many: PluginUiNode[] = Array.from({ length: MAX_UI_NODES + 1 }, () => ({
      type: "text",
      value: "x",
    }));
    expect(checkTree(many).map((i) => i.message)).toEqual([`more than ${MAX_UI_NODES} nodes`]);
  });
});
