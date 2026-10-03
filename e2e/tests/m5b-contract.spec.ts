import { expect, test, type Page } from "@playwright/test";
import { startNail, type NailServer } from "../support/nail";

/**
 * M5b contract (ADR 0109) against `nail serve --seed demo`: the view
 * shortcuts of the default layout (`ctrl+1` … `ctrl+4` → `view.mail` …
 * aliases of `view.show`) work before the rail exists — without a
 * `view-host` in the main window a view opens as its window — and the
 * command palette lists the views.
 */

let server: NailServer;

test.beforeEach(async () => {
  server = await startNail({ seed: "demo" });
});

test.afterEach(async () => {
  await server.stop();
});

async function open(page: Page) {
  await page.goto(server.url);
  await expect(page.locator('[data-part="message-list.row"]').first()).toBeVisible();
  await page.locator('[data-part="message-list.row"]').first().click();
}

test("view shortcuts open the views' windows", async ({ page }) => {
  await open(page);
  await page.keyboard.press("Control+2");
  const calendar = page.getByRole("dialog", { name: "Calendar", exact: true });
  await expect(calendar).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(calendar).toBeHidden();

  await page.keyboard.press("Control+3");
  const contacts = page.getByRole("dialog", { name: "Contacts", exact: true });
  await expect(contacts).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(contacts).toBeHidden();
});

test("the command palette lists the views", async ({ page }) => {
  await open(page);
  await page.keyboard.press("Control+k");
  const palette = page.getByRole("dialog", { name: "Command palette" });
  await expect(palette).toBeVisible();
  await palette.getByRole("combobox").fill("tasks");
  // The view alias and the older window action both say "Tasks".
  const option = palette.getByRole("option", { name: /^Tasks/ }).first();
  await expect(option).toBeVisible();
  await option.click();
  await expect(page.getByRole("dialog", { name: "Tasks", exact: true })).toBeVisible();
});
