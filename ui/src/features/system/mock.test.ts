import { QueryClient } from "@tanstack/react-query";
import { describe, expect, it, vi } from "vitest";
import type { Event as NailEvent, UpdateStatus } from "../../api/generated";
import { MockTransport } from "../../api/mock";
import { appRegistry } from "../registry";
import type { FeatureEventContext } from "../types";
import { SYSTEM_EVENTS, handleSystemEvent, setUpdateState, systemKeys } from ".";
import { runtimeIds } from "./messages";
import { SYSTEM_REQUESTS, isSystemRequest } from "./mock";
import { DEFAULT_APP_TARGETS, MAPI_RESULTS, canCheck, canDownload, canInstall } from "./model";

const release = {
  version: "1.1.0",
  channel: "stable",
  published_at: null,
  notes: "Faster search",
  size_bytes: null,
} as const;

describe("system model", () => {
  it("mirrors the contract", () => {
    expect(DEFAULT_APP_TARGETS).toEqual(["mailto", "webcal", "webcals", "eml", "ics", "vcf"]);
    expect(MAPI_RESULTS).toHaveLength(16);
    expect(canCheck({ state: "idle" })).toBe(true);
    expect(canCheck({ state: "checking" })).toBe(false);
    expect(canDownload({ state: "available", update: release })).toBe(true);
    expect(canInstall({ state: "ready-to-install", update: release })).toBe(true);
    expect(canCheck({ state: "disabled", reason: "policy" })).toBe(false);
    expect(runtimeIds()).toContain("update-disabled-msi-managed");
    expect(runtimeIds()).toContain("mapi-result-bad-recip-type");
  });
});

describe("system mock", () => {
  it("routes exactly its requests", () => {
    expect(new Set(SYSTEM_REQUESTS).size).toBe(SYSTEM_REQUESTS.length);
    expect(isSystemRequest({ type: "CheckForUpdates" })).toBe(true);
    expect(isSystemRequest({ type: "ListMcpClients" })).toBe(false);
  });

  it("runs the updater like the core with a desktop host", async () => {
    const t = new MockTransport();
    const seen: NailEvent[] = [];
    t.onEvent((e) => seen.push(e));
    expect(await t.send({ type: "GetUpdateStatus" })).toMatchObject({
      status: { current_version: "1.0.0", state: { state: "idle" } },
    });
    expect(await t.send({ type: "DownloadUpdate" })).toMatchObject({ code: "conflict" });
    expect(await t.send({ type: "CheckForUpdates" })).toMatchObject({
      status: { state: { state: "checking" } },
    });
    setUpdateState(t, { state: "ready-to-install", update: release });
    expect(seen.at(-1)).toMatchObject({
      type: "UpdateStatusChanged",
      status: { state: { state: "ready-to-install" } },
    });
    expect(await t.send({ type: "InstallUpdate" })).toEqual({ type: "Done" });
    expect(
      await t.send({
        type: "SetUpdateSettings",
        settings: { channel: "beta", auto_check: true, auto_download: false },
      }),
    ).toMatchObject({ status: { settings: { channel: "beta", auto_download: false } } });
    expect(t.data.features.system.calls).toEqual(["check", "install"]);
  });

  it("lets the policy win", async () => {
    const t = new MockTransport();
    Object.assign(t.data.features.system.installation, {
      kind: "msi",
      scope: "per-machine",
      updates_disabled: "msi-managed",
      forced_channel: "stable",
    });
    expect(await t.send({ type: "GetUpdateStatus" })).toMatchObject({
      status: { state: { state: "disabled", reason: "msi-managed" }, forced_channel: "stable" },
    });
    expect(await t.send({ type: "CheckForUpdates" })).toMatchObject({
      code: "forbidden",
      localized: { id: "update-disabled-msi-managed" },
    });
    expect(
      await t.send({
        type: "SetUpdateSettings",
        settings: { channel: "beta", auto_check: true, auto_download: true },
      }),
    ).toMatchObject({ code: "forbidden", localized: { id: "update-channel-locked" } });
    expect(await t.send({ type: "GetDefaultApps" })).toMatchObject({
      settings_uri: "ms-settings:defaultapps?registeredAppMachine=Nail",
    });
  });

  it("stores desktop settings and the first run; nail serve has no desktop", async () => {
    const t = new MockTransport();
    const settings = {
      close_to_tray: false,
      start_hidden: true,
      start_with_windows: true,
      plugin_dev_endpoint: true,
    };
    const saved = await t.send({ type: "SetDesktopSettings", settings });
    expect(saved).toMatchObject({ settings, dev_endpoint_url: "http://127.0.0.1:47811/" });
    expect(await t.send({ type: "GetFirstRun" })).toMatchObject({ state: { completed: false } });
    expect(await t.send({ type: "CompleteFirstRun" })).toMatchObject({
      state: { completed: true, completed_version: "1.0.0" },
    });
    t.data.features.system.desktop = false;
    expect(await t.send({ type: "GetInstallation" })).toMatchObject({
      code: "unsupported",
      localized: { id: "desktop-unavailable" },
    });
    expect(await t.send({ type: "GetFirstRun" })).toMatchObject({ state: { completed: true } });
  });
});

describe("system events", () => {
  it("are registered and keep the queries fresh", () => {
    expect(appRegistry.events).toContain(handleSystemEvent);
    const queryClient = new QueryClient();
    const invalidate = vi.spyOn(queryClient, "invalidateQueries");
    const ctx = { queryClient } as unknown as FeatureEventContext;
    const status = { current_version: "1.0.0" } as UpdateStatus;
    handleSystemEvent({ type: "UpdateStatusChanged", status }, ctx);
    expect(queryClient.getQueryData(systemKeys.updates)).toBe(status);
    handleSystemEvent({ type: "DefaultAppsChanged" }, ctx);
    expect(invalidate).toHaveBeenCalledWith({ queryKey: systemKeys.defaultApps });
    expect(SYSTEM_EVENTS).toHaveLength(3);
  });
});
