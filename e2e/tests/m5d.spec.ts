import { appendFileSync, existsSync, readFileSync, readdirSync } from "node:fs";
import path from "node:path";
import { expect, test, type Locator, type Page } from "@playwright/test";
import { defined } from "../support/defined";
import { nailPerTest } from "../support/nail";
import {
  openSettings,
  rail,
  readerToolbar,
  runFromPalette,
  showView,
  viewRegion,
  messageRows,
  openMail,
  messageList,
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
 * - a file changed on disk: Undo refuses, "Undo anyway" goes ahead;
 * - the review fixes (ADR 0138): swapped / docked panes keep their widths
 *   and limits, every dock side and "Remove from layout", Redo, one undo per
 *   double-click, Ctrl+Z / Ctrl+Y, customize mode following the shown view,
 *   Undo not overtaken by a delayed write, rename / delete of a theme,
 *   part overrides from the inspector's "Edit style", the contrast warning
 *   with its ways back (Ctrl+Z, "Reset appearance"), Appearance overrides
 *   named in the theme editor.
 *
 * Each test runs its own server (the files are per config directory).
 */

const server = nailPerTest({ seed: "demo" });

function configFile(name: string): string {
  const file = path.join(server.configDir, name);
  return existsSync(file) ? readFileSync(file, "utf8") : "";
}

const layoutFile = () => configFile("layout.kdl");
const themeFile = () => configFile("theme.kdl");
const userThemeFile = (id: string) => configFile(path.join("themes", `${id}.kdl`));
const userThemeExists = (id: string) =>
  existsSync(path.join(server.configDir, "themes", `${id}.kdl`));

/** How often `needle` occurs in `text`. */
const count = (text: string, needle: string) => text.split(needle).length - 1;

/** Whether `toolbar "reader"` in layout.kdl lists `action` after its `more`. */
function inReaderMore(action: string): boolean {
  const text = layoutFile();
  const reader = text.indexOf('toolbar "reader"');
  const more = reader < 0 ? -1 : text.indexOf("more", reader);
  return more > 0 && text.indexOf(`"${action}"`, more) > more;
}

/** A CSS custom property of the page's root. */
function rootToken(page: Page, name: string): Promise<string> {
  return page.evaluate(
    (n) => getComputedStyle(document.documentElement).getPropertyValue(n).trim().toLowerCase(),
    name,
  );
}

/** The rendered width of the layout node with this id. */
async function widthOf(page: Page, id: string): Promise<number> {
  const box = await page.locator(`[data-node-id="${id}"]`).first().boundingBox();
  return defined(box, `the box of ${id}`).width;
}

/**
 * Where `id="<id>"` stands in layout.kdl's (first breakpoint's) mail window
 * (-1 = not there; a copied default window has a narrow breakpoint too).
 */
function positionOf(id: string): number {
  const text = layoutFile();
  const mail = text.indexOf('window "mail"');
  if (mail < 0) return -1;
  const next = text.indexOf('window "mail"', mail + 1);
  const at = text.indexOf(`id="${id}"`, mail);
  return next >= 0 && at > next ? -1 : at;
}

const open = (page: Page) => openMail(page, server.url);

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

/** Keyboard: picks the folder pane up and swaps it with the reader. */
async function swapFoldersWithReader(page: Page): Promise<void> {
  await pane(page, "Folders").getByRole("button", { name: "Move Folders", exact: true }).click();
  await page.getByRole("button", { name: "Swap Folders with Message", exact: true }).click();
}

test("customize layout: swap panes, undo, hide and show, Done", async ({ page }) => {
  await open(page);
  const view = await customizeFromRail(page);
  for (const name of ["Folders", "Messages", "Message"]) {
    await expect(pane(page, name)).toBeVisible();
  }
  const undo = view.getByRole("button", { name: /^Undo/ }).first();
  await expect(undo).toBeDisabled();

  await swapFoldersWithReader(page);
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
  await expect(messageRows(page).first()).toBeVisible();
});

test("customize layout: swapped panes keep their widths and limits", async ({ page }) => {
  await open(page);
  const view = await customizeFromRail(page);
  await swapFoldersWithReader(page);
  // Each pane takes its own entries along (ADR 0138).
  await expect.poll(layoutFile).toContain('sizes="1.4fr 1fr 260px"');
  const text = layoutFile();
  expect(text).toContain('min-sizes="320px 300px 200px"');
  expect(text).toContain('max-sizes="none none 480px"');
  expect(text).toContain('collapsible="true false true"');
  await view.getByRole("button", { name: "Done" }).click();
  await messageRows(page).first().click();
  await expect(readerToolbar(page)).toBeVisible();
  // On screen: the reader is the wide pane, the folders stay narrow.
  const sidebar = await widthOf(page, "sidebar");
  const reader = await widthOf(page, "reader");
  expect(sidebar).toBeLessThanOrEqual(480);
  expect(reader).toBeGreaterThan(480);
  expect(reader).toBeGreaterThan(sidebar * 1.5);
  await expect(
    readerToolbar(page).getByRole("button", { name: "Forward", exact: true }),
  ).toBeVisible();
});

test("customize layout: dock on every side, Remove from layout, Undo and Redo", async ({
  page,
}) => {
  await open(page);
  const view = await customizeFromRail(page);
  const move = async (name: string, target: string) => {
    await pane(page, name)
      .getByRole("button", { name: `Move ${name}`, exact: true })
      .click();
    await page.getByRole("button", { name: target, exact: true }).click();
  };
  const reset = async () => {
    await view.getByRole("button", { name: "Reset to default" }).click();
    await expect.poll(() => layoutFile().includes('window "mail"')).toBe(false);
  };

  // Right of the reader: into the same split, keeping the folder width.
  await move("Folders", "Put Folders right of Message");
  await expect.poll(() => positionOf("sidebar")).toBeGreaterThan(positionOf("reader"));
  expect(layoutFile()).toContain('sizes="1fr 1.4fr 260px"');
  expect(layoutFile()).toContain('max-sizes="none none 480px"');
  await reset();

  // Below the list: a vertical split wraps both.
  await move("Message", "Put Message below Messages");
  await expect.poll(layoutFile).toContain('split "vertical"');
  expect(positionOf("list-pane")).toBeLessThan(positionOf("reader"));
  // Undo and Redo through the banner.
  await view.getByRole("button", { name: "Undo", exact: true }).click();
  await expect.poll(layoutFile).not.toContain('split "vertical"');
  const redo = view.getByRole("button", { name: "Redo", exact: true });
  await expect(redo).toBeEnabled();
  await redo.click();
  await expect.poll(layoutFile).toContain('split "vertical"');
  await reset();

  // Above the list.
  await move("Folders", "Put Folders above Messages");
  await expect.poll(layoutFile).toContain('split "vertical"');
  expect(positionOf("sidebar")).toBeLessThan(positionOf("list-pane"));
  await reset();

  // Left of the folders.
  await move("Message", "Put Message left of Folders");
  await expect.poll(() => positionOf("reader")).toBeGreaterThan(-1);
  expect(positionOf("reader")).toBeLessThan(positionOf("sidebar"));
  expect(layoutFile()).toContain('sizes="1.4fr 260px 1fr"');

  // Remove from layout.
  await pane(page, "Folders")
    .getByRole("button", { name: "Remove Folders from the layout", exact: true })
    .click();
  await expect.poll(() => positionOf("sidebar")).toBe(-1);
  expect(positionOf("reader")).toBeGreaterThan(-1);
  await expect(pane(page, "Folders")).toHaveCount(0);
});

test("customize layout: one undo per double-click, Ctrl+Z / Ctrl+Y", async ({ page }) => {
  await open(page);
  const view = await customizeFromRail(page);
  await pane(page, "Folders").getByRole("button", { name: "Hide Folders" }).click();
  await expect.poll(() => count(layoutFile(), "hidden=#true")).toBe(1);
  await pane(page, "Messages").getByRole("button", { name: "Hide Messages" }).click();
  await expect.poll(() => count(layoutFile(), "hidden=#true")).toBe(2);

  const undo = view.getByRole("button", { name: "Undo", exact: true });
  await expect(undo).toBeEnabled();
  await undo.dblclick();
  await expect.poll(() => count(layoutFile(), "hidden=#true")).toBe(1);
  await page.waitForTimeout(1_000);
  expect(count(layoutFile(), "hidden=#true")).toBe(1);

  // The keyboard inside the editor.
  await view.getByRole("heading", { name: "Customizing layout: Mail" }).focus();
  await page.keyboard.press("Control+z");
  await expect.poll(() => count(layoutFile(), "hidden=#true")).toBe(0);
  await expect(view.getByRole("button", { name: "Redo", exact: true })).toBeEnabled();
  await page.keyboard.press("Control+y");
  await expect.poll(() => count(layoutFile(), "hidden=#true")).toBe(1);
});

test("customize layout follows the view on screen", async ({ page }) => {
  await open(page);
  await customizeFromRail(page);
  // Another view: the mail view's session ends.
  await showView(page, "Calendar");
  await expect(page.locator('[data-part="layout-editor.frame"]')).toHaveCount(0);
  // One "Customize layout" customizes the calendar.
  await rail(page).click({ button: "right", position: { x: 20, y: 300 } });
  await page
    .getByRole("menu", { name: "View" })
    .getByRole("menuitem", { name: "Customize layout" })
    .click();
  await expect(page.getByRole("region", { name: "Customizing layout: Calendar" })).toBeVisible();
  await expect(page.locator('[data-part="layout-editor.frame"]')).toHaveCount(1);
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
  await messageRows(page).first().click();
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

  // Undo puts it back on the bar: the first write made layout.kdl, so its
  // undo removes the file again.
  await dialog.getByRole("button", { name: /^Undo/ }).click();
  await expect(barList.getByRole("option", { name: /^Forward/ })).toBeVisible();
  await expect(bar.getByRole("button", { name: "Forward", exact: true })).toBeVisible();
  await expect.poll(layoutFile).toBe("");

  // Dragging works too: Reply all from the bar into More.
  await barList
    .getByRole("option", { name: /^Reply all/ })
    .dragTo(moreList.getByRole("option").first());
  await expect(moreList.getByRole("option", { name: /^Reply all/ })).toBeVisible();
  await expect.poll(() => inReaderMore("mail.reply-all")).toBe(true);
  expect(inReaderMore("mail.forward")).toBe(false);

  // Reset to default restores the bar (the toolbar leaves layout.kdl);
  // Done closes the window.
  await dialog.getByRole("button", { name: "Reset to default" }).click();
  await expect(barList.getByRole("option", { name: /^Reply all/ })).toBeVisible();
  await expect.poll(() => layoutFile().includes('toolbar "reader"')).toBe(false);
  expect(inReaderMore("mail.reply-all")).toBe(false);
  await dialog.getByRole("button", { name: "Done" }).click();
  await expect(dialog).toBeHidden();
});

/** Opens Nail at Settings → Appearance and applies the accent #aa3366; the Appearance region. */
async function openWithAccent(page: Page): Promise<Locator> {
  await open(page);
  const settings = await openSettings(page, "Appearance");
  const appearance = settings.getByRole("region", { name: "Appearance" });
  await appearance.getByRole("textbox", { name: "Accent colour", exact: true }).fill("#aa3366");
  await appearance.getByRole("button", { name: "Apply" }).click();
  await expect.poll(themeFile).toContain('color "accent" "#aa3366"');
  return appearance;
}

/** The theme editor, open on the theme `id`. */
async function themeEditorOn(page: Page, id: string): Promise<Locator> {
  const themeEditor = page.getByRole("dialog", { name: "Theme editor" });
  await expect(themeEditor.getByLabel("Theme", { exact: true })).toHaveValue(id);
  return themeEditor;
}

test("appearance: accent and corner radius go to theme.kdl and undo", async ({ page }) => {
  const appearance = await openWithAccent(page);

  // The slider writes after a pause.
  const radius = appearance.getByRole("slider", { name: "Corner radius" });
  // The sliders are disabled until the core answered the Apply (the file is
  // written before the answer arrives).
  await expect(radius).toBeEnabled();
  const start = Number(await radius.inputValue());
  await radius.focus();
  for (let i = 0; i < 4; i += 1) await page.keyboard.press("ArrowRight");
  await expect.poll(themeFile).toMatch(new RegExp(`radius "md" ${String(start + 4)}\\b`));
  await expect.poll(() => rootToken(page, "--nail-radius-md")).toBe(`${String(start + 4)}px`);

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
  // A copy of the active theme (Frost) under the new name.
  const created = readFileSync(file, "utf8");
  expect(created).toMatch(/^theme "Ocean Café" extends="default"/m);
  expect(created).toMatch(/color "bg"\s+"#[0-9a-f]{6}"/);
  // "Use the new theme now" is on by default: the selection names it.
  await expect.poll(themeFile).toContain("ocean-cafe");

  // The editor opens on the new theme; a token edit lands in its file.
  const themeEditor = await themeEditorOn(page, "ocean-cafe");
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
  await expect(themeEditor.getByRole("button", { name: /^Redo / })).toBeEnabled();

  // Review (ADR 0138): Undo right after a picker change (still waiting for
  // its write) undoes that change — not an older entry, and the change does
  // not come back afterwards.
  await themeEditor.getByLabel("Pick a colour for color.bg", { exact: true }).fill("#ff0000");
  await themeEditor.getByRole("button", { name: /^Undo / }).click();
  await expect(themeEditor.getByRole("button", { name: /^Redo / })).toBeEnabled();
  await page.waitForTimeout(1_000);
  expect(readFileSync(file, "utf8")).not.toContain("#ff0000");
  expect(themeFile()).toContain("ocean-cafe");

  // Duplicate as new theme from the editor: a second file, named after the
  // first, with the same palette.
  await themeEditor.getByRole("button", { name: "Duplicate as new theme" }).click();
  await expect
    .poll(() => readdirSync(path.join(server.configDir, "themes")).sort())
    .toEqual(["ocean-cafe-copy.kdl", "ocean-cafe.kdl"]);
  const copy = userThemeFile("ocean-cafe-copy");
  expect(copy).toMatch(/^theme "Ocean Café copy" extends="default"/m);
  const original = readFileSync(file, "utf8");
  expect(copy.slice(copy.indexOf("tokens {"))).toBe(original.slice(original.indexOf("tokens {")));
  // The copy replaces the original where it was in use.
  await expect.poll(themeFile).toContain("ocean-cafe-copy");
});

/** Settings → Appearance → New theme "Ocean Café" (used now); the theme editor. */
async function newTheme(page: Page): Promise<Locator> {
  const settings = await openSettings(page, "Appearance");
  await settings
    .getByRole("region", { name: "Appearance" })
    .getByRole("button", { name: "New theme" })
    .click();
  const create = page.getByRole("dialog", { name: "New theme" });
  await create.getByRole("textbox", { name: "Name" }).fill("Ocean Café");
  await create.getByRole("button", { name: "Create" }).click();
  await expect.poll(() => userThemeExists("ocean-cafe")).toBe(true);
  await expect.poll(themeFile).toContain("ocean-cafe");
  return themeEditorOn(page, "ocean-cafe");
}

test("theme editor: rename a theme with its file, delete it, undo", async ({ page }) => {
  await open(page);
  const themeEditor = await newTheme(page);

  await themeEditor.getByRole("button", { name: "Rename", exact: true }).click();
  const rename = page.getByRole("dialog", { name: "Rename theme" });
  await rename.getByRole("textbox", { name: "Name" }).fill("Deep Sea");
  await rename.getByRole("checkbox", { name: /Also rename the file/ }).check();
  await rename.getByRole("button", { name: "Rename", exact: true }).click();
  await expect.poll(() => userThemeExists("deep-sea")).toBe(true);
  expect(userThemeExists("ocean-cafe")).toBe(false);
  expect(userThemeFile("deep-sea")).toMatch(/^theme "Deep Sea"/m);
  // The selection follows the new id.
  await expect.poll(themeFile).toContain("deep-sea");
  expect(themeFile()).not.toContain("ocean-cafe");

  // The window closes once the core answered; the editor follows the new id.
  await expect(rename).toBeHidden();
  await expect(themeEditor.getByLabel("Theme", { exact: true })).toHaveValue("deep-sea");
  await themeEditor.getByRole("button", { name: "Delete", exact: true }).click();
  const remove = page.getByRole("dialog", { name: "Delete theme?" });
  await expect(remove.getByText("Delete the theme “Deep Sea”?")).toBeVisible();
  await remove.getByRole("button", { name: "Delete", exact: true }).click();
  await expect.poll(() => userThemeExists("deep-sea")).toBe(false);
  await expect.poll(themeFile).not.toContain("deep-sea");

  // The deletion can be undone: the file comes back.
  await themeEditor.getByRole("button", { name: /^Undo / }).click();
  await expect.poll(() => userThemeExists("deep-sea")).toBe(true);
  expect(userThemeFile("deep-sea")).toMatch(/^theme "Deep Sea"/m);
});

test("theme editor: Edit style from the inspector, a part override", async ({ page }) => {
  await open(page);
  const themeEditor = await newTheme(page);
  await themeEditor.locator('[data-part="window.close"]').first().click();
  await expect(themeEditor).toBeHidden();
  await showView(page, "Mail");

  // The inspector: pick a message subject, then "Edit style".
  const subject = page.locator('[data-part="message-list.row.subject"]').first();
  await messageList(page).focus();
  await page.keyboard.press("Control+Shift+I");
  const inspector = page.getByRole("complementary", { name: "Inspector" });
  await expect(inspector).toBeVisible();
  await subject.hover();
  await subject.click();
  await expect(inspector.getByText("Showing the element you clicked.")).toBeVisible();
  await inspector.getByRole("button", { name: "Edit style" }).click();

  // The editor opens on the Parts tab at that part.
  await expect(themeEditor).toBeVisible();
  await expect(themeEditor.getByRole("tab", { name: "Parts" })).toHaveAttribute(
    "aria-selected",
    "true",
  );
  const part = themeEditor.getByRole("region", { name: "message-list.row.subject", exact: true });
  await expect(part).toBeVisible();
  await part.getByLabel("Property", { exact: true }).fill("color");
  await part.getByLabel("Value", { exact: true }).fill("#aa0000");
  await part.getByRole("button", { name: "Add", exact: true }).click();
  await expect
    .poll(() => userThemeFile("ocean-cafe"))
    .toMatch(/part "message-list\.row\.subject"[^}]*color "#aa0000"/);
  await expect
    .poll(() => subject.evaluate((el) => getComputedStyle(el).color))
    .toBe("rgb(170, 0, 0)");
});

