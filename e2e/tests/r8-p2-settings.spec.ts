import { expect, test, type Locator } from "@playwright/test";
import { nailPerTest } from "../support/nail";
import { discardSettings, saveSettings, settingsSaveBar } from "../support/settings";
import { openMail, openSettings, rail, settingsNav } from "../support/views";

/**
 * R8 (mail and windows UI) against `nail serve --seed demo`: the settings
 * page — every section's widgets name themselves without repeating the
 * section's title, "Pair a device" once in Remote access, the navigation
 * marks the shown section while it has unsaved changes, a layout preset is
 * written with the settings bar's Save; the toolbar customizer's grouped
 * actions.
 */

const server = nailPerTest({ seed: "demo" });

/** The shown section's widget headings (cards, sections) as they show, trimmed. */
async function widgetHeadings(view: Locator): Promise<string[]> {
  return view
    .locator('[data-part="settings-page.body"]')
    .locator("h2, h3, h4, [data-part='card.title']")
    .evaluateAll((els) =>
      els
        // Shown: a visually hidden heading (it names a region) is no repeat.
        .filter((el) => el.getBoundingClientRect().width > 1)
        .map((el) => el.textContent.trim()),
    );
}

test("no settings widget repeats its section's title; Pair a device shows once", async ({
  page,
}) => {
  await openMail(page, server.url);
  const view = await openSettings(page);
  const sections = await settingsNav(view)
    .locator('[data-part="settings-page.nav.item"]')
    .allTextContents();
  expect(sections.length).toBeGreaterThan(5);
  const repeats: string[] = [];
  for (const section of sections) {
    await openSettings(page, section);
    const headings = await widgetHeadings(view);
    if (headings.includes(section)) repeats.push(section);
  }
  expect(repeats).toEqual([]);

  await openSettings(page, "Remote access");
  await expect(view.getByRole("button", { name: "Pair a device", exact: true })).toHaveCount(1);
});

test("the shown section is marked while it has unsaved changes", async ({ page }) => {
  await openMail(page, server.url);
  const view = await openSettings(page, "Notifications");
  const entry = settingsNav(view).getByRole("button", { name: "Notifications", exact: true });
  const mark = entry.locator('[data-part="settings-page.nav.unsaved"]');
  await expect(mark).toHaveCount(0);
  await view.getByRole("checkbox", { name: "Tell me about new mail" }).click();
  await expect(settingsSaveBar(page)).toBeVisible();
  await expect(mark).toBeVisible();
  await expect(entry).toHaveAccessibleDescription("Unsaved changes");
  await discardSettings(page);
  await expect(mark).toHaveCount(0);
});

test("a layout preset waits for the settings bar's Save", async ({ page }) => {
  await openMail(page, server.url);
  const view = await openSettings(page, "Layout & toolbars");
  const card = view.getByRole("region", { name: "Window layout" });
  const wide = card.getByRole("button", { name: /Wide reader/ });
  const three = card.getByRole("button", { name: /Three panes/ });
  await expect(three).toHaveAttribute("aria-pressed", "true");
  await wide.click();
  await expect(wide).toHaveAttribute("aria-pressed", "true");
  await expect(settingsSaveBar(page)).toBeVisible();
  // Discard forgets it; nothing was written.
  await discardSettings(page);
  await expect(three).toHaveAttribute("aria-pressed", "true");
  await expect(card.locator('[data-part="layout-editor.status"]')).toHaveText("");
  await wide.click();
  await saveSettings(page);
  await expect(card.locator('[data-part="layout-editor.status"]')).toContainText("Wide reader");
  await expect(wide).toHaveAttribute("aria-pressed", "true");
  await expect(rail(page)).toBeVisible();
});

test("the toolbar customizer lists every action, grouped, and the search narrows the groups", async ({
  page,
}) => {
  await openMail(page, server.url);
  const view = await openSettings(page, "Layout & toolbars");
  await view
    .locator('[data-part="toolbar-customizer-settings"]')
    .getByRole("button")
    .first()
    .click();
  const dialog = page.getByRole("dialog", { name: "Customize toolbar" });
  const available = dialog.getByRole("listbox", { name: "Available actions" });
  await expect(available).toBeVisible();
  await expect(available.getByRole("group", { name: "Spacing" })).toBeVisible();
  await expect(available.getByRole("group", { name: "Message list" })).toBeVisible();
  expect(await available.getByRole("option").count()).toBeGreaterThan(30);
  await dialog.getByRole("searchbox", { name: "Find an action" }).fill("reply all");
  await expect(available.getByRole("group", { name: "Spacing" })).toHaveCount(0);
  await expect(available.getByRole("option", { name: /^Reply all/ }).first()).toBeVisible();
});
