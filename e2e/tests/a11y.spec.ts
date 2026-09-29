import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";

const SERIOUS = new Set(["serious", "critical"]);

async function seriousViolations(page: Page) {
  const results = await new AxeBuilder({ page }).analyze();
  return results.violations
    .filter((v) => SERIOUS.has(v.impact ?? ""))
    .map((v) => ({ id: v.id, help: v.help, targets: v.nodes.map((n) => n.target.join(" ")) }));
}

test("the main window has no serious accessibility violations", async ({ page }) => {
  await page.goto("/");
  await expect(page.locator('[data-part="message-list.row"]').first()).toBeVisible();
  expect(await seriousViolations(page)).toEqual([]);
});

test("the reader has no serious accessibility violations", async ({ page }) => {
  await page.goto("/");
  const rows = page.locator('[data-part="message-list.row"]');
  await rows.filter({ hasText: "Welcome to Nail" }).click();
  await expect(page.locator('[data-part="message-view.header.subject"]')).toContainText(
    "Welcome to Nail",
  );
  expect(await seriousViolations(page)).toEqual([]);
});

test("dark mode has no serious accessibility violations", async ({ page }) => {
  await page.goto("/?mode=dark");
  await expect(page.locator('[data-part="message-list.row"]').first()).toBeVisible();
  expect(await seriousViolations(page)).toEqual([]);
});
