/** @jsxImportSource @nail/plugin-api */
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import type { PluginActivationContext, PluginPanelProvider } from "./api";
import { definePlugin, PLUGIN_MARKER } from "./define";
import type { PluginManifest, PluginMessage } from "./generated";
import {
  createPluginApiError,
  isPluginApiError,
  nail,
  toDialogSpec,
  toNotificationSpec,
  toPanelProvider,
} from "./nail";
import { createTestHost, type TestHost } from "./testing";

const message: PluginMessage = {
  id: "m1",
  uri: "nail://messages/m1",
  accountId: "work",
  folderId: "INBOX",
  subject: "Quarterly report",
  from: { name: "Ada", email: "ada@example.org" },
  to: [],
  cc: [],
  date: "2026-09-29T10:00:00Z",
  flags: { seen: false, flagged: false, answered: false, draft: false },
  tags: [],
  hasAttachments: false,
  encrypted: false,
};

const manifest: PluginManifest = {
  id: "follow-up",
  name: "Follow up",
  version: "1.2.3",
  apiVersion: "1",
  main: "dist/index.js",
  permissions: ["mail.read", "ui.dialogs", "notifications", "storage"],
  contributes: {
    commands: [{ id: "hello", title: "Hello" }],
    panels: [{ id: "due", title: "Due" }],
  },
};

describe("nail outside Nail", () => {
  it("fails with a clear unsupported error", async () => {
    expect(() => nail.plugin).toThrow(/must run inside Nail/);
    expect(() => nail.mail.get.name).toThrow(/must run inside Nail/);
    expect(() => nail.ui.notify("x")).toThrow(/must run inside Nail/);
    try {
      await nail.storage.get("k");
      expect.unreachable();
    } catch (error) {
      expect(isPluginApiError(error, "unsupported")).toBe(true);
    }
  });
});

describe("nail with a host", () => {
  let host: TestHost;
  let restore: () => void;
  beforeEach(() => {
    host = createTestHost({
      manifest,
      messages: [message],
      settings: { days: 2 },
      dialog: (spec) => ({ button: spec.buttons?.[0]?.id ?? null, values: { due: "2026-10-01" } }),
    });
    restore = host.install();
  });
  afterEach(() => {
    restore();
  });

  it("forwards to the host and exposes its identity", async () => {
    expect(nail.plugin).toEqual({ id: "follow-up", version: "1.2.3" });
    expect(nail.apiVersion).toBe("1");
    expect(await nail.mail.get("m1")).toEqual(message);
    await nail.storage.set("k", { a: 1 });
    expect(await nail.storage.get("k")).toEqual({ a: 1 });
    expect(nail.settings.get("days")).toBe(2);
    nail.log.info("hi", { n: 1 });
    expect(host.logs).toEqual([{ level: "info", message: 'hi {"n":1}' }]);
  });

  it("accepts JSX dialog bodies and string notifications", async () => {
    const result = await nail.ui.showDialog({
      title: "Follow up",
      body: (
        <>
          <text>Remind me</text>
          <date id="due" label="Due" />
        </>
      ),
      buttons: [{ id: "save", label: "Save", primary: true }],
    });
    expect(result).toEqual({ button: "save", values: { due: "2026-10-01" } });
    expect(host.dialogs[0]?.body).toEqual([
      { type: "text", value: "Remind me" },
      { type: "date", id: "due", label: "Due" },
    ]);
    await nail.ui.notify("Saved");
    expect(host.notifications).toEqual([{ title: "Saved" }]);
  });

  it("passes the host's forbidden errors through", async () => {
    await expect(nail.mail.getBody("m1")).rejects.toMatchObject({ code: "forbidden" });
    try {
      await nail.mail.getBody("m1");
    } catch (error) {
      expect(isPluginApiError(error, "forbidden")).toBe(true);
      expect(isPluginApiError(error, "not_found")).toBe(false);
    }
  });

  it("registers JSX panels and updates them", async () => {
    nail.ui.registerPanel("due", () => <text>Nothing due</text>);
    expect(await host.renderPanel("due")).toEqual([{ type: "text", value: "Nothing due" }]);
    await nail.ui.updatePanel("due", <text>One due</text>);
    expect(host.panelUpdates).toEqual([
      { name: "due", tree: [{ type: "text", value: "One due" }] },
    ]);
  });

  it("registers commands through nail.ui and nail.commands", () => {
    nail.ui.registerCommand("hello", () => {
      /* no-op */
    });
    expect(host.commandNames()).toEqual(["hello"]);
    expect(() => {
      nail.commands.register("other", () => {
        /* no-op */
      });
    }).toThrow(/not in the manifest/);
  });
});

