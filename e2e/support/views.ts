/**
 * The M5b shell in end-to-end tests: the navigation rail ("Sections"), the
 * views it switches (each a region named "<View> view"), the settings page
 * and the command palette.
 */
import { expect, type Locator, type Page } from "@playwright/test";

export type ViewName = "Mail" | "Calendar" | "Contacts" | "Tasks" | "Settings";

/**
 * The subject the reader shows (P2): the card feed's heading (the
 * conversation's subject), or the message's own subject when it shows alone.
 */
export const READER_SUBJECT =
  ':is([data-part="thread-view.subject"], [data-part="message-view.header.subject"])';

/** The rows of the message list (of every list on the page). */
export const messageRows = (scope: Page | Locator) =>
  scope.locator('[data-part="message-list.row"]');

/** The rows of the conversation (or message) `subject`. */
export const messageRow = (scope: Page | Locator, subject: string) =>
  messageRows(scope).filter({ hasText: subject });

/** The message list (focus it for the list's keys). */
export const messageList = (page: Page) => page.locator('[data-part="message-list"]');

/** Fetches new mail now: F5 in the message list. */
export async function refreshList(page: Page): Promise<void> {
  await messageList(page).focus();
  await page.keyboard.press("F5");
}

/** Moves the selected messages into `folder` (key `v` in the list, then the folder picker). */
export async function moveSelectionTo(page: Page, folder: string): Promise<void> {
  await messageList(page).focus();
  await page.keyboard.press("v");
  const move = page.getByRole("dialog", { name: "Move to folder" });
  await move.getByRole("button", { name: folder, exact: true }).click();
}

/**
 * Settings → Vacation reply: switches the reply of `account` on with
 * `message` and saves.
 */
export async function saveVacationReply(
  page: Page,
  account: string,
  message: string,
): Promise<void> {
  const settings = await openSettingsTab(page, "Vacation reply");
  const vacation = settings.locator('[data-part="vacation-editor"]');
  await vacation.getByRole("combobox", { name: "Account" }).selectOption({ label: account });
  await vacation.getByRole("checkbox", { name: /Send a vacation reply/ }).check();
  await vacation.getByRole("textbox", { name: "Message" }).fill(message);
  await vacation.getByRole("button", { name: "Save", exact: true }).click();
}

/** In the open "Apply labels" dialog: checks `label` and applies. */
export async function applyLabel(page: Page, label: string): Promise<void> {
  const apply = page.getByRole("dialog", { name: "Apply labels" });
  await apply.getByRole("checkbox", { name: label }).check();
  await apply.getByRole("button", { name: "Apply" }).click();
  await expect(apply).toBeHidden();
}

/** The navigation rail. */
export const rail = (page: Page) => page.getByRole("navigation", { name: "Sections", exact: true });

/**
 * The first load of the app from a freshly started `nail serve`: a cold
 * bundle, the seeded core's first answers and a busy machine can take far
 * longer than the default 5 s expect timeout (P1 review: a known flake), so
 * the wait for the shell is explicit and generous. Later waits keep the
 * default.
 */
export const FIRST_LOAD_TIMEOUT = 30_000;

/** Opens the app at `url` and waits until the shell (the navigation rail) shows. */
export async function openApp(page: Page, url: string): Promise<void> {
  await page.goto(url);
  await expect(rail(page)).toBeVisible({ timeout: FIRST_LOAD_TIMEOUT });
}

/** Conversations in the unified inbox of `--seed demo-multi` (P2's demo threads included). */
export const DEMO_MULTI_CONVERSATIONS = 17;

/** A wide desktop window: the mail view shows every pane. */
export const WIDE_WINDOW = { width: 1600, height: 1000 };

/**
 * Opens the app at `url` (in a window of `size`) and waits for the message
 * list: `rows` rows, or at least one.
 */
export async function openMail(
  page: Page,
  url: string,
  options: { rows?: number; size?: { width: number; height: number } } = {},
): Promise<void> {
  if (options.size) await page.setViewportSize(options.size);
  await page.goto(url);
  const rows = messageRows(page);
  if (options.rows === undefined) {
    await expect(rows.first()).toBeVisible({ timeout: FIRST_LOAD_TIMEOUT });
  } else {
    await expect(rows).toHaveCount(options.rows, { timeout: FIRST_LOAD_TIMEOUT });
  }
}

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

/** Waits two animation frames (measured toolbars, lists and layout effects settle). */
export async function nextFrames(page: Page): Promise<void> {
  await page.evaluate(
    () =>
      new Promise<void>((resolve) => {
        requestAnimationFrame(() => {
          requestAnimationFrame(() => {
            resolve();
          });
        });
      }),
  );
}

/**
 * P3 (ADR 0164): "Delete permanently?" asks only for mail in Trash / Junk.
 * Moves the conversation `subject` to Trash and selects it there; `click`
 * are the options of both row clicks.
 */
export async function selectInTrash(
  page: Page,
  subject: string,
  click: Parameters<Locator["click"]>[0] = {},
): Promise<void> {
  const row = messageRows(page).filter({ hasText: subject }).first();
  const account = await row.getAttribute("data-account");
  await row.click(click);
  await page.keyboard.press("Delete");
  await expect(row).toHaveCount(0);
  await page
    .locator(`[data-part="folder-tree.item"][data-role="trash"][data-account="${account ?? ""}"]`)
    .first()
    .click();
  await messageRows(page).filter({ hasText: subject }).first().click(click);
}

/** Back to the unified inbox (Favorites → All inboxes). */
export async function showAllInboxes(page: Page): Promise<void> {
  await page.locator('[data-part="folder-tree.favorite"][data-favorite="all-inboxes"]').click();
}

/** Closes a window or dialog with its Close button and waits until it is gone. */
export async function closeWindow(window: Locator): Promise<void> {
  await window.getByRole("button", { name: "Close" }).first().click();
  await expect(window).toBeHidden();
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
  const gone = page.locator(`[data-part="window"][data-window="${window ?? ""}"]`);
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

/**
 * Settings → Accounts → `name` → "Synchronize this account" → Save (the demo
 * accounts that start switched off); `check` first looks at the account's
 * settings (its protocol, its options). Back in Mail it waits until the
 * account is online, opens its inbox and waits for `messages` rows.
 */
export async function syncAccount(
  page: Page,
  name: string,
  messages: number,
  check?: (settings: Locator) => Promise<void>,
): Promise<Locator> {
  const settings = await openSettingsTab(page, "Accounts");
  await settings.getByRole("button", { name: new RegExp(`^${name}`) }).click();
  if (check !== undefined) await check(settings);
  await settings.getByRole("checkbox", { name: "Synchronize this account" }).check();
  await settings.getByRole("button", { name: "Save", exact: true }).click();
  await expect(settings.getByText("Settings saved.")).toBeVisible();
  await showView(page, "Mail");
  const account = accountSectionNamed(page, name);
  await expect(account).toHaveAttribute("data-status", "online", { timeout: 30_000 });
  await sectionInbox(account).click();
  await expect(messageRows(page)).toHaveCount(messages, { timeout: 30_000 });
  return account;
}
