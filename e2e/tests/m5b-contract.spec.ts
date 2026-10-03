import { writeFileSync } from "node:fs";
import path from "node:path";
import { expect, test, type Page } from "@playwright/test";
import { startNail, type NailServer } from "../support/nail";

/**
 * M5b contract (ADR 0109) against `nail serve --seed demo` with a layout of
 * the user's from before the rail (its own `window "main"`, no view host):
 * the view shortcuts (`ctrl+1` … `ctrl+4` → `view.mail` … aliases of
 * `view.show`) still work — without a `view-host` in the main window a view
 * opens as its window — and the command palette lists the views.
 */

let server: NailServer;

/** A pre-M5b layout: its own three-pane main window, extending the default. */
const OLD_LAYOUT = [
  'layout "Mine" extends="default" {',
  '    window "main" {',
  '        split "horizontal" id="panes" sizes="240px 1fr 1.4fr" {',
  '            folder-tree id="folders" show-unread=#true',
  '            message-list id="list"',
  '            message-view id="message"',
  "        }",
  "    }",
  "}",
  "",
].join("\n");

test.beforeEach(async () => {
  server = await startNail({ seed: "demo" });
  writeFileSync(path.join(server.configDir, "layout.kdl"), OLD_LAYOUT);
});

test.afterEach(async () => {
  await server.stop();
});

async function open(page: Page) {
  await page.goto(server.url);
  await expect(page.locator('[data-part="message-list.row"]').first()).toBeVisible();
  // The old main window shows; there is no rail.
  await expect(page.locator('[data-part="nav-rail"]')).toHaveCount(0);
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
