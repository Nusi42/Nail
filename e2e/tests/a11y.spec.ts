import { expect, test } from "@playwright/test";
import { startNail, nailPerTest } from "../support/nail";
import { rpcOk } from "../support/remote";
import {
  composerWindow,
  discardComposer,
  newMessageButton,
  openSettings,
  READER_SUBJECT,
  readerAction,
  readerToolbar,
  runFromPalette,
  settingsNav,
  showView,
  messageRows,
  DEMO_MULTI_CONVERSATIONS,
  openMail,
} from "../support/views";
import { axeViolations } from "../support/axe";

test("the main window has no accessibility violations", async ({ page }) => {
  await page.goto("/");
  await expect(messageRows(page).first()).toBeVisible();
  expect(await axeViolations(page)).toEqual([]);
});

test("the reader has no accessibility violations", async ({ page }) => {
  await page.goto("/");
  const rows = messageRows(page);
  await rows.filter({ hasText: "Welcome to Nail" }).click();
  await expect(page.locator(READER_SUBJECT)).toContainText("Welcome to Nail");
  expect(await axeViolations(page)).toEqual([]);
});

test("dark mode has no accessibility violations", async ({ page }) => {
  await page.goto("/?mode=dark");
  await expect(messageRows(page).first()).toBeVisible();
  expect(await axeViolations(page)).toEqual([]);
});

// The high-contrast mode (colour contrast is only checked in a browser).
test("high-contrast mode has no accessibility violations", async ({ page }) => {
  await page.goto("/?mode=high-contrast");
  const rows = messageRows(page);
  await expect(rows.first()).toBeVisible();
  expect(await axeViolations(page), "main window").toEqual([]);
  await rows.filter({ hasText: "Welcome to Nail" }).click();
  await expect(page.locator(READER_SUBJECT)).toContainText("Welcome to Nail");
  expect(await axeViolations(page), "reader").toEqual([]);
});

test.describe("windows and dialogs of demo-multi", () => {
  const server = nailPerTest({ seed: "demo-multi" });

  test("have no accessibility violations", async ({ page }) => {
    test.setTimeout(120_000);
    await openMail(page, server.url, { rows: DEMO_MULTI_CONVERSATIONS });
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
    expect(await axeViolations(page)).toEqual([]);
    await composer.getByRole("radio", { name: "HTML" }).click();
    await expect(composer.locator('[data-part="composer.html.content"]')).toBeVisible();
    expect(await axeViolations(page)).toEqual([]);
    await composer.getByRole("radio", { name: "Markdown" }).click();
    await expect(composer.locator('[data-part="composer.markdown.content"]')).toBeVisible();
    expect(await axeViolations(page)).toEqual([]);
    await discardComposer(page, composer);
    await expect(composer).toBeHidden();

    // Outbox, filter rules, account setup.
    await runFromPalette(page, "outbox", "Outbox");
    await expect(page.getByRole("dialog", { name: "Outbox" })).toBeVisible();
    expect(await axeViolations(page)).toEqual([]);
    await close("Outbox");

    await runFromPalette(page, "filter rules", "Filter rules");
    const rules = page.getByRole("dialog", { name: "Filter rules" });
    await rules.getByRole("button", { name: "New rule" }).click();
    await expect(rules.getByRole("form", { name: "Rule" })).toBeVisible();
    expect(await axeViolations(page)).toEqual([]);
    await close("Filter rules");

    await runFromPalette(page, "add account", "Add account");
    await expect(page.getByRole("dialog", { name: "Add account" })).toBeVisible();
    expect(await axeViolations(page)).toEqual([]);
    await close("Add account");

    // M2c: the snoozed list, the snooze picker and a folder's sync dialog.
    await runFromPalette(page, "snoozed", "Snoozed messages");
    await expect(page.getByRole("dialog", { name: "Snoozed messages" })).toBeVisible();
    expect(await axeViolations(page), "snoozed").toEqual([]);
    await close("Snoozed messages");
    await messageRows(page).filter({ hasText: "Welcome to Nail" }).click();
    await readerAction(page, "Snooze…");
    await expect(page.getByRole("dialog", { name: "Snooze" })).toBeVisible();
    expect(await axeViolations(page), "snooze").toEqual([]);
    await page
      .getByRole("dialog", { name: "Snooze" })
      .getByRole("button", { name: "Cancel" })
      .click();
    await page
      .getByRole("treeitem", { name: /^Inbox/ })
      .first()
      .click({ button: "right" });
    await expect(page.getByRole("menu")).toBeVisible();
    expect(await axeViolations(page), "folder menu").toEqual([]);
    await page.getByRole("menu").getByRole("menuitem", { name: "Synchronization…" }).click();
    await expect(page.getByRole("dialog", { name: "Folder synchronization" })).toBeVisible();
    expect(await axeViolations(page), "sync-folder").toEqual([]);
    await close("Folder synchronization");

    // The message context menu and the move window (folder picker).
    await messageRows(page).filter({ hasText: "Welcome to Nail" }).click({ button: "right" });
    const menu = page.getByRole("menu");
    await expect(menu).toBeVisible();
    expect(await axeViolations(page), "message menu").toEqual([]);
    await menu.getByRole("menuitem", { name: "Move to…" }).click();
    await expect(page.getByRole("dialog", { name: "Move to folder" })).toBeVisible();
    expect(await axeViolations(page), "move").toEqual([]);
    await close("Move to folder");

    // Every settings section (M5b: the settings page holds every former tab).
    const settings = await openSettings(page);
    const sections = await settingsNav(settings)
      .getByRole("button")
      .evaluateAll((items) => items.map((item) => item.textContent.trim()));
    expect(sections.length).toBeGreaterThanOrEqual(15);
    for (const section of sections) {
      await openSettings(page, section);
      expect(await axeViolations(page), section).toEqual([]);
    }
    await openSettings(page, "Layout & toolbars");
    // The merge dialog, opened from the Layout & toolbars section.
    await settings.getByRole("button", { name: "Apply a theme or layout…", exact: true }).click();
    await expect(page.getByRole("dialog", { name: "Apply a theme or layout" })).toBeVisible();
    expect(await axeViolations(page)).toEqual([]);
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
        const rows = messageRows(page);
        await rows.filter({ hasText: "Welcome to Nail" }).click();
        await expect(page.locator(READER_SUBJECT)).toContainText("Welcome to Nail");
        expect(await axeViolations(page), `${theme}: mail`).toEqual([]);
        await readerToolbar(page).getByRole("button", { name: "More", exact: true }).click();
        await expect(page.getByRole("menu")).toBeVisible();
        expect(await axeViolations(page), `${theme}: More menu`).toEqual([]);
        await page.keyboard.press("Escape");
        await showView(page, "Calendar");
        expect(await axeViolations(page), `${theme}: calendar`).toEqual([]);
        await openSettings(page, "Appearance");
        expect(await axeViolations(page), `${theme}: settings`).toEqual([]);
      } finally {
        await server.stop();
      }
    });
  }
});
