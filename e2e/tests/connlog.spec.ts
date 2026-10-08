import { expect, test } from "@playwright/test";
import {
  allowAllFolders,
  openWorkLog,
  otherMailbox,
  showRefusedFolderDetails,
  workConnection,
  workLogWindow,
} from "../support/connlog";
import { control, startNail, type NailServer } from "../support/nail";
import { openSettings, showView } from "../support/views";

/**
 * ADR 0190: the connection log against the real core and the `demo-multi`
 * mock servers. The IMAP account "Work" (`work`) logs in, lists, opens and
 * fetches; the log shows the commands with the password redacted and the
 * messages only as `{n bytes}` — no subject of the mailbox appears in the
 * log or in its exported text. A folder the server refuses to open gives a
 * status bar error whose details name the step, the folder and the
 * server's `NO`, and lead into the connection log.
 */

let server: NailServer;

test.beforeEach(async () => {
  server = await startNail({ seed: "demo-multi" });
});

test.afterEach(async () => {
  await server.stop();
});

/** A message on a mock IMAP server, as `imap-messages` lists it. */
interface MockMessage {
  uid: number;
  subject: string;
}

test("the connection log shows the protocol, never credentials or content", async ({ page }) => {
  await page.goto(server.url);
  await showView(page, "Mail");
  const viewer = await openWorkLog(page);
  const list = viewer.getByRole("list", { name: "Recorded lines" });
  await expect(list).toContainText(/LOGIN .*<redacted>/);
  await expect(list).toContainText(/(SELECT|EXAMINE) "?INBOX"?/);
  await expect(list).toContainText(/FETCH .*\{\d+ bytes\}/);
  // The demo account's password (`MULTI_WORK_PASSWORD`) is never shown, its
  // address only masked.
  await expect(list).not.toContainText("work-secret");
  await expect(list).not.toContainText("ada@work.example");
  await expect(list).toContainText("a***@work.example");

  const subjects = (await control<MockMessage[]>(server, "imap-messages", { account: "work" }))
    .map((m) => m.subject)
    .filter((s) => s.length > 3);
  expect(subjects.length).toBeGreaterThan(5);
  const shown = (await list.textContent()) ?? "";
  for (const subject of subjects) expect(shown, subject).not.toContain(subject);

  // The exported text (a browser download outside the desktop app).
  const download = page.waitForEvent("download");
  await viewer.getByRole("button", { name: "Export…" }).click();
  const file = await download;
  expect(file.suggestedFilename()).toBe("nail-connection-log-work.txt");
  const chunks: Buffer[] = [];
  for await (const chunk of await file.createReadStream()) chunks.push(chunk as Buffer);
  const text = Buffer.concat(chunks).toString("utf8");
  expect(text).toMatch(/ imap #\d+ C: A\d+ LOGIN .*<redacted>/);
  expect(text).not.toContain("work-secret");
  for (const subject of subjects) expect(text, subject).not.toContain(subject);
});

test("a refused folder shows details and leads into the connection log", async ({ page }) => {
  const folder = await otherMailbox(server);
  await page.goto(server.url);
  const details = await showRefusedFolderDetails(page, server, folder);
  await expect(details.locator('[data-field="step"]')).toContainText("Opening the folder");
  await expect(details.locator('[data-field="folder"]')).toContainText(folder);
  await expect(details.locator('[data-field="server"]')).toContainText(/NO/);
  await expect(details.locator('[data-field="command"]')).toContainText(/(SELECT|EXAMINE) /);

  await details.getByRole("button", { name: "Show in connection log" }).click();
  const viewer = workLogWindow(page);
  await expect(viewer).toBeVisible();
  await expect(viewer.locator("[data-highlight]")).toBeVisible();

  // The error is listed with the account until a sync of the folder works.
  const connection = await workConnection(page);
  await expect(connection.getByRole("listitem").filter({ hasText: folder })).toBeVisible();
  await allowAllFolders(page, server);
  const again = await workConnection(page);
  await expect(again.getByRole("listitem").filter({ hasText: folder })).toHaveCount(0);
});

test("keeping the connection log on disk is off by default", async ({ page }) => {
  await page.goto(server.url);
  const settings = await openSettings(page, "Data & backup");
  const toggle = settings.getByRole("switch", { name: "Keep connection log on disk" });
  await expect(toggle).toHaveAttribute("aria-checked", "false");
  await toggle.click();
  await expect(toggle).toHaveAttribute("aria-checked", "true");
  await toggle.click();
  await expect(toggle).toHaveAttribute("aria-checked", "false");
  await settings.getByRole("button", { name: "Show recent lines" }).click();
  const appLog = page.locator('[data-part="window"][data-window="application-log"]');
  await expect(appLog.getByLabel("Recent application log lines")).toBeVisible();
});
