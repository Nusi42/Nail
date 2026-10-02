import { describe, expect, it } from "vitest";
import type { PluginInfo } from "../../api/generated";
import { MockTransport } from "../../api/mock";
import { PLUGIN_EXTENSION_REQUESTS, isPluginExtensionRequest } from "./extensionsMock";

function digest(): PluginInfo {
  return {
    id: "digest",
    manifest: {
      id: "digest",
      name: "Digest",
      version: "1.0.0",
      apiVersion: "1",
      main: "index.js",
      permissions: ["background"],
      contributes: { jobs: [{ id: "daily", schedule: "0 8 * * *" }] },
    },
    source: { kind: "dev", folder: "/dev/digest" },
    enabled: true,
    state: "running",
    grants: [{ permission: "background", accounts: null }],
    storage: { keys: 0, used_bytes: 0, quota_bytes: 5_242_880 },
    package_sha256: null,
    installed_at: "2026-10-02T08:00:00Z",
    updated_at: "2026-10-02T08:00:00Z",
    error: null,
  };
}

describe("plug-in extension mock", () => {
  it("routes exactly its requests", () => {
    expect(new Set(PLUGIN_EXTENSION_REQUESTS).size).toBe(PLUGIN_EXTENSION_REQUESTS.length);
    expect(isPluginExtensionRequest({ type: "OverrideSendVeto", veto_id: "v1" })).toBe(true);
    expect(isPluginExtensionRequest({ type: "ListPlugins" })).toBe(false);
  });

  it("answers like the core at the contract stage", async () => {
    const t = new MockTransport();
    t.data.features.plugins.plugins.push(digest());
    const activity = await t.send({ type: "GetPluginActivity", plugin_id: "digest" });
    expect(activity).toMatchObject({
      type: "PluginActivity",
      activity: { jobs: [{ job: "daily", schedule: "0 8 * * *", run_count: 0 }], hooks: [] },
    });
    expect(await t.send({ type: "GetPluginActivity", plugin_id: "ghost" })).toMatchObject({
      code: "not_found",
    });
    expect(
      await t.send({ type: "GetMessagePluginAnnotations", message_id: "m1", client_id: null }),
    ).toEqual({ type: "MessagePluginAnnotations", message_id: "m1", banners: [], badges: [] });
    expect(await t.send({ type: "RunPluginJob", plugin_id: "digest", job: "daily" })).toMatchObject(
      { code: "unsupported" },
    );
    expect(await t.send({ type: "OverrideSendVeto", veto_id: "v1" })).toMatchObject({
      code: "not_found",
    });
  });
});
