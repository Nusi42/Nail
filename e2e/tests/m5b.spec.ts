import { readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { expect, test, type Page } from "@playwright/test";
import { startNail, type NailServer } from "../support/nail";
import { createUser, enableRemoteUi, pairBrowser, pairingCode } from "../support/remote";
import {
  openSettings,
  rail,
  railButton,
  readerToolbar,
  runFromPalette,
  showView,
  viewRegion,
} from "../support/views";

/**
 * Wave M5b (ADRs 0109–0114) against `nail serve --seed demo` (and
 * `demo-multi` where several accounts matter): the navigation rail and the
 * views, the unread badge, the remembered view, the reader's ranked action
 * bar with "More", the list header's filter chips, the settings page's
 * search and remembered section, the calendar's view switch and calendar
 * menu, dates in the UI language with a German browser, the new default
 * offered to an old layout, and a remote browser following its own
 * light / dark mode.
 */

let server: NailServer;

test.afterEach(async () => {
  await server?.stop();
});

const rows = (page: Page) => page.locator('[data-part="message-list.row"]');

async function open(page: Page, seed: "demo" | "demo-multi" = "demo") {
  server = await startNail({ seed });
  await page.goto(server.url);
  await expect(rows(page).first()).toBeVisible();
}

/** Whether the focused element is inside `region`. */
const focusInside = (page: Page, view: Parameters<typeof viewRegion>[1]) =>
  viewRegion(page, view).evaluate((el) => el.contains(document.activeElement));

test("the rail and the shortcuts switch views; focus follows into the view", async ({ page }) => {
  await open(page);
  await expect(rail(page)).toBeVisible();
  await expect(railButton(page, "Mail")).toHaveAttribute("aria-current", "page");
  await expect(viewRegion(page, "Mail")).toBeVisible();
  await expect(railButton(page, "Calendar")).toHaveAttribute("aria-keyshortcuts", "Control+2");

  await rows(page).first().click();
  for (const [key, view] of [
    ["Control+2", "Calendar"],
    ["Control+3", "Contacts"],
    ["Control+4", "Tasks"],
    ["Control+1", "Mail"],
  ] as const) {
    await page.keyboard.press(key);
    await expect(viewRegion(page, view)).toBeVisible();
    await expect(railButton(page, view)).toHaveAttribute("aria-current", "page");
    await expect.poll(() => focusInside(page, view)).toBe(true);
  }
  // `g m` from another view; the hidden mail list kept its selection.
  await page.keyboard.press("Control+3");
  await expect(viewRegion(page, "Contacts")).toBeVisible();
  await page.keyboard.press("g");
  await page.keyboard.press("m");
  await expect(viewRegion(page, "Mail")).toBeVisible();
  await expect(rows(page).first()).toHaveAttribute("aria-selected", "true");

  // A click on the rail keeps the focus on the rail; arrow keys move on it.
  await showView(page, "Calendar");
  await expect(railButton(page, "Calendar")).toBeFocused();
  await page.keyboard.press("ArrowDown");
  await expect(railButton(page, "Contacts")).toBeFocused();
  await page.keyboard.press("Home");
  await expect(railButton(page, "Mail")).toBeFocused();
});

test("the Mail badge shows the unread inbox mail and the last view survives a reload", async ({
  page,
}) => {
  await open(page);
  const favorite = page.locator('[data-part="folder-tree.favorite"][data-favorite="all-inboxes"]');
  const count = favorite.locator('[data-part="folder-tree.item.unread-count"]');
  await expect(count).toHaveText(/^\d+$/);
  const unread = (await count.textContent())?.trim() ?? "";
  await expect(railButton(page, "Mail").locator('[data-part="nav-rail.badge"]')).toHaveText(unread);
  await expect(railButton(page, "Mail")).toHaveAccessibleName(`Mail, ${unread} unread messages`);

  await showView(page, "Tasks");
  await expect
    .poll(() => {
      try {
        return JSON.parse(readFileSync(path.join(server.configDir, "ui-state.json"), "utf8"))[
          "shell.view"
        ];
      } catch {
        return null;
      }
    })
    .toBe("tasks");
  await page.reload();
  await expect(viewRegion(page, "Tasks")).toBeVisible();
  await expect(railButton(page, "Tasks")).toHaveAttribute("aria-current", "page");
});

test("the reader bar moves actions into More when narrow and runs them from there", async ({
  page,
}) => {
  await open(page);
  await rows(page).filter({ hasText: "Welcome to Nail" }).first().click();
  const bar = readerToolbar(page);
  // Reply is the primary, labelled action; Archive is an icon with its key.
  await expect(bar.getByRole("button", { name: "Reply", exact: true })).toBeVisible();
  await expect(bar.getByRole("button", { name: "Archive" })).toHaveAttribute(
    "title",
    "Archive (E)",
  );
  // Disabled actions (Gmail labels on a local message) are hidden.
  await expect(bar.getByRole("button", { name: "Labels…" })).toHaveCount(0);

  // More from the keyboard: headings, shortcuts, and running an entry.
  const more = bar.getByRole("button", { name: "More" });
  await more.focus();
  await page.keyboard.press("Enter");
  const menu = page.getByRole("menu");
  await expect(menu).toBeVisible();
  await expect(menu.getByText("Message", { exact: true })).toBeVisible();
  await expect(menu.getByText("Turn into", { exact: true })).toBeVisible();
  await menu.getByRole("menuitem", { name: /^Mark as (read|unread)/ }).click();
  await expect(menu).toBeHidden();

  // Narrow: the highest ranks move into More, Reply stays; wide again: back.
  await page.setViewportSize({ width: 960, height: 800 });
  await expect(bar.getByRole("button", { name: "Snooze…" })).toHaveCount(0);
  await expect(bar.getByRole("button", { name: "Reply", exact: true })).toBeVisible();
  await more.click();
  await expect(page.getByRole("menuitem", { name: "Snooze…" })).toBeVisible();
  await page.keyboard.press("Escape");
  await page.setViewportSize({ width: 1600, height: 900 });
  await expect(bar.getByRole("button", { name: "Snooze…" })).toBeVisible();
});

test("filter chips narrow the list to unread or flagged mail", async ({ page }) => {
  await open(page);
  const chips = page.getByRole("group", { name: "Filter messages" });
  const all = await rows(page).count();
  await chips.getByRole("button", { name: "Unread" }).click();
  await expect(chips.getByRole("button", { name: "Unread" })).toHaveAttribute(
    "aria-pressed",
    "true",
  );
  await expect.poll(() => rows(page).count()).toBeLessThan(all);
  for (const row of await rows(page).all()) {
    await expect(row).toHaveAttribute("data-state", /unread/);
  }
  await chips.getByRole("button", { name: "All" }).click();
  await expect.poll(() => rows(page).count()).toBe(all);
});

test("settings search opens the setting; the section is remembered", async ({ page }) => {
  await open(page);
  const settings = await openSettings(page);
  const search = settings.getByRole("searchbox", { name: "Search settings" });
  await search.fill("junk");
  const results = settings.getByRole("list", { name: "Search results" });
  const section = results.locator('[data-part="settings-page.result.section"]').first();
  await expect(section).toContainText("Mail & junk");
  await expect(section.locator('[data-part="settings-page.mark"]')).toHaveText("junk");
  // A setting of the section: opens it, marks the setting and focuses it.
  const setting = results.locator('[data-part="settings-page.result.setting"]').first();
  await setting.click();
  await expect(settings.locator('[data-part="settings-page.title"]')).toHaveText("Mail & junk");
  await expect(settings.locator("[data-settings-found]")).toBeVisible();
  // Escape in the search clears it; the sections show again.
  await search.fill("");

  await openSettings(page, "Keys & encryption");
  await page.reload();
  const again = viewRegion(page, "Settings");
  await expect(again).toBeVisible();
  await expect(again.locator('[data-part="settings-page.title"]')).toHaveText("Keys & encryption");
});

test("the calendar switches views and recolours a calendar from its menu", async ({ page }) => {
  await open(page, "demo-multi");
  const calendar = await showView(page, "Calendar");
  const views = calendar.getByRole("group", { name: "View", exact: true });
  await views.getByRole("button", { name: "Free time" }).click();
  await expect(views.getByRole("button", { name: "Free time" })).toHaveAttribute(
    "aria-pressed",
    "true",
  );
  await views.getByRole("button", { name: "Week" }).click();

  const list = calendar.locator('[data-part="calendar-list"]');
  const first = list.locator('[data-part="calendar-list.item"]').first();
  const actions = first.getByRole("button", { name: /^Actions for / });
  await actions.click();
  const menu = page.getByRole("menu");
  await menu.getByRole("menuitemradio", { name: "Purple" }).click();
  await expect(menu).toBeHidden();
  // The "…" menu shows the new colour as chosen.
  await actions.click();
  await expect(menu.getByRole("menuitemradio", { name: "Purple" })).toHaveAttribute(
    "aria-checked",
    "true",
  );
  await page.keyboard.press("Escape");
  await expect(menu).toBeHidden();
});

test("dates follow the UI language, not a German browser", async ({ browser }) => {
  server = await startNail({ seed: "demo" });
  const context = await browser.newContext({ locale: "de-DE" });
  const page = await context.newPage();
  await page.goto(server.url);
  await expect(rows(page).first()).toBeVisible();
  await expect(page.locator("html")).toHaveAttribute("lang", "en");
  const dates = await rows(page).allTextContents();
  expect(dates.join(" ")).not.toMatch(/\b(vor|gestern|Uhr|Okt|Mär)\b/);
  await context.close();
});

test("an old layout keeps its main window and is offered the new default", async ({ page }) => {
  server = await startNail({ seed: "demo" });
  writeFileSync(
    path.join(server.configDir, "layout.kdl"),
    [
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
    ].join("\n"),
  );
  await page.goto(server.url);
  await expect(rows(page).first()).toBeVisible();
  await expect(rail(page)).toHaveCount(0);
  // The views still work: they open as windows.
  await page.keyboard.press("Control+2");
  await expect(page.getByRole("dialog", { name: "Calendar", exact: true })).toBeVisible();
  await page.keyboard.press("Escape");

  const offer = page.getByRole("region", { name: "A new layout with a navigation rail" });
  await expect(offer).toBeVisible();
  await offer.getByRole("button", { name: "Review the new layout" }).click();
  const merge = page.getByRole("dialog", { name: /Apply a theme or layout/ });
  await expect(merge).toBeVisible();
  const main = merge.locator('[data-part="merge-dialog.item"]', { hasText: 'window "main"' });
  await main.getByRole("radio", { name: "Take theirs" }).check();
  await merge.getByRole("button", { name: "Apply" }).click();
  await expect
    .poll(() => readFileSync(path.join(server.configDir, "layout.kdl"), "utf8"))
    .toContain("view-host");
  await merge.getByRole("button", { name: "Close" }).first().click();
  await expect(rail(page)).toBeVisible();
  await expect(offer).toHaveCount(0);
});

test("a remote browser follows its own light / dark mode", async ({ page }) => {
  server = await startNail({ seed: "demo" });
  const remote = await enableRemoteUi(server);
  const userId = await createUser(server, "ada");
  const code = await pairingCode(server, userId);

  // The desktop (light OS in seeded runs).
  await page.goto(server.url);
  await expect(rows(page).first()).toBeVisible();
  await expect(page.locator("html")).toHaveAttribute("data-nail-mode", "light");

  const browser = page.context().browser();
  if (!browser) throw new Error("no browser");
  const context = await browser.newContext({ ignoreHTTPSErrors: true, colorScheme: "dark" });
  const other = await context.newPage();
  await pairBrowser(other, remote, code);
  await other.getByRole("button", { name: "Not now" }).click();
  await expect(other.locator('[data-part="message-list.row"]').first()).toBeVisible();
  await expect(other.locator("html")).toHaveAttribute("data-nail-mode", "dark");
  // The host stays light.
  await expect(page.locator("html")).toHaveAttribute("data-nail-mode", "light");

  // The remote browser turns light: it follows.
  await other.emulateMedia({ colorScheme: "light" });
  await expect(other.locator("html")).toHaveAttribute("data-nail-mode", "light");
  await context.close();
});

test("Settings, Calendar and Tasks also open from the command palette", async ({ page }) => {
  await open(page);
  await runFromPalette(page, "tasks", "Tasks");
  await expect(viewRegion(page, "Tasks")).toBeVisible();
  await runFromPalette(page, "settings", "Settings");
  await expect(viewRegion(page, "Settings")).toBeVisible();
});
