/**
 * The M5b shell in end-to-end tests: the navigation rail ("Sections"), the
 * views it switches (each a region named "<View> view"), the settings page
 * and the command palette.
 */
import { expect, type Locator, type Page } from "@playwright/test";

export type ViewName = "Mail" | "Calendar" | "Contacts" | "Tasks" | "Settings";

/** The navigation rail. */
export const rail = (page: Page) => page.getByRole("navigation", { name: "Sections", exact: true });

/** A rail button by its view label (its name may carry a badge: "Mail, 3 unread messages"). */
export const railButton = (page: Page, name: ViewName) =>
  rail(page).getByRole("button", { name: new RegExp(`^${name}(,|$)`) });

/** The region of a view in the view host. */
export const viewRegion = (page: Page, name: ViewName) =>
  page.getByRole("region", { name: `${name} view`, exact: true });

/** Shows a view from the rail and returns its region. */
export async function showView(page: Page, name: ViewName): Promise<Locator> {
  await railButton(page, name).click();
  const region = viewRegion(page, name);
  await expect(region).toBeVisible();
  await expect(railButton(page, name)).toHaveAttribute("aria-current", "page");
  return region;
}

/** The settings page's section navigation. */
export const settingsNav = (scope: Page | Locator) =>
  scope.getByRole("navigation", { name: "Settings sections", exact: true });

/**
 * Shows the settings view at a section (by its label, e.g. "Keys &
 * encryption") and returns the view's region.
 */
export async function openSettings(page: Page, section?: string): Promise<Locator> {
  const view = await showView(page, "Settings");
  if (section !== undefined) {
    const item = settingsNav(view).getByRole("button", { name: section, exact: true });
    await item.click();
    await expect(item).toHaveAttribute("aria-current", "page");
    await expect(view.getByRole("heading", { level: 2, name: section, exact: true })).toBeVisible();
  }
  return view;
}

/** Runs an action from the command palette by searching for `query`. */
export async function runFromPalette(page: Page, query: string, option: string | RegExp) {
  await page.keyboard.press("Control+k");
  const palette = page.getByRole("dialog", { name: "Command palette" });
  await expect(palette).toBeVisible();
  await palette.getByRole("combobox").fill(query);
  await palette
    .getByRole("option", { name: typeof option === "string" ? new RegExp(`^${option}`) : option })
    .first()
    .click();
  await expect(palette).toBeHidden();
}

/** The New message button on top of the folder pane. */
export const newMessageButton = (page: Page) => page.locator('[data-part="folder-tree.compose"]');

/** An account's section in the folder pane (`data-account`). */
export const accountSection = (page: Page, accountId: string) =>
  page.locator(`[data-part="folder-tree.section"][data-account="${accountId}"]`);

/** The reader's action bar. */
export const readerToolbar = (page: Page) => page.getByRole("toolbar", { name: "Message actions" });

/** A menu entry's accessible name may end with its key ("Mark as junk Shift+J"). */
const startsWith = (name: string) =>
  new RegExp(`^${name.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}(\\s|$)`);

/**
 * Runs a toolbar action by its label: the bar's button when it shows one,
 * else the entry of the bar's "More" menu (M5b: ranked bars move what does
 * not fit, and their less used actions, into More).
 */
export async function toolbarAction(page: Page, toolbar: Locator, name: string): Promise<void> {
  const button = toolbar.getByRole("button", { name, exact: true });
  if ((await button.count()) > 0 && (await button.first().isVisible())) {
    await button.first().click();
    return;
  }
  await toolbar.getByRole("button", { name: "More", exact: true }).click();
  const menu = page.getByRole("menu");
  await expect(menu).toBeVisible();
  await menu
    .getByRole("menuitem", { name: startsWith(name) })
    .first()
    .click();
}

/** Runs a reader action (see `toolbarAction`). */
export const readerAction = (page: Page, name: string) =>
  toolbarAction(page, readerToolbar(page), name);
