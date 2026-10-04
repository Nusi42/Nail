import { appendFileSync, existsSync, readFileSync, readdirSync } from "node:fs";
import path from "node:path";
import { expect, test, type Locator, type Page } from "@playwright/test";
import { startNail, type NailServer } from "../support/nail";
import {
  openSettings,
  rail,
  readerToolbar,
  runFromPalette,
  showView,
  viewRegion,
} from "../support/views";

/**
 * Wave M5d (ADRs 0131–0137) against `nail serve --seed demo`: the in-app
 * editors write `layout.kdl` / `theme.kdl` / `themes/<id>.kdl` through the
 * core and undo through its history —
 *
 * - customize-layout mode (from the rail's View menu, the command palette
 *   and Settings → Layout & toolbars): swap panes, add a widget, apply a
 *   preset, hide a pane, Reset to default, Done;
 * - the toolbar customizer (right-click a toolbar → Customize toolbar):
 *   a button into "More" with the keyboard and by dragging, Undo;
 * - Settings → Appearance: accent and corner radius into `theme.kdl`, Undo;
 * - New theme / the theme editor: a duplicated theme file, a token edit,
 *   Undo;
 * - a file changed on disk: Undo refuses, "Undo anyway" goes ahead.
 *
 * Each test runs its own server (the files are per config directory).
 */

let server: NailServer;

test.beforeEach(async () => {
  server = await startNail({ seed: "demo" });
});

test.afterEach(async () => {
  await server.stop();
});

function configFile(name: string): string {
  const file = path.join(server.configDir, name);
  return existsSync(file) ? readFileSync(file, "utf8") : "";
}

const layoutFile = () => configFile("layout.kdl");
const themeFile = () => configFile("theme.kdl");

/** Where `id="<id>"` stands in layout.kdl's mail window (-1 = not there). */
function positionOf(id: string): number {
  const text = layoutFile();
  const mail = text.indexOf('window "mail"');
  return mail < 0 ? -1 : text.indexOf(`id="${id}"`, mail);
}

async function open(page: Page) {
  await page.goto(server.url);
  await expect(page.locator('[data-part="message-list.row"]').first()).toBeVisible();
}

const editor = (page: Page) => page.getByRole("region", { name: "Customizing layout: Mail" });

/** A pane of customize mode by its name (its header names the region). */
const pane = (page: Page, name: string) =>
  page.locator('[data-part="layout-editor.pane"]').filter({
    has: page.getByRole("button", { name: `Move ${name}`, exact: true }),
  });

const palette = (page: Page) => page.getByRole("complementary", { name: "Layout palette" });

/** Customize mode through the rail's View menu (right-click). */
async function customizeFromRail(page: Page): Promise<Locator> {
  await rail(page).click({ button: "right", position: { x: 20, y: 300 } });
  const menu = page.getByRole("menu", { name: "View" });
  await expect(menu).toBeVisible();
  await menu.getByRole("menuitem", { name: "Customize layout" }).click();
  await expect(editor(page)).toBeVisible();
  return editor(page);
}

