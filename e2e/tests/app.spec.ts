import { expect, test } from "@playwright/test";

// Placeholder until M0 WP6 starts `nail serve --seed demo` as the web server.
test.skip("default three-pane layout renders", async ({ page }) => {
  await page.goto("/");
  await expect(page.locator('[data-part="app"]')).toBeVisible();
});
