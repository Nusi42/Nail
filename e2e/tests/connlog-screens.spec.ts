import path from "node:path";
import { expect, test, type Locator, type Page } from "@playwright/test";
import {
  errorDetailsWindow,
  openWorkLog,
  otherMailbox,
  showRefusedFolderDetails,
} from "../support/connlog";
import { startNail, type NailServer } from "../support/nail";
import { rpcOk } from "../support/remote";
import { openSettings } from "../support/views";

/**
 * ADR 0190: screenshots of the connection log for review (not a check):
 * the error with "Show details" (Frost, light), the connection log viewer
 * (Graphite, dark) and the disk logging switch (both). Runs only with
 * `NAIL_CONNLOG_SCREENS=<folder>`, where the PNG files go.
 */

const out = process.env.NAIL_CONNLOG_SCREENS ?? "";

test.skip(out === "", "NAIL_CONNLOG_SCREENS is not set");

let server: NailServer;

test.beforeEach(async () => {
  server = await startNail({ seed: "demo-multi" });
});

test.afterEach(async () => {
  await server.stop();
});

async function useTheme(page: Page, theme: "frost" | "graphite"): Promise<void> {
  await rpcOk(server, { type: "SetTheme", theme_id: theme }, "Themes");
  await page.goto(server.url);
  await expect(page.getByRole("navigation", { name: "Sections", exact: true })).toBeVisible();
}

async function shoot(target: Locator | Page, name: string): Promise<void> {
  await target.screenshot({ path: path.join(out, name), animations: "disabled" });
}

test("connection log screenshots", async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  const folder = await otherMailbox(server);

  await useTheme(page, "frost");
  const details = await showRefusedFolderDetails(page, server, folder);
  await expect(details.locator('[data-field="server"]')).toBeVisible();
  await shoot(page, "error-details-frost.png");
  await shoot(errorDetailsWindow(page), "error-details-window-frost.png");

  await useTheme(page, "graphite");
  const viewer = await openWorkLog(page);
  await expect(viewer.locator('[data-part="connection-log.entry"]').first()).toBeVisible();
  await shoot(page, "connection-log-graphite.png");
  await shoot(viewer, "connection-log-window-graphite.png");

  for (const theme of ["frost", "graphite"] as const) {
    await useTheme(page, theme);
    const settings = await openSettings(page, "Data & backup");
    const card = settings.locator('[data-part="connection-log-settings"]');
    await card.scrollIntoViewIfNeeded();
    await expect(card.getByRole("switch", { name: "Keep connection log on disk" })).toBeVisible();
    await shoot(page, `settings-disk-logging-${theme}.png`);
  }
});