test("theme editor: unreadable text warns; Ctrl+Z and Reset appearance lead back", async ({
  page,
}) => {
  await open(page);
  const themeEditor = await newTheme(page);
  const fgBefore = await rootToken(page, "--nail-color-fg");
  expect(fgBefore).not.toBe("#ffffff");

  const fg = themeEditor.getByRole("textbox", { name: "Value of color.fg", exact: true });
  await fg.fill("#ffffff");
  await fg.press("Enter");
  await expect.poll(() => userThemeFile("ocean-cafe")).toMatch(/color "fg"\s+"#ffffff"/);
  await expect.poll(() => rootToken(page, "--nail-color-fg")).toBe("#ffffff");
  const warning = themeEditor.getByText(/Hard to read: color\.fg on color\.bg/);
  await expect(warning).toBeVisible();
  await expect(warning).toContainText("Ctrl+Z");

  // Ctrl+Z (the focus is still in the field, nothing typed) undoes it ...
  await expect(fg).toBeFocused();
  await page.keyboard.press("Control+z");
  await expect.poll(() => rootToken(page, "--nail-color-fg")).toBe(fgBefore);
  await expect(warning).toBeHidden();
  // ... Ctrl+Y redoes it.
  await page.keyboard.press("Control+y");
  await expect.poll(() => rootToken(page, "--nail-color-fg")).toBe("#ffffff");

  // "Reset appearance" from the palette: the built-in themes again.
  await themeEditor.locator('[data-part="window.close"]').first().click();
  await runFromPalette(page, "reset appearance", "Reset appearance");
  await expect.poll(() => rootToken(page, "--nail-color-fg")).toBe(fgBefore);
  await expect.poll(themeFile).toMatch(/follow-system=#true/);
  // The theme file stays; the reset is one undo entry.
  expect(userThemeFile("ocean-cafe")).toMatch(/color "fg"\s+"#ffffff"/);
});

test("theme editor: an Appearance override is named and can be cleared", async ({ page }) => {
  await openWithAccent(page);

  const themeEditor = await newTheme(page);
  const row = themeEditor.locator('[data-part="theme-editor.token"][data-token="color.accent"]');
  await expect(row.getByText("Overridden in Appearance: #aa3366")).toBeVisible();
  const accent = row.getByRole("textbox", { name: "Value of color.accent", exact: true });
  await accent.fill("#2255ff");
  await accent.press("Enter");
  await expect.poll(() => userThemeFile("ocean-cafe")).toContain("#2255ff");
  // The override still wins in the app.
  expect(await rootToken(page, "--nail-color-accent")).toBe("#aa3366");

  await row.getByRole("button", { name: "Clear override" }).click();
  await expect.poll(themeFile).not.toContain("#aa3366");
  await expect(row.getByText(/Overridden in Appearance/)).toBeHidden();
  await expect.poll(() => rootToken(page, "--nail-color-accent")).toBe("#2255ff");
});

test("appearance: Undo right after a slider move undoes that move", async ({ page }) => {
  const appearance = await openWithAccent(page);
  const undo = appearance.getByRole("button", { name: /^Undo / });
  await expect(undo).toBeEnabled();

  const radius = appearance.getByRole("slider", { name: "Corner radius" });
  await expect(radius).toBeEnabled();
  await radius.focus();
  await page.keyboard.press("ArrowRight");
  await page.keyboard.press("ArrowRight");
  // At once, before the slider's delayed write.
  await undo.click();
  await expect(appearance.getByRole("button", { name: /^Redo / })).toBeEnabled();
  await page.waitForTimeout(1_000);
  expect(themeFile()).toContain('color "accent" "#aa3366"');
  expect(themeFile()).not.toMatch(/radius "md"/);
  await expect(appearance.getByRole("button", { name: /^Redo / })).toBeEnabled();
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
