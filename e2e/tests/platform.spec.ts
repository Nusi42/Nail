import { expect, test } from "@playwright/test";
import { DESKTOP_APP_FEATURES } from "../../ui/src/api/generated/constants";
import type { Os } from "../../ui/src/api/generated/Os";
import type { PlatformInfo } from "../../ui/src/api/generated/PlatformInfo";
import { nailPerTest } from "../support/nail";
import { rpcOk } from "../support/remote";
import { DEMO_MULTI_CONVERSATIONS, openMail, openSettingsTab } from "../support/views";

/**
 * The platform the core reports (ADR 0202) decides what the UI offers: on
 * Windows the Windows certificate store, Windows sign-in for Exchange and
 * "Follow Windows"; on macOS and Linux none of them, and the texts name the
 * system's own parts. `nail serve` never reports the desktop app's
 * features. The same scenario runs on every system against whatever the
 * core says. Which feature belongs to which system is pinned by the
 * core's unit tests (`nail_api::platform`); here only what the UI makes of
 * the report.
 */

const server = nailPerTest({ seed: "demo-multi" });

async function platform(): Promise<PlatformInfo> {
  const config = await rpcOk(server, { type: "GetConfig" }, "Config");
  return config.platform as PlatformInfo;
}

test("offers what the core's system has and names its parts", async ({ page }) => {
  const { os, features } = await platform();
  // Web mode: the desktop app's features are never reported.
  for (const desktopOnly of DESKTOP_APP_FEATURES) expect(features).not.toContain(desktopOnly);
  const windowsStore = features.includes("windows-certificate-store");

  await openMail(page, server.url, { rows: DEMO_MULTI_CONVERSATIONS });

  const keys = await openSettingsTab(page, "Keys and certificates");
  const tools = keys.getByRole("toolbar", { name: "Key tools" });
  await expect(tools.getByRole("button", { name: "Find online" })).toBeVisible();
  await expect(tools.getByRole("button", { name: "Windows certificates" })).toHaveCount(
    windowsStore ? 1 : 0,
  );

  const appearance = await openSettingsTab(page, "Appearance");
  const follow: Partial<Record<Os, string>> = { windows: "Follow Windows", macos: "Follow macOS" };
  await expect(
    appearance.getByRole("switch", { name: follow[os] ?? "Follow the system" }),
  ).toBeVisible();
});
