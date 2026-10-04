import { describe, expect, it } from "vitest";
import type { Event, PluginGrant, PluginInfo, Response } from "../../api/generated";
import { MockTransport } from "../../api/mock";
import { PLUGINS_REQUESTS, acceptsSetting, defaultSettingValue, isPluginsRequest } from "./mock";
import { crashPlugin, followUpManifest, sourceKey } from "./mockHost";
import { must } from "./testUtil";
import { stringContaining } from "../../test/matchers";

function followUp(): PluginInfo {
  return {
    id: "follow-up",
    manifest: {
      id: "follow-up",
      name: "Follow-up",
      version: "1.0.0",
      apiVersion: "1",
      main: "dist/index.js",
      permissions: ["mail.read", "storage"],
      contributes: {
        settings: [{ type: "number", key: "days", title: "Days", default: 1, min: 0 }],
      },
    },
    source: { kind: "dev", folder: "C:/dev/follow-up" },
    enabled: false,
    state: "disabled",
    grants: [],
    storage: { keys: 0, used_bytes: 0, quota_bytes: 5242880 },
    package_sha256: null,
    installed_at: "2026-09-30T00:00:00Z",
    updated_at: "2026-09-30T00:00:00Z",
    error: null,
  };
}

function transport(): { t: MockTransport; events: Event[] } {
  const t = new MockTransport();
  t.data.features.plugins.plugins.push(followUp());
  const events: Event[] = [];
  t.onEvent((e) => events.push(e));
  return { t, events };
}

const FILE = { kind: "file", path: "C:/plugins/follow-up.nailplugin" } as const;
const ALL: PluginGrant[] = must(followUpManifest().permissions).map((permission) => ({
  permission,
  accounts: null,
}));

/** A transport with the example plug-in installed from its package. */
async function installed(grants: PluginGrant[] = ALL) {
  const t = new MockTransport();
  const events: Event[] = [];
  t.onEvent((e) => events.push(e));
  const preview = await t.send({ type: "PreviewPluginInstall", source: FILE });
  if (preview.type !== "PluginInstallPreview") throw new Error("no preview");
  const r = await t.send({
    type: "InstallPlugin",
    source: FILE,
    package_sha256: preview.preview.package_sha256,
    grants,
    enable: true,
  });
  return { t, events, r };
}

const settle = () => new Promise((resolve) => setTimeout(resolve, 10));

function ofType<T extends Event["type"]>(events: Event[], type: T): Extract<Event, { type: T }>[] {
  return events.filter((e): e is Extract<Event, { type: T }> => e.type === type);
}

describe("plugins mock: administration", () => {
  it("routes exactly its requests", () => {
    expect(new Set(PLUGINS_REQUESTS).size).toBe(PLUGINS_REQUESTS.length);
    expect(isPluginsRequest({ type: "ListPlugins" })).toBe(true);
    expect(isPluginsRequest({ type: "Ping" })).toBe(false);
  });

  it("lists, enables and uninstalls like the core", async () => {
    const { t, events } = transport();
    expect(await t.send({ type: "ListPlugins" })).toMatchObject({
      type: "Plugins",
      plugins: [{ id: "follow-up", state: "disabled" }],
    });
    expect(await t.send({ type: "EnablePlugin", plugin_id: "follow-up" })).toMatchObject({
      type: "Plugin",
      plugin: { enabled: true, state: "running" },
    });
    expect(events).toContainEqual({ type: "PluginsChanged" });
    expect(await t.send({ type: "UninstallPlugin", plugin_id: "follow-up" })).toEqual({
      type: "Done",
    });
    expect(await t.send({ type: "GetPlugin", plugin_id: "follow-up" })).toMatchObject({
      type: "Error",
      code: "not_found",
    });
  });

  it("reports enabled plug-ins as stopped without a host", async () => {
    const { t } = transport();
    t.data.features.plugins.host = null;
    expect(await t.send({ type: "EnablePlugin", plugin_id: "follow-up" })).toMatchObject({
      plugin: { state: "stopped" },
    });
  });

  it("accepts only declared grants", async () => {
    const { t } = transport();
    expect(
      await t.send({
        type: "UpdatePluginGrants",
        plugin_id: "follow-up",
        grants: [{ permission: "mail.read", accounts: ["acc-work"] }],
      }),
    ).toMatchObject({ type: "Plugin", plugin: { grants: [{ permission: "mail.read" }] } });
    expect(
      await t.send({
        type: "UpdatePluginGrants",
        plugin_id: "follow-up",
        grants: [{ permission: "notifications", accounts: null }],
      }),
    ).toMatchObject({ type: "Error", code: "invalid_request" });
    expect(
      await t.send({
        type: "UpdatePluginGrants",
        plugin_id: "follow-up",
        grants: [{ permission: "storage", accounts: ["acc-work"] }],
      }),
    ).toMatchObject({ type: "Error", code: "invalid_request" });
  });

  it("validates settings", async () => {
    const { t } = transport();
    expect(await t.send({ type: "GetPluginSettings", plugin_id: "follow-up" })).toMatchObject({
      type: "PluginSettings",
      values: { days: 1 },
    });
    expect(
      await t.send({ type: "UpdatePluginSettings", plugin_id: "follow-up", values: { days: 4 } }),
    ).toMatchObject({ values: { days: 4 } });
    expect(
      await t.send({
        type: "UpdatePluginSettings",
        plugin_id: "follow-up",
        values: { days: -1, nope: true },
      }),
    ).toMatchObject({
      type: "Error",
      code: "invalid_request",
      localized: { id: "plugin-settings-invalid", args: { keys: "days, nope" } },
    });
  });

  it("answers host requests with unsupported without a host", async () => {
    const { t } = transport();
    t.data.features.plugins.host = null;
    expect(await t.send({ type: "PreviewPluginPlacements", plugin_id: "follow-up" })).toMatchObject(
      { type: "Error", code: "unsupported" },
    );
    expect(
      await t.send({
        type: "RunPluginCommand",
        command: "plugin.follow-up.remind",
        surface: "reader",
        context: t.data.uiContext,
        client_id: null,
      }),
    ).toMatchObject({ type: "Error", code: "unsupported" });
  });

  it("computes setting defaults and checks values like the core", () => {
    expect(defaultSettingValue({ type: "number", key: "n", title: "N", min: 5 })).toBe(5);
    expect(defaultSettingValue({ type: "boolean", key: "b", title: "B" })).toBe(false);
    expect(
      defaultSettingValue({
        type: "select",
        key: "s",
        title: "S",
        options: [{ value: "a", label: "A" }],
      }),
    ).toBe("a");
    expect(acceptsSetting({ type: "string", key: "s", title: "S", maxLength: 2 }, "abc")).toBe(
      false,
    );
    expect(acceptsSetting({ type: "number", key: "n", title: "N", max: 3 }, 2.5)).toBe(false);
  });
});

