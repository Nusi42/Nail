import { expect, test, type Page } from "@playwright/test";
import { nailPerTest, screenshotFile } from "../support/nail";
import { READER_SUBJECT, messageRows, showView } from "../support/views";

/**
 * ADR 0194: "Show source" — a card's "More" menu and Ctrl+U open the
 * message's source: the original as received (every header, the body still
 * quoted-printable), the decoded view (the HTML as its author wrote it),
 * Wrap lines and Copy all. The corpus's garden digest is quoted-printable
 * HTML with remote images; showing its source loads none of them.
 */

const DIGEST = "Your weekly garden digest 🌱";

async function openDigest(page: Page): Promise<void> {
  const row = messageRows(page).filter({ hasText: DIGEST }).first();
  await expect(row).toBeVisible({ timeout: 30_000 });
  await row.click();
  await expect(page.locator(READER_SUBJECT)).toHaveText(DIGEST);
}

const sourceWindow = (page: Page) =>
  page.locator('[data-part="window"][data-window^="message-source:"]');

test.describe("Show source (demo)", () => {
  const server = nailPerTest({ seed: "demo" });

  test("a card's menu shows the original and the decoded source and copies it", async ({
    page,
  }) => {
    await page.context().grantPermissions(["clipboard-read", "clipboard-write"]);
    const remote: string[] = [];
    page.on("request", (request) => {
      if (request.url().includes("garden-weekly.example")) remote.push(request.url());
    });
    await page.goto(server.url);
    await showView(page, "Mail");
    await openDigest(page);
    const card = page.locator('[data-part="thread-view.card"]').last();
    await card.getByRole("button", { name: "More actions for this message" }).click();
    await page.getByRole("menu").getByRole("menuitem", { name: "Show source" }).click();

    const win = sourceWindow(page);
    await expect(win).toHaveCount(1);
    await expect(win.getByRole("heading", { name: "Message source" })).toBeVisible();
    const text = win.getByLabel("Source of the message");
    // As received: the encoded subject, every header, the body still encoded.
    await expect(text).toContainText("Subject: =?utf-8?q?Your_weekly_garden_digest_=F0=9F=8C=B1?=");
    await expect(text).toContainText("List-Unsubscribe: <https://garden-weekly.example/");
    await expect(text).toContainText('<body bgcolor=3D"#f4f1e8">');

    await page.screenshot({ path: screenshotFile("message-source", "original") });
    await win.getByRole("button", { name: "Decoded" }).click();
    await expect(text).toContainText('<body bgcolor="#f4f1e8">');
    await expect(text).toContainText(`Subject: ${DIGEST}`);
    await expect(text).toContainText("Content-Transfer-Encoding: quoted-printable");
    await page.screenshot({ path: screenshotFile("message-source", "decoded") });

    await win.getByRole("checkbox", { name: "Wrap lines" }).uncheck();
    await expect(win.locator('[data-part="message-source"]')).not.toHaveAttribute(
      "data-state",
      /wrapped/,
    );
    await win.getByRole("button", { name: "Copy all" }).click();
    await expect(win.getByText("Copied to the clipboard.")).toBeVisible();
    const copied = await page.evaluate(() => navigator.clipboard.readText());
    expect(copied).toContain("Message-ID: <digest-2026-39@garden-weekly.example>");
    expect(copied).toContain("<h1>Autumn is here</h1>");

    await win.locator('[data-part="form.button"][data-action="close"]').click();
    await expect(win).toHaveCount(0);
    expect(remote).toEqual([]);
  });

  test("Ctrl+U opens the selected message's source once", async ({ page }) => {
    await page.goto(server.url);
    await showView(page, "Mail");
    await openDigest(page);
    await messageRows(page).filter({ hasText: DIGEST }).first().focus();
    await page.keyboard.press("Control+u");
    await expect(sourceWindow(page)).toHaveCount(1);
    await expect(sourceWindow(page).getByLabel("Source of the message")).toContainText(
      "Message-ID: <digest-2026-39@garden-weekly.example>",
    );
    // Again: the open window comes to the front, no second one.
    await messageRows(page).filter({ hasText: DIGEST }).first().focus();
    await page.keyboard.press("Control+u");
    await expect(sourceWindow(page)).toHaveCount(1);
  });
});
