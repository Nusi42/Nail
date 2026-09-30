import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";
import { startNail, type NailServer } from "../support/nail";

/**
 * Every axe violation, whatever its impact: the scan must pass completely.
 * The composer's preview frame is left out: it shows the outgoing mail as
 * its recipients will see it, in a sandbox without scripts that axe cannot
 * enter.
 */
async function violations(page: Page) {
  const results = await new AxeBuilder({ page })
    .exclude('[data-part="composer.preview.frame"]')
    .analyze();
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

test.describe("windows and dialogs of demo-multi", () => {
  let server: NailServer;

  test.beforeEach(async () => {
    server = await startNail({ seed: "demo-multi" });
  });

  test.afterEach(async () => {
    await server.stop();
  });

  test("have no accessibility violations", async ({ page }) => {
    test.setTimeout(120_000);
    await page.goto(server.url);
    await expect(page.locator('[data-part="message-list.row"]')).toHaveCount(17);
    const toolbar = page.getByRole("toolbar", { name: "Main toolbar" });
    const close = async (name: string) => {
      await page
        .getByRole("dialog", { name })
        .getByRole("button", { name: "Close" })
        .first()
        .click();
      await expect(page.getByRole("dialog", { name })).toBeHidden();
    };

    // The rich composer, in each mode.
    await toolbar.getByRole("button", { name: "New message" }).click();
    const composer = page.getByRole("dialog", { name: "New message" });
    await expect(composer).toBeVisible();
    expect(await violations(page)).toEqual([]);
    await composer.getByRole("radio", { name: "HTML" }).click();
    await expect(composer.locator('[data-part="composer.html.content"]')).toBeVisible();
    expect(await violations(page)).toEqual([]);
    await composer.getByRole("radio", { name: "Markdown" }).click();
    await expect(composer.locator('[data-part="composer.markdown.content"]')).toBeVisible();
    expect(await violations(page)).toEqual([]);
    await composer.getByRole("button", { name: "Discard" }).click();
    await expect(composer).toBeHidden();

    // Outbox, filter rules, account setup.
    await toolbar.getByRole("button", { name: "Outbox" }).click();
    await expect(page.getByRole("dialog", { name: "Outbox" })).toBeVisible();
    expect(await violations(page)).toEqual([]);
    await close("Outbox");

    await toolbar.getByRole("button", { name: "Filter rules" }).click();
    const rules = page.getByRole("dialog", { name: "Filter rules" });
    await rules.getByRole("button", { name: "New rule" }).click();
    await expect(rules.getByRole("form", { name: "Rule" })).toBeVisible();
    expect(await violations(page)).toEqual([]);
    await close("Filter rules");

    await toolbar.getByRole("button", { name: "Add account" }).click();
    await expect(page.getByRole("dialog", { name: "Add account" })).toBeVisible();
    expect(await violations(page)).toEqual([]);
    await close("Add account");

    // M2c: the snoozed list, the snooze picker and a folder's sync dialog.
    await toolbar.getByRole("button", { name: "Snoozed messages" }).click();
    await expect(page.getByRole("dialog", { name: "Snoozed messages" })).toBeVisible();
    expect(await violations(page), "snoozed").toEqual([]);
    await close("Snoozed messages");
    await page
      .locator('[data-part="message-list.row"]')
      .filter({ hasText: "Welcome to Nail" })
      .click();
    await page
      .getByRole("toolbar", { name: "Message actions" })
      .getByRole("button", { name: "Snooze…" })
      .click();
    await expect(page.getByRole("dialog", { name: "Snooze" })).toBeVisible();
    expect(await violations(page), "snooze").toEqual([]);
    await page
      .getByRole("dialog", { name: "Snooze" })
      .getByRole("button", { name: "Cancel" })
      .click();
    await page.getByRole("treeitem", { name: /^INBOX/ }).click({ button: "right" });
    await expect(page.getByRole("menu")).toBeVisible();
    expect(await violations(page), "folder menu").toEqual([]);
    await page.getByRole("menu").getByRole("menuitem", { name: "Synchronization…" }).click();
    await expect(page.getByRole("dialog", { name: "Folder synchronization" })).toBeVisible();
    expect(await violations(page), "sync-folder").toEqual([]);
    await close("Folder synchronization");

    // Every settings tab (M2c: server rules, vacation, junk, notifications,
    // synchronization, and the Customize editors).
    await toolbar.getByRole("button", { name: "Settings" }).click();
    const settings = page.getByRole("dialog", { name: "Settings" });
    for (const tab of [
      "Accounts",
      "Virtual accounts",
      "MCP clients",
      "Server rules",
      "Vacation reply",
      "Junk mail",
      "Notifications",
      "Synchronization",
      "Customize",
      "Toolbars",
      "Keys",
      "Columns",
    ]) {
      await settings.getByRole("tab", { name: tab, exact: true }).click();
      await expect(settings.getByRole("tab", { name: tab, exact: true })).toHaveAttribute(
        "aria-selected",
        "true",
      );
      expect(await violations(page), tab).toEqual([]);
    }
    await settings.getByRole("tab", { name: "Customize", exact: true }).click();
    // The merge dialog, opened from the Customize tab.
    await settings.getByRole("button", { name: "Apply a theme or layout…", exact: true }).click();
    await expect(page.getByRole("dialog", { name: "Apply a theme or layout" })).toBeVisible();
    expect(await violations(page)).toEqual([]);
  });
});