test("customize layout: swap panes, undo, hide and show, Done", async ({ page }) => {
  await open(page);
  const view = await customizeFromRail(page);
  for (const name of ["Folders", "Messages", "Message"]) {
    await expect(pane(page, name)).toBeVisible();
  }
  const undo = view.getByRole("button", { name: /^Undo/ }).first();
  await expect(undo).toBeDisabled();

  // Keyboard: pick Folders up, swap it with the reader.
  await pane(page, "Folders").getByRole("button", { name: "Move Folders", exact: true }).click();
  await page.getByRole("button", { name: "Swap Folders with Message", exact: true }).click();
  await expect(page.getByRole("status").filter({ hasText: "swapped places" })).toBeVisible();
  await expect.poll(() => positionOf("reader")).toBeGreaterThan(-1);
  expect(positionOf("reader")).toBeLessThan(positionOf("sidebar"));
  // The comment above the folder tree moved with its pane.
  expect(layoutFile()).toContain("// New message, Favorites");

  // Undo restores the file's text: there was no layout.kdl before.
  await expect(undo).toBeEnabled();
  await undo.click();
  await expect.poll(layoutFile).toBe("");
  await expect(page.getByRole("button", { name: /^Redo/ }).first()).toBeEnabled();

  // Hide the folder pane (the eye), then show it again.
  await pane(page, "Folders").getByRole("button", { name: "Hide Folders" }).click();
  await expect.poll(layoutFile).toMatch(/id="sidebar"[^\n]*hidden=#true/);
  await pane(page, "Folders").getByRole("button", { name: "Show Folders" }).click();
  await expect.poll(layoutFile).not.toContain("hidden=#true");

  // Done leaves customize mode; the mail view works as before.
  await view.getByRole("button", { name: "Done" }).click();
  await expect(editor(page)).toBeHidden();
  await expect(page.locator('[data-part="message-list.row"]').first()).toBeVisible();
});

test("customize layout: add a widget and apply presets, Reset to default", async ({ page }) => {
  await open(page);
  await runFromPalette(page, "customize layout", "Customize layout");
  await expect(editor(page)).toBeVisible();

  // Add a widget from the palette into the folder pane.
  await palette(page).getByRole("button", { name: "Move Mini month", exact: true }).click();
  await page.getByRole("button", { name: "Add Mini month to Folders", exact: true }).click();
  await expect.poll(layoutFile).toContain("mini-month");
  await expect(pane(page, "Folders").locator('[data-part="mini-month"]')).toBeVisible();

  // Presets: Reader below puts the reader under the list; Focus hides the
  // folder and list panes.
  const presets = palette(page);
  await presets.getByRole("button", { name: /Reader below/ }).click();
  await expect(presets.getByRole("button", { name: /Reader below/ })).toHaveAttribute(
    "aria-pressed",
    "true",
  );
  await expect.poll(layoutFile).toContain('split "vertical"');
  await presets.getByRole("button", { name: /Focus/ }).click();
  await expect(presets.getByRole("button", { name: /Focus/ })).toHaveAttribute(
    "aria-pressed",
    "true",
  );
  await expect.poll(layoutFile).toMatch(/hidden=#true/);

  // Reset to default brings the three panes back (the widget goes too).
  await editor(page).getByRole("button", { name: "Reset to default" }).click();
  await expect(presets.getByRole("button", { name: /Three panes/ })).toHaveAttribute(
    "aria-pressed",
    "true",
  );
  await expect.poll(layoutFile).not.toContain("mini-month");
  await editor(page).getByRole("button", { name: "Done" }).click();

  // The settings card applies presets too and starts customize mode.
  const settings = await openSettings(page, "Layout & toolbars");
  const card = settings.getByRole("region", { name: "Window layout" });
  await card.getByRole("button", { name: /Wide reader/ }).click();
  await expect(card.getByRole("button", { name: /Wide reader/ })).toHaveAttribute(
    "aria-pressed",
    "true",
  );
  await card.getByRole("button", { name: "Customize layout" }).click();
  await expect(viewRegion(page, "Mail")).toBeVisible();
  await expect(editor(page)).toBeVisible();
});

test("customize a toolbar: right-click → Customize toolbar, into More, Undo", async ({ page }) => {
  await open(page);
  await page.locator('[data-part="message-list.row"]').first().click();
  const bar = readerToolbar(page);
  await expect(bar).toBeVisible();
  await bar.getByRole("button", { name: "Forward", exact: true }).click({ button: "right" });
  const menu = page.getByRole("menu", { name: "Toolbar" });
  await menu.getByRole("menuitem", { name: "Customize toolbar" }).click();
  const dialog = page.getByRole("dialog", { name: "Customize toolbar" });
  const barList = dialog.getByRole("listbox", { name: "Toolbar" });
  await expect(barList).toBeVisible();
  await expect(bar).toHaveAttribute("data-customizing", "");

  // Keyboard: choose Forward on the bar and move it into More (M).
  const forward = barList.getByRole("option", { name: /^Forward/ });
  await forward.click();
  await page.keyboard.press("m");
  const moreList = dialog.getByRole("listbox", { name: "More menu" });
  await expect(moreList.getByRole("option", { name: /^Forward/ })).toBeVisible();
  await expect
    .poll(() => {
      const text = layoutFile();
      const reader = text.indexOf('toolbar "reader"');
      const more = text.indexOf("more", reader);
      return reader >= 0 && more > 0 && text.indexOf('"mail.forward"', more) > more;
    })
    .toBe(true);
  await expect(bar.getByRole("button", { name: "Forward", exact: true })).toHaveCount(0);

  // Undo puts it back on the bar.
  await dialog.getByRole("button", { name: /^Undo/ }).click();
  await expect(barList.getByRole("option", { name: /^Forward/ })).toBeVisible();
  await expect(bar.getByRole("button", { name: "Forward", exact: true })).toBeVisible();

  // Dragging works too: Reply all from the bar into More.
  await barList
    .getByRole("option", { name: /^Reply all/ })
    .dragTo(moreList.getByRole("option").first());
  await expect(moreList.getByRole("option", { name: /^Reply all/ })).toBeVisible();

  // Reset to default restores the bar; Done closes the window.
  await dialog.getByRole("button", { name: "Reset to default" }).click();
  await expect(barList.getByRole("option", { name: /^Reply all/ })).toBeVisible();
  await dialog.getByRole("button", { name: "Done" }).click();
  await expect(dialog).toBeHidden();
});

test("appearance: accent and corner radius go to theme.kdl and undo", async ({ page }) => {
  await open(page);
  const settings = await openSettings(page, "Appearance");
  const appearance = settings.getByRole("region", { name: "Appearance" });

  await appearance.getByRole("textbox", { name: "Accent colour", exact: true }).fill("#aa3366");
  await appearance.getByRole("button", { name: "Apply" }).click();
  await expect.poll(themeFile).toContain('color "accent" "#aa3366"');

  // The slider writes after a pause.
  const radius = appearance.getByRole("slider", { name: "Corner radius" });
  await radius.focus();
  for (let i = 0; i < 4; i += 1) await page.keyboard.press("ArrowRight");
  await expect.poll(themeFile).toMatch(/radius "md"/);
  await expect
    .poll(() =>
      page.evaluate(() =>
        getComputedStyle(document.documentElement).getPropertyValue("--nail-radius-md").trim(),
      ),
    )
    .toMatch(/px$/);

  // Undo the radius, then the accent.
  await appearance.getByRole("button", { name: /^Undo / }).click();
  await expect.poll(themeFile).not.toMatch(/radius "md"/);
  await appearance.getByRole("button", { name: /^Undo / }).click();
  await expect.poll(themeFile).not.toContain("#aa3366");
});

test("New theme: duplicate a theme, edit a token, undo", async ({ page }) => {
  await open(page);
  const settings = await openSettings(page, "Appearance");
  const appearance = settings.getByRole("region", { name: "Appearance" });
  await appearance.getByRole("button", { name: "New theme" }).click();

  const create = page.getByRole("dialog", { name: "New theme" });
  await create.getByRole("textbox", { name: "Name" }).fill("Ocean Café");
  await expect(create.getByText("themes/ocean-cafe.kdl")).toBeVisible();
  await create.getByRole("button", { name: "Create" }).click();
  const file = path.join(server.configDir, "themes", "ocean-cafe.kdl");
  await expect.poll(() => existsSync(file)).toBe(true);
  // "Use the new theme now" is on by default: the selection names it.
  await expect.poll(themeFile).toContain("ocean-cafe");

  // The editor opens on the new theme; a token edit lands in its file.
  const themeEditor = page.getByRole("dialog", { name: "Theme editor" });
  await expect(themeEditor.getByLabel("Theme", { exact: true })).toHaveValue("ocean-cafe");
  const accent = themeEditor.getByRole("textbox", { name: "Value of color.accent", exact: true });
  await accent.fill("#117755");
  await accent.press("Enter");
  await expect.poll(() => readFileSync(file, "utf8")).toContain("#117755");
  await expect
    .poll(() =>
      page.evaluate(() =>
        getComputedStyle(document.documentElement)
          .getPropertyValue("--nail-color-accent")
          .trim()
          .toLowerCase(),
      ),
    )
    .toBe("#117755");

  // Undo takes the edit back.
  await themeEditor.getByRole("button", { name: "Undo Edit theme" }).click();
  await expect.poll(() => readFileSync(file, "utf8")).not.toContain("#117755");

  // Duplicate as new theme from the editor: a second file.
  await themeEditor.getByRole("button", { name: "Duplicate as new theme" }).click();
  await expect
    .poll(() => readdirSync(path.join(server.configDir, "themes")).sort())
    .toEqual(["ocean-cafe-copy.kdl", "ocean-cafe.kdl"]);
});

test("a file changed on disk: Undo refuses until 'Undo anyway'", async ({ page }) => {
  await open(page);
  await showView(page, "Mail");
  const view = await customizeFromRail(page);
  await pane(page, "Folders").getByRole("button", { name: "Hide Folders" }).click();
  await expect.poll(layoutFile).toContain("hidden=#true");

  // Someone edits layout.kdl outside Nail.
  appendFileSync(path.join(server.configDir, "layout.kdl"), "\n// edited by hand\n");
  await view.getByRole("button", { name: /^Undo/ }).first().click();
  await expect(page.getByRole("status").filter({ hasText: "changed outside Nail" })).toBeVisible();
  expect(layoutFile()).toContain("hidden=#true");

  await view.getByRole("button", { name: "Undo anyway" }).click();
  await expect.poll(layoutFile).not.toContain("hidden=#true");
  // The hand-edited file was backed up first (ADR 0022: `backups/`).
  const backups = path.join(server.configDir, "backups");
  const saved = readdirSync(backups)
    .filter((f) => f.startsWith("layout-"))
    .map((f) => readFileSync(path.join(backups, f), "utf8"));
  expect(saved.some((text) => text.includes("// edited by hand"))).toBe(true);
});
