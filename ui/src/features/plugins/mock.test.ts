import { describe, expect, it } from "vitest";
import type { Event, PluginInfo } from "../../api/generated";
import { MockTransport } from "../../api/mock";
import { PLUGINS_REQUESTS, acceptsSetting, defaultSettingValue, isPluginsRequest } from "./mock";

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

describe("plugins mock", () => {
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
      plugin: { enabled: true, state: "stopped" },
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

  it("answers host requests with unsupported", async () => {
    const { t } = transport();
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
