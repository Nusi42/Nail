import { expect, type Locator, type Page } from "@playwright/test";
import { control, type NailServer } from "./nail";
import { openSettings, showView } from "./views";

/**
 * ADR 0190: the connection log and error details of the `demo-multi` IMAP
 * account "Work" (`work`), shared by `connlog.spec.ts`, the a11y window
 * sweep and the P1 window checks.
 */

/** The viewer window of the account `work`. */
export const workLogWindow = (page: Page) =>
  page.locator('[data-part="window"][data-window="connection-log:account-work"]');

/** The newest error details window. */
export const errorDetailsWindow = (page: Page) =>
  page.locator('[data-part="window"][data-window^="error-details:"]').last();

/** Settings → Accounts → Work: the account's "Connection" section. */
export async function workConnection(page: Page): Promise<Locator> {
  const settings = await openSettings(page, "Accounts");
  await settings.getByRole("button", { name: /^Work/ }).click();
  const connection = settings.getByRole("region", { name: "Connection" });
  await expect(connection).toBeVisible();
  return connection;
}

/** Opens the connection log of Work from its settings. */
export async function openWorkLog(page: Page): Promise<Locator> {
  const connection = await workConnection(page);
  await connection.getByRole("button", { name: "Connection log" }).click();
  const viewer = workLogWindow(page);
  await expect(viewer).toBeVisible();
  return viewer;
}

/** A top-level mailbox of Work other than the inbox. */
export async function otherMailbox(server: NailServer): Promise<string> {
  const mailboxes = await control<{ name: string; count: number }[]>(server, "imap-mailboxes", {
    account: "work",
  });
  const name = mailboxes.map((m) => m.name).find((n) => n !== "INBOX" && !n.includes("/"));
  expect(name, "a second mailbox of work").toBeDefined();
  return name ?? "";
}

/** Syncs every account now (F5 in the message list). */
async function syncNow(page: Page): Promise<void> {
  await showView(page, "Mail");
  await page.locator('[data-part="message-list"]').focus();
  await page.keyboard.press("F5");
}

/**
 * Makes the mock server refuse to open `folder` of Work, syncs, and opens the
 * status bar's "Show details". Returns the details window.
 */
export async function showRefusedFolderDetails(
  page: Page,
  server: NailServer,
  folder: string,
): Promise<Locator> {
  await control(server, "imap-deny", { account: "work", mailboxes: [folder] });
  await syncNow(page);
  const status = page.locator('[data-item="sync-progress"]');
  await expect(status).toHaveAttribute("data-state", "error");
  await status.getByRole("button", { name: "Show details" }).click();
  const details = errorDetailsWindow(page);
  await expect(details).toBeVisible();
  return details;
}

/** Lets the mock server open every folder again and syncs (the error goes). */
export async function allowAllFolders(page: Page, server: NailServer): Promise<void> {
  await control(server, "imap-deny", { account: "work", mailboxes: [] });
  await syncNow(page);
  await expect(page.locator('[data-item="sync-progress"]')).not.toHaveAttribute(
    "data-state",
    "error",
  );
}
