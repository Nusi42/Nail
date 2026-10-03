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
    // The page's title (a widget may repeat it in its own heading).
    await expect(view.locator('[data-part="settings-page.title"]')).toHaveText(section);
  }
  return view;
}

/** Runs an action from the command palette by searching for `query`. */
export async function runFromPalette(page: Page, query: string, option: string | RegExp) {
  const palette = page.getByRole("dialog", { name: "Command palette" });
  // The key binding works once the app is loaded (the rail shows).
  await expect(rail(page).or(page.locator('[data-part="app"]')).first()).toBeVisible();
  await expect(async () => {
    if (!(await palette.isVisible())) await page.keyboard.press("Control+k");
    await expect(palette).toBeVisible({ timeout: 1_000 });
  }).toPass({ timeout: 10_000 });
  await palette.getByRole("combobox").fill(query);
  await palette
    .getByRole("option", { name: typeof option === "string" ? new RegExp(`^${option}`) : option })
    .first()
    .click();
  await expect(palette).toBeHidden();
}

/**
 * The newest composer window (M5c: every message has its own window
 * `composer:<key>`, titled by its subject once it has one; windows keep
 * their opening order in the page).
 */
export const composerWindow = (scope: Page | Locator) =>
  scope.locator('[data-part="window"][data-window^="composer:"]').last();

/** All composer windows, in the order they opened. */
export const composerWindows = (scope: Page | Locator) =>
  scope.locator('[data-part="window"][data-window^="composer:"]');

/**
 * Discards a composer: its Discard button, then — when the message has a
 * saved draft or changes (M5c) — "Discard" in the modal confirmation.
 */
export async function discardComposer(page: Page, composer: Locator): Promise<void> {
  const window = await composer.getAttribute("data-window");
  await composer.getByRole("button", { name: "Discard" }).click();
  const key = window?.split(":")[1];
  const confirm = page.locator(
    `[data-part="window"][data-window="composer-discard${key ? `:${key}` : ""}"]`,
  );
  const gone = page.locator(`[data-part="window"][data-window="${window}"]`);
  await expect(confirm.or(gone))
    .toHaveCount(1)
    .catch(() => undefined);
  await expect(async () => {
    if (await confirm.isVisible()) {
      await confirm.getByRole("button", { name: "Discard" }).click({ timeout: 1000 });
    }
    await expect(gone).toHaveCount(0, { timeout: 1000 });
  }).toPass({ timeout: 10_000 });
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

/** The settings section that holds what a pre-M5b settings tab showed. */
export const FORMER_SETTINGS_TABS: Readonly<Record<string, string>> = {
  Accounts: "Accounts",
  "Virtual accounts": "Accounts",
  Appearance: "Appearance",
  "MCP clients": "Agents (MCP)",
  Confirmations: "Agents (MCP)",
  "Keys and certificates": "Keys & encryption",
  "Signing and encryption": "Keys & encryption",
  Autocrypt: "Keys & encryption",
  Smartcards: "Keys & encryption",
  "Data protection": "Privacy & data protection",
  "Recovery key": "Privacy & data protection",
  Privacy: "Privacy & data protection",
  "Remote access": "Remote access",
  "Remote users": "Remote access",
  Devices: "Remote access",
  "Security log": "Remote access",
  "Server rules": "Server rules & vacation",
  "Exchange inbox rules": "Server rules & vacation",
  "Vacation reply": "Server rules & vacation",
  "Junk mail": "Mail & junk",
  Synchronization: "Mail & junk",
  Notifications: "Notifications",
  "Plug-ins": "Plug-ins",
  Updates: "Desktop app",
  "Tray and startup": "Desktop app",
  "Default apps": "Desktop app",
  "Import and export": "Data & backup",
  Backups: "Data & backup",
  Diagnostics: "Data & backup",
  Customize: "Layout & toolbars",
};

/** Shows the settings section of a former settings tab (see `FORMER_SETTINGS_TABS`). */
export function openSettingsTab(page: Page, tab: string): Promise<Locator> {
  const section = FORMER_SETTINGS_TABS[tab];
  if (section === undefined) throw new Error(`no settings section for the former tab ${tab}`);
  return openSettings(page, section);
}

/** The account sections of the folder pane, in account order. */
export const accountSections = (page: Page) =>
  page.locator('[data-part="folder-tree.section"][data-kind="accounts"]');

/** The folder-pane section of the account whose heading contains `name`. */
export const accountSectionNamed = (page: Page, name: string) =>
  accountSections(page).filter({
    has: page.locator('[data-part="folder-tree.section.title"]', { hasText: name }),
  });

/** A section's top-level inbox. */
export const sectionInbox = (section: Locator) =>
  section.locator('[data-part="folder-tree.item"][data-role="inbox"][data-depth="0"]').first();
