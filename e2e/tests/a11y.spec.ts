import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";
import { startNail, type NailServer } from "../support/nail";
import { rpcOk } from "../support/remote";
import {
  composerWindow,
  discardComposer,
  newMessageButton,
  openSettings,
  readerAction,
  readerToolbar,
  runFromPalette,
  settingsNav,
  showView,
} from "../support/views";

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

// The high-contrast mode (colour contrast is only checked in a browser).
test("high-contrast mode has no accessibility violations", async ({ page }) => {
  await page.goto("/?mode=high-contrast");
  const rows = page.locator('[data-part="message-list.row"]');
  await expect(rows.first()).toBeVisible();
  expect(await violations(page), "main window").toEqual([]);
  await rows.filter({ hasText: "Welcome to Nail" }).click();
  await expect(page.locator('[data-part="message-view.header.subject"]')).toContainText(
    "Welcome to Nail",
  );
  expect(await violations(page), "reader").toEqual([]);
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
    const close = async (name: string) => {
      await page
        .getByRole("dialog", { name })
        .getByRole("button", { name: "Close" })
        .first()
        .click();
      await expect(page.getByRole("dialog", { name })).toBeHidden();
    };

    // The rich composer, in each mode.
    await newMessageButton(page).click();
    const composer = composerWindow(page);
    await expect(composer).toBeVisible();
    expect(await violations(page)).toEqual([]);
    await composer.getByRole("radio", { name: "HTML" }).click();
    await expect(composer.locator('[data-part="composer.html.content"]')).toBeVisible();
    expect(await violations(page)).toEqual([]);
    await composer.getByRole("radio", { name: "Markdown" }).click();
    await expect(composer.locator('[data-part="composer.markdown.content"]')).toBeVisible();
    expect(await violations(page)).toEqual([]);
    await discardComposer(page, composer);
    await expect(composer).toBeHidden();

    // Outbox, filter rules, account setup.
    await runFromPalette(page, "outbox", "Outbox");
    await expect(page.getByRole("dialog", { name: "Outbox" })).toBeVisible();
    expect(await violations(page)).toEqual([]);
    await close("Outbox");

    await runFromPalette(page, "filter rules", "Filter rules");
    const rules = page.getByRole("dialog", { name: "Filter rules" });
    await rules.getByRole("button", { name: "New rule" }).click();
    await expect(rules.getByRole("form", { name: "Rule" })).toBeVisible();
    expect(await violations(page)).toEqual([]);
    await close("Filter rules");

    await runFromPalette(page, "add account", "Add account");
    await expect(page.getByRole("dialog", { name: "Add account" })).toBeVisible();
    expect(await violations(page)).toEqual([]);
    await close("Add account");

    // M2c: the snoozed list, the snooze picker and a folder's sync dialog.
    await runFromPalette(page, "snoozed", "Snoozed messages");
    await expect(page.getByRole("dialog", { name: "Snoozed messages" })).toBeVisible();
    expect(await violations(page), "snoozed").toEqual([]);
    await close("Snoozed messages");
    await page
      .locator('[data-part="message-list.row"]')
      .filter({ hasText: "Welcome to Nail" })
      .click();
    await readerAction(page, "Snooze…");
    await expect(page.getByRole("dialog", { name: "Snooze" })).toBeVisible();
    expect(await violations(page), "snooze").toEqual([]);
    await page
      .getByRole("dialog", { name: "Snooze" })
      .getByRole("button", { name: "Cancel" })
      .click();
    await page
      .getByRole("treeitem", { name: /^Inbox/ })
      .first()
      .click({ button: "right" });
    await expect(page.getByRole("menu")).toBeVisible();
    expect(await violations(page), "folder menu").toEqual([]);
    await page.getByRole("menu").getByRole("menuitem", { name: "Synchronization…" }).click();
    await expect(page.getByRole("dialog", { name: "Folder synchronization" })).toBeVisible();
    expect(await violations(page), "sync-folder").toEqual([]);
    await close("Folder synchronization");

    // The message context menu and the move window (folder picker).
    await page
      .locator('[data-part="message-list.row"]')
      .filter({ hasText: "Welcome to Nail" })
      .click({ button: "right" });
    const menu = page.getByRole("menu");
    await expect(menu).toBeVisible();
    expect(await violations(page), "message menu").toEqual([]);
    await menu.getByRole("menuitem", { name: "Move to…" }).click();
    await expect(page.getByRole("dialog", { name: "Move to folder" })).toBeVisible();
    expect(await violations(page), "move").toEqual([]);
    await close("Move to folder");

    // Every settings section (M5b: the settings page holds every former tab).
    const settings = await openSettings(page);
    const sections = await settingsNav(settings)
      .getByRole("button")
      .evaluateAll((items) => items.map((item) => item.textContent?.trim() ?? ""));
    expect(sections.length).toBeGreaterThanOrEqual(15);
    for (const section of sections) {
      await openSettings(page, section);
      expect(await violations(page), section).toEqual([]);
    }
    await openSettings(page, "Layout & toolbars");
    // The merge dialog, opened from the Layout & toolbars section.
    await settings.getByRole("button", { name: "Apply a theme or layout…", exact: true }).click();
    await expect(page.getByRole("dialog", { name: "Apply a theme or layout" })).toBeVisible();
    expect(await violations(page)).toEqual([]);
  });
});

/**
 * M5b: every built-in theme (Frost, Sage, Graphite, Nocturne, High
 * contrast), Follow Windows off, over the new shell: the mail view with a
 * message and the reader's More menu open, the calendar view, the settings
 * page (colour contrast included, which jsdom cannot check).
 */
test.describe("every built-in theme", () => {
  for (const theme of ["frost", "sage", "graphite", "nocturne", "high-contrast"] as const) {
    test(`${theme} has no accessibility violations in the views`, async ({ page }) => {
      test.setTimeout(120_000);
      const server = await startNail({ seed: "demo" });
      try {
        await rpcOk(server, { type: "SetTheme", theme_id: theme }, "Themes");
        await rpcOk(
          server,
          {
            type: "SetThemePreferences",
            preferences: {
              follow_system: false,
              light: "frost",
              dark: "graphite",
              density: "comfortable",
              overrides: {},
            },
          },
          "Themes",
        );
        await page.goto(server.url);
        const rows = page.locator('[data-part="message-list.row"]');
        await rows.filter({ hasText: "Welcome to Nail" }).click();
        await expect(page.locator('[data-part="message-view.header.subject"]')).toContainText(
          "Welcome to Nail",
        );
        expect(await violations(page), `${theme}: mail`).toEqual([]);
        await readerToolbar(page).getByRole("button", { name: "More", exact: true }).click();
        await expect(page.getByRole("menu")).toBeVisible();
        expect(await violations(page), `${theme}: More menu`).toEqual([]);
        await page.keyboard.press("Escape");
        await showView(page, "Calendar");
        expect(await violations(page), `${theme}: calendar`).toEqual([]);
        await openSettings(page, "Appearance");
        expect(await violations(page), `${theme}: settings`).toEqual([]);
      } finally {
        await server.stop();
      }
    });
  }
});