describe("plugins mock: host", () => {
  it("keys packages by source", () => {
    expect(sourceKey(FILE)).toBe("file:C:/plugins/follow-up.nailplugin");
    expect(sourceKey({ kind: "url", url: "https://x" })).toBe("url:https://x");
    expect(sourceKey({ kind: "dev", folder: "D:/p" })).toBe("dev:D:/p");
  });

  it("previews and installs exactly the previewed package", async () => {
    const t = new MockTransport();
    const preview = await t.send({ type: "PreviewPluginInstall", source: FILE });
    expect(preview).toMatchObject({
      type: "PluginInstallPreview",
      preview: { manifest: { id: "follow-up" }, issues: [], installed_version: null },
    });
    expect(
      await t.send({
        type: "InstallPlugin",
        source: FILE,
        package_sha256: "0".repeat(64),
        grants: [],
        enable: true,
      }),
    ).toMatchObject({ type: "Error", code: "conflict" });
    const { r, events } = await installed();
    expect(r).toMatchObject({
      type: "Plugin",
      plugin: { id: "follow-up", enabled: true, state: "running", grants: ALL },
    });
    expect(events).toContainEqual({ type: "PluginsChanged" });
    expect(ofType(events, "PluginLogLine")[0]?.entry).toMatchObject({ source: "host" });
  });

  it("reports an update and a broken package", async () => {
    const { t } = await installed();
    expect(await t.send({ type: "PreviewPluginInstall", source: FILE })).toMatchObject({
      preview: { installed_version: "1.0.0" },
    });
    expect(
      await t.send({
        type: "PreviewPluginInstall",
        source: { kind: "file", path: "C:/plugins/broken.nailplugin" },
      }),
    ).toMatchObject({ preview: { manifest: null, issues: [{ kind: "invalid-json" }] } });
    expect(
      await t.send({ type: "PreviewPluginInstall", source: { kind: "file", path: "C:/nope" } }),
    ).toMatchObject({ type: "Error", code: "not_found" });
  });

  it("refuses undeclared grants on install", async () => {
    const { r, t } = await installed([{ permission: "network:x.example", accounts: null }]);
    expect(r).toMatchObject({ type: "Error", code: "invalid_request" });
    expect(await t.send({ type: "ListPlugins" })).toMatchObject({ plugins: [] });
  });

  it("loads development folders", async () => {
    const t = new MockTransport();
    expect(
      await t.send({ type: "LoadDevPlugin", folder: "C:/dev/follow-up", grants: ALL }),
    ).toMatchObject({
      type: "Plugin",
      plugin: { source: { kind: "dev" }, package_sha256: null, enabled: true },
    });
  });

  it("runs a command: dialog, storage, notification and panel", async () => {
    const { t, events } = await installed();
    const message = must(t.data.messages[0]);
    const context = { ...t.data.uiContext, message_id: message.id };
    expect(
      await t.send({
        type: "RunPluginCommand",
        command: "plugin.follow-up.remind",
        surface: "reader",
        context,
        client_id: "ui-1",
      }),
    ).toEqual({ type: "Done" });
    await settle();
    const opened = must(ofType(events, "PluginDialogOpened")[0]).dialog;
    expect(opened).toMatchObject({ plugin_id: "follow-up", client_id: "ui-1" });
    expect(opened.spec.body[0]).toMatchObject({
      type: "text",
      value: stringContaining(message.subject),
    });
    expect(
      await t.send({
        type: "RespondPluginDialog",
        dialog_id: opened.dialog_id,
        button: "ok",
        values: { due: "2026-10-01", note: "" },
      }),
    ).toEqual({ type: "Done" });
    await settle();
    expect(ofType(events, "PluginDialogClosed")).toHaveLength(1);
    expect(must(ofType(events, "PluginNotification")[0]).notification).toMatchObject({
      plugin_id: "follow-up",
      client_id: "ui-1",
      spec: { title: "Reminder saved", level: "success" },
    });
    const changed = must(ofType(events, "PluginPanelChanged")[0]);
    expect(changed.panel).toBe("follow-up.reminders");
    expect(changed.tree[0]).toMatchObject({ type: "table", rows: [[message.id, "2026-10-01"]] });
    const panel = (await t.send({
      type: "GetPluginPanel",
      panel: "follow-up.reminders",
      context,
      client_id: null,
    })) as Extract<Response, { type: "PluginPanel" }>;
    expect(panel.tree[0]).toMatchObject({ type: "table" });
    await t.send({
      type: "PluginUiAction",
      target: { kind: "panel", panel: "follow-up.reminders" },
      component_id: "clear",
      values: {},
      client_id: null,
    });
    await settle();
    expect(ofType(events, "PluginPanelChanged").at(-1)?.tree).toEqual([
      { type: "text", value: "No reminders.", style: "muted" },
    ]);
  });

  it("gates API calls by the grants and logs refusals", async () => {
    const { t, events } = await installed([{ permission: "mail.read", accounts: null }]);
    await t.send({
      type: "RunPluginCommand",
      command: "plugin.follow-up.remind",
      surface: "reader",
      context: { ...t.data.uiContext, message_id: must(t.data.messages[0]).id },
      client_id: null,
    });
    await settle();
    expect(ofType(events, "PluginDialogOpened")).toHaveLength(0);
    expect(ofType(events, "PluginLogLine").map((e) => e.entry.message)).toContain(
      "permission denied: ui.dialogs",
    );
  });

  it("refuses commands of plug-ins that do not run", async () => {
    const { t } = await installed();
    await t.send({ type: "DisablePlugin", plugin_id: "follow-up" });
    const run = (command: string) =>
      t.send({
        type: "RunPluginCommand",
        command,
        surface: "global",
        context: t.data.uiContext,
        client_id: null,
      });
    expect(await run("plugin.follow-up.remind")).toMatchObject({ code: "unavailable" });
    expect(await run("plugin.nope.x")).toMatchObject({ code: "not_found" });
  });

  it("closes a crashed plug-in's dialogs and says why it stopped", async () => {
    const { t, events } = await installed();
    await t.send({
      type: "RunPluginCommand",
      command: "plugin.follow-up.remind",
      surface: "reader",
      context: { ...t.data.uiContext, message_id: must(t.data.messages[0]).id },
      client_id: null,
    });
    await settle();
    crashPlugin(t, "follow-up", "cpu-budget");
    expect(ofType(events, "PluginDialogClosed")).toHaveLength(1);
    expect(ofType(events, "PluginStopped")[0]).toMatchObject({
      reason: "cpu-budget",
      localized: { id: "plugin-stopped-cpu-budget" },
    });
    expect(await t.send({ type: "GetPlugin", plugin_id: "follow-up" })).toMatchObject({
      plugin: { state: "crashed" },
    });
  });

  it("proposes placements that the merge flow applies", async () => {
    const { t } = await installed();
    const r = await t.send({ type: "PreviewPluginPlacements", plugin_id: "follow-up" });
    if (r.type !== "PluginPlacements") throw new Error(JSON.stringify(r));
    expect(r.incoming).toContain('item "plugin.follow-up.remind"');
    const keys = r.preview.sections.find((s) => s.section === "keys");
    expect(keys?.items[0]).toMatchObject({ change: "added", key: "list: shift+r" });
    const applied = await t.send({
      type: "ApplyConfigMerge",
      file: "layout",
      incoming: r.incoming,
      choices: [{ item_id: must(must(keys).items[0]).id, take: "theirs" }],
      default: "mine",
    });
    expect(applied).toMatchObject({ type: "ConfigMergeApplied" });
    expect(t.data.layout.keys).toContainEqual(
      expect.objectContaining({ key: "shift+r", action: "plugin.follow-up.remind" }),
    );
  });
});