describe("helpers", () => {
  it("normalizes specs and panels", async () => {
    expect(toNotificationSpec("hi")).toEqual({ title: "hi" });
    expect(toNotificationSpec({ title: "t", level: "error" })).toEqual({
      title: "t",
      level: "error",
    });
    expect(toDialogSpec({ title: "T", body: <text>x</text> }).body).toEqual([
      { type: "text", value: "x" },
    ]);
    const onAction = () => {
      /* no-op */
    };
    const provider = toPanelProvider({ render: () => "plain", onAction });
    expect(provider.onAction).toBe(onAction);
    const ctx = { selection: { messageIds: [] } };
    expect(await provider.render(ctx)).toEqual([{ type: "text", value: "plain" }]);
    expect(toPanelProvider(() => <stack />).onAction).toBeUndefined();
  });

  it("builds API errors", () => {
    const error = createPluginApiError("too_large", "quota");
    expect(error).toBeInstanceOf(Error);
    expect(error.name).toBe("NailError");
    expect(error.code).toBe("too_large");
    expect(isPluginApiError(new Error("plain"))).toBe(false);
    expect(isPluginApiError("forbidden")).toBe(false);
  });
});

describe("definePlugin", () => {
  let host: TestHost;
  let restore: () => void;
  beforeEach(() => {
    host = createTestHost({ manifest });
    restore = host.install();
  });
  afterEach(() => {
    restore();
  });

  it("marks the definition and passes the host context through", async () => {
    const seen: string[] = [];
    const plugin = definePlugin({
      activate(ctx) {
        seen.push(ctx.plugin.id);
        ctx.commands.register("hello", () => {
          seen.push("hello");
        });
        ctx.panels.register("due", { render: () => <text>side</text> });
      },
    });
    expect(plugin.kind).toBe(PLUGIN_MARKER);
    expect(plugin.apiVersion).toBe("1");
    expect(plugin).not.toHaveProperty("deactivate");
    await plugin.activate(host.activationContext());
    expect(host.commandNames()).toEqual(["hello"]);
    expect(await host.renderPanel("due")).toEqual([{ type: "text", value: "side" }]);
    await host.runCommand("hello");
    expect(seen).toEqual(["follow-up", "hello"]);
  });

  it("uses the global API when the host passes no context", async () => {
    let id = "";
    const plugin = definePlugin({
      activate(ctx) {
        id = ctx.plugin.id;
        ctx.panels.register("due", () => <text>global</text>);
        expect(ctx.settings.all()).toEqual({});
      },
    });
    await (plugin.activate as (ctx?: PluginActivationContext) => Promise<void>)();
    expect(id).toBe("follow-up");
    expect(await host.renderPanel("due")).toEqual([{ type: "text", value: "global" }]);
  });

  it("gives the host normalized panel providers", async () => {
    const registered: PluginPanelProvider[] = [];
    const ctx: PluginActivationContext = {
      ...host.activationContext(),
      panels: { register: (_name, panel) => void registered.push(panel as PluginPanelProvider) },
    };
    await definePlugin({
      activate(c) {
        c.panels.register("due", () => ["a", <text>b</text>]);
      },
    }).activate(ctx);
    expect(await registered[0]?.render({ selection: { messageIds: [] } })).toEqual([
      { type: "text", value: "a" },
      { type: "text", value: "b" },
    ]);
  });

  it("passes deactivate on", () => {
    let stopped = false;
    const plugin = definePlugin({
      activate() {
        /* no-op */
      },
      deactivate() {
        stopped = true;
      },
    });
    plugin.deactivate?.();
    expect(stopped).toBe(true);
  });
});
