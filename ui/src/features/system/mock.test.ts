import { QueryClient } from "@tanstack/react-query";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { Event as NailEvent, UpdateStatus } from "../../api/generated";
import { MockTransport } from "../../api/mock";
import { appRegistry } from "../registry";
import type { FeatureEventContext } from "../types";
import { SYSTEM_EVENTS, handleSystemEvent, setUpdateState, systemKeys } from ".";
import { runtimeIds } from "./messages";
import {
  SYSTEM_REQUESTS,
  isSystemRequest,
  mapiRecipientEmail,
  simulateMapiSendMail,
  updateStatusOf,
} from "./mock";
import {
  DEFAULT_APP_TARGETS,
  MAPI_RESULTS,
  canCheck,
  canDownload,
  canInstall,
  compareVersions,
  isBusy,
  parseReleaseNotes,
  progressPercent,
  updateOf,
} from "./model";

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
    expect(runtimeIds()).toContain("window-first-run");
  });

  it("orders semver versions with pre-releases", () => {
    expect(compareVersions("1.1.0", "1.0.0")).toBeGreaterThan(0);
    expect(compareVersions("1.0.0", "1.0.0")).toBe(0);
    expect(compareVersions("1.2.0-beta.1", "1.2.0")).toBeLessThan(0);
    expect(compareVersions("1.2.0-beta.2", "1.2.0-beta.10")).toBeLessThan(0);
    expect(compareVersions("1.2.0-beta.1", "1.1.9")).toBeGreaterThan(0);
    expect(compareVersions("v2.0.0+build.5", "2.0.0")).toBe(0);
    expect(compareVersions("1.0.0-alpha", "1.0.0-1")).toBeGreaterThan(0);
  });

  it("reads release notes as text blocks, never HTML", () => {
    const blocks = parseReleaseNotes(
      [
        "## What is new",
        "",
        "- **Fast** search",
        "- See [notes](https://x.example)",
        "  continued",
        "",
        "Plain <b>text</b> `code`",
        "more",
      ].join("\n"),
    );
    expect(blocks).toEqual([
      { kind: "heading", text: "What is new" },
      { kind: "list", items: ["Fast search", "See notes continued"] },
      { kind: "paragraph", text: "Plain <b>text</b> code more" },
    ]);
    expect(parseReleaseNotes("")).toEqual([]);
  });

  it("knows progress and the release of a state", () => {
    const downloading = {
      state: "downloading",
      update: release,
      downloaded_bytes: 25,
      total_bytes: 100,
    } as const;
    expect(progressPercent(downloading)).toBe(25);
    expect(progressPercent({ ...downloading, total_bytes: null })).toBeNull();
    expect(updateOf(downloading)).toBe(release);
    expect(updateOf({ state: "idle" })).toBeNull();
    expect(isBusy(downloading)).toBe(true);
    expect(isBusy({ state: "up-to-date" })).toBe(false);
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
    t.data.features.system.autoProgress = false;
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
    // The mock starts as a returning user; a fresh install has not seen it.
    expect(await t.send({ type: "GetFirstRun" })).toMatchObject({ state: { completed: true } });
    t.data.features.system.firstRun = {
      completed: false,
      completed_at: null,
      completed_version: null,
    };
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

describe("simulated updater", () => {
  afterEach(() => vi.useRealTimers());

  async function steps(t: MockTransport, n = 10) {
    for (let i = 0; i < n; i++) await vi.advanceTimersByTimeAsync(t.data.behaviour.workMs);
  }

  it("checks, downloads in steps and gets ready to install", async () => {
    vi.useFakeTimers();
    const t = new MockTransport();
    const seen: string[] = [];
    t.onEvent((e) => {
      if (e.type === "UpdateStatusChanged") seen.push(e.status.state.state);
    });
    await t.send({ type: "CheckForUpdates" });
    await steps(t);
    expect(seen[0]).toBe("checking");
    expect(seen).toContain("available");
    expect(seen.filter((s) => s === "downloading").length).toBeGreaterThan(1);
    expect(seen.at(-1)).toBe("ready-to-install");
    const status = updateStatusOf(t.data.features.system);
    expect(status.state).toMatchObject({ update: { version: "1.1.0" } });
    expect(status.last_checked_at).not.toBeNull();
    expect(await t.send({ type: "InstallUpdate" })).toEqual({ type: "Done" });
    expect(updateStatusOf(t.data.features.system).state.state).toBe("installing");
  });

  it("waits for Download without auto download; beta sees the beta release", async () => {
    vi.useFakeTimers();
    const t = new MockTransport();
    t.data.features.system.updateSettings = {
      channel: "beta",
      auto_check: true,
      auto_download: false,
    };
    await t.send({ type: "CheckForUpdates" });
    await steps(t);
    expect(updateStatusOf(t.data.features.system).state).toMatchObject({
      state: "available",
      update: { version: "1.2.0-beta.1" },
    });
    await t.send({ type: "DownloadUpdate" });
    await steps(t);
    expect(updateStatusOf(t.data.features.system).state.state).toBe("ready-to-install");
  });

  it("is up to date without a newer release and fails when told to", async () => {
    vi.useFakeTimers();
    const t = new MockTransport();
    t.data.features.system.releases = {};
    await t.send({ type: "CheckForUpdates" });
    await steps(t);
    expect(updateStatusOf(t.data.features.system).state.state).toBe("up-to-date");
    t.data.features.system.failNext = "network";
    await t.send({ type: "CheckForUpdates" });
    await steps(t);
    expect(updateStatusOf(t.data.features.system).state).toMatchObject({
      state: "failed",
      failure: "network",
    });
  });

  it("stops pending steps when the state is set by hand", async () => {
    vi.useFakeTimers();
    const t = new MockTransport();
    await t.send({ type: "CheckForUpdates" });
    setUpdateState(t, { state: "idle" });
    await steps(t);
    expect(updateStatusOf(t.data.features.system).state.state).toBe("idle");
  });
});

describe("simulated Simple MAPI", () => {
  const file = {
    path: "C:\\Temp\\report.xlsx",
    file_name: null,
    mime: "application/vnd.ms-excel",
    data: "AAEC",
  };
  const base = {
    subject: null,
    body: null,
    recipients: [],
    attachments: [],
    ui: "dialog",
    program: null,
  } as const;

  it("saves a draft with recipients, text and files and asks to open it", async () => {
    const t = new MockTransport();
    const seen: string[] = [];
    t.onEvent((e) => seen.push(e.type));
    const result = simulateMapiSendMail(t, {
      subject: "Quarterly report",
      body: "Here it is.",
      recipients: [
        { class: "to", name: "Ada", address: "SMTP:ada@example.org" },
        { class: "cc", name: "bob@example.org", address: null },
      ],
      attachments: [file],
      ui: "none",
      program: "EXCEL.EXE",
    });
    expect(result).toBe("success");
    expect(seen).toContain("LaunchRequested");
    const launch = t.data.launches.at(-1);
    expect(launch).toMatchObject({ type: "ComposeDraft", program: "EXCEL.EXE" });
    if (launch?.type !== "ComposeDraft") throw new Error("no launch");
    const draft = await t.send({ type: "GetDraft", draft_id: launch.draft_id });
    expect(draft).toMatchObject({
      type: "Draft",
      draft: {
        subject: "Quarterly report",
        to: [{ name: "Ada", email: "ada@example.org" }],
        cc: [{ name: null, email: "bob@example.org" }],
        attachments: [{ filename: "report.xlsx", size: 3 }],
      },
    });
    if (draft.type !== "Draft") throw new Error(draft.type);
    expect(draft.draft.text.startsWith("Here it is.")).toBe(true);
    // Never sent, even though the program asked for no UI.
    expect(t.requests.some((r) => r.type === "SendDraft")).toBe(false);
  });

  it("answers MAPI errors like the core", () => {
    const t = new MockTransport();
    expect(
      simulateMapiSendMail(t, {
        ...base,
        recipients: [{ class: "to", name: "Ada", address: "not an address" }],
      }),
    ).toBe("invalid-recips");
    expect(
      simulateMapiSendMail(t, {
        ...base,
        attachments: [{ path: "C:\\gone.txt", file_name: null }],
      }),
    ).toBe("attachment-not-found");
    expect(
      simulateMapiSendMail(t, { ...base, attachments: [{ ...file, path: "relative.txt" }] }),
    ).toBe("attachment-not-found");
    expect(
      simulateMapiSendMail(t, { ...base, attachments: Array.from({ length: 101 }, () => file) }),
    ).toBe("too-many-files");
    expect(mapiRecipientEmail({ class: "to", name: null, address: "<x@y.example>" })).toBe(
      "x@y.example",
    );
    t.data.accountsConfig.accounts = [];
    expect(simulateMapiSendMail(t, base)).toBe("login-failure");
    expect(t.data.launches).toEqual([]);
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
