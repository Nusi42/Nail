import { writeFileSync } from "node:fs";
import path from "node:path";
import { expect, test as base } from "@playwright/test";
import { startNail, type NailServer } from "../support/nail";

/**
 * Tests that change theme.kdl / layout.kdl get their own `nail serve`, so
 * they never disturb tests using the shared server.
 */
const test = base.extend<{ nail: NailServer }>({
  // eslint-disable-next-line no-empty-pattern -- Playwright requires a destructuring pattern for fixtures without dependencies
  nail: async ({}, use) => {
    const server = await startNail();
    await use(server);
    await server.stop();
  },
});

const accent = () =>
  getComputedStyle(document.documentElement).getPropertyValue("--nail-color-accent").trim();

test("editing theme.kdl hot-reloads a CSS custom property", async ({ page, nail }) => {
  await page.goto(nail.url);
  await expect(page.locator('[data-part="message-list"]')).toBeVisible();
  const before = await page.evaluate(accent);
  expect(before).not.toBe("");
  expect(before.toLowerCase()).not.toBe("#ff0066");

  writeFileSync(
    path.join(nail.configDir, "theme.kdl"),
    [
      'theme "E2E" extends="default" mode="light" {',
      "    tokens {",
      '        color "accent" "#ff0066"',
      "    }",
      "}",
      "",
    ].join("\n"),
  );
  await expect.poll(() => page.evaluate(accent)).toBe("#ff0066");

  // A broken file keeps the last valid theme and reports the error.
  writeFileSync(path.join(nail.configDir, "theme.kdl"), 'theme "Broken" {\n  tokens {\n');
  await expect(page.locator('[data-item="sync-progress"]')).toHaveAttribute("data-state", "error");
  await expect(page.locator('[data-item="sync-progress"]')).toContainText("theme.kdl");
  expect(await page.evaluate(accent)).toBe("#ff0066");
});

test("editing layout.kdl moves and removes panes", async ({ page, nail }) => {
  await page.goto(nail.url);
  const panes = page.locator(
    '[data-part="split"][data-node-id="panes"] > [data-part="split.pane"]',
  );
  await expect(panes).toHaveCount(3);
  await expect(page.locator('[data-part="message-view"]')).toBeVisible();

  // The message list moves to the left, the reader is gone.
  writeFileSync(
    path.join(nail.configDir, "layout.kdl"),
    [
      'layout "E2E" extends="default" {',
      '    window "main" {',
      '        split "horizontal" id="panes" sizes="1fr 240px" {',
      '            message-list id="list" label="@layout.pane.messages" {',
      '                row "{subject}"',
      "            }",
      '            folder-tree id="folders" label="@layout.pane.folders"',
      "        }",
      "    }",
      "}",
      "",
    ].join("\n"),
  );
  await expect(panes).toHaveCount(2);
  await expect(panes.nth(0).locator('[data-part="message-list"]')).toBeVisible();
  await expect(panes.nth(1).locator('[data-part="folder-tree"]')).toBeVisible();
  await expect(page.locator('[data-part="message-view"]')).toHaveCount(0);
  await expect(page.locator('[data-part="status-bar"]')).toHaveCount(0);
  await expect(page.locator('[data-part="message-list.row"]').first()).toBeVisible();
});

test("the server's German locale falls back to English strings", async ({ page }) => {
  const server = await startNail({ locale: "de" });
  try {
    await page.goto(server.url);
    await expect(page.locator("html")).toHaveAttribute("lang", "de");
    await expect(page.getByRole("tree", { name: "Folders" })).toBeVisible();
    await expect(page.locator('[data-item="sync-progress"]')).toHaveText("Up to date");
  } finally {
    await server.stop();
  }
});
