import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";

/** Every axe violation, whatever its impact: the scan must pass completely. */
async function violations(page: Page) {
  const results = await new AxeBuilder({ page }).analyze();
  return results.violations.map((v) => ({
    id: v.id,
    impact: v.impact,
    help: v.help,
    targets: v.nodes.map((n) => n.target.join(" ")),
  }));
}

test("the main window has no accessibility violations", async ({ page }) => {
  await page.goto("/");
  await expect(page.locator('[data-part="message-list.row"]').first()).toBeVisible();
  expect(await violations(page)).toEqual([]);
});

test("the reader has no accessibility violations", async ({ page }) => {
  await page.goto("/");
  const rows = page.locator('[data-part="message-list.row"]');
  await rows.filter({ hasText: "Welcome to Nail" }).click();
  await expect(page.locator('[data-part="message-view.header.subject"]')).toContainText(
    "Welcome to Nail",
  );
  expect(await violations(page)).toEqual([]);
});

test("dark mode has no accessibility violations", async ({ page }) => {
  await page.goto("/?mode=dark");
  await expect(page.locator('[data-part="message-list.row"]').first()).toBeVisible();
  expect(await violations(page)).toEqual([]);
});
