import { readFileSync } from "node:fs";
import path from "node:path";
import { expect, test, type Locator, type Page } from "@playwright/test";
import { McpSession, createMcpClient, grantMcpAccess } from "../support/mcp";
import { nailPerTest, control, repoRoot } from "../support/nail";
import {
  discardComposer,
  openSettingsTab,
  READER_SUBJECT,
  sectionInbox,
  showView,
  viewRegion,
  messageRows,
  openMail,
  DEMO_MULTI_CONVERSATIONS,
  refreshList,
  messageRow,
} from "../support/views";
import { newMessage, sendMessage, smtpMessageMatching } from "../support/compose";

/**
 * End-to-end encryption and data protection (M2a, ADR 0034) against the
 * `demo-multi` mock servers. Both demo accounts have an OpenPGP key and an
 * S/MIME certificate from the test CA (`DemoMulti::seed_security`): mail
 * the "work" account sends goes to the mock SMTP server, from where the
 * test delivers it to the POP3 "home" account and reads it there.
 */

const server = nailPerTest({ seed: "demo-multi" });

const pgpFixture = (name: string) =>
  readFileSync(
    path.join(repoRoot, "crates", "nail-testkit", "corpus", "crypto", "pgp", name),
    "utf8",
  );

const openApp = (page: Page) => openMail(page, server.url, { rows: DEMO_MULTI_CONVERSATIONS });

/** Waits until the composer knows every recipient's key, then sends. */
async function sendProtected(page: Page, dialog: Locator) {
  await expect(dialog.locator('[data-part="security-compose.recipient"]').first()).toHaveAttribute(
    "data-state",
    "usable",
  );
  await sendMessage(page, dialog);
}

/**
 * Delivers raw mail (sent by Ada from the work account) to the POP3 "home"
 * account, shows that account and returns the new message's row.
 */
async function deliverHome(page: Page, raw: string): Promise<Locator> {
  await sectionInbox(
    page.locator('[data-part="folder-tree.section"][data-account-kind="pop3"]'),
  ).click();
  const rows = messageRows(page);
  await expect(rows).toHaveCount(3);
  await control(server, "pop3-deliver", { raw });
  await refreshList(page);
  const row = rows.filter({ hasText: "Ada Lovelace" });
  await expect(row).toHaveCount(1, { timeout: 20_000 });
  return row;
}

/** Signs and encrypts the composer's message and sends it. */
async function signEncryptAndSend(page: Page, dialog: Locator): Promise<void> {
  await dialog.getByRole("checkbox", { name: "Sign" }).check();
  await dialog.getByRole("checkbox", { name: "Encrypt" }).check();
  await sendProtected(page, dialog);
}

const badge = (page: Page) => page.locator('button[data-part="security-badge"]');

test("a signed and encrypted OpenPGP mail between two accounts hides its subject on the wire and reads as verified", async ({
  page,
}) => {
  await openApp(page);
  const dialog = await newMessage(page, {
    to: "ada@home.example",
    subject: "Secret plans",
    text: "The rocket leaves at dawn.",
  });
  await signEncryptAndSend(page, dialog);

  // Header protection: the SMTP server saw "..." and no plain text.
  const data = await smtpMessageMatching(server, /multipart\/encrypted/);
  expect(data).toContain("Subject: ...");
  expect(data).not.toContain("Secret plans");
  expect(data).not.toContain("rocket");
  expect(data).toContain("Autocrypt: addr=ada@work.example");

  const row = await deliverHome(page, data);
  await expect(row).toContainText("...");
  await row.click();
  const view = page.locator('[data-part="message-view"]');
  // The stored subject is the placeholder "..." (the feed's heading); the
  // card shows the decrypted one because it differs.
  await expect(view.locator('[data-part="message-view.header.subject"]')).toContainText(
    "Secret plans",
  );
  await expect(badge(page)).toHaveAttribute("data-encryption", "decrypted");
  await expect(badge(page)).toHaveAttribute("data-signature", "verified");
  await expect(badge(page)).toHaveAttribute("data-tone", "good");
  await expect(view.locator('[data-part="message-view.text"]')).toContainText(
    "The rocket leaves at dawn.",
  );

  // The details name the header protection.
  await badge(page).click();
  const details = page.locator('[data-part="security-badge.popover"]');
  await expect(details).toBeVisible();
  await expect(details.locator('[data-part="security-details.headers"]')).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(details).toBeHidden();
});

test("a tampered signature shows as invalid", async ({ page }) => {
  await openApp(page);
  const dialog = await newMessage(page, {
    to: "ada@home.example",
    subject: "Payment",
    text: "Pay 10 EUR to Grace.",
  });
  await dialog.getByRole("checkbox", { name: "Sign" }).check();
  await sendProtected(page, dialog);
  const data = await smtpMessageMatching(server, /multipart\/signed/);
  expect(data).toContain("Pay 10 EUR");
  const row = await deliverHome(page, data.replace("Pay 10 EUR", "Pay 99 EUR"));
  await row.click();
  await expect(badge(page)).toHaveAttribute("data-signature", "invalid");
  await expect(badge(page)).toHaveAttribute("data-tone", "danger");
});

test("S/MIME signs and encrypts with the test certificates", async ({ page }) => {
  await openApp(page);
  const dialog = await newMessage(page, {
    to: "ada@home.example",
    subject: "Certified",
    text: "Greetings with a certificate.",
  });
  await dialog.getByRole("combobox", { name: "Technology" }).selectOption("smime");
  await signEncryptAndSend(page, dialog);
  const data = await smtpMessageMatching(server, /application\/pkcs7-mime/);
  expect(data).toContain("Subject: Certified");
  expect(data).not.toContain("Greetings with a certificate");

  const row = await deliverHome(page, data);
  await row.click();
  await expect(badge(page)).toHaveAttribute("data-technology", "smime");
  await expect(badge(page)).toHaveAttribute("data-encryption", "decrypted");
  await expect(badge(page)).toHaveAttribute("data-signature", "verified");
  await expect(page.locator('[data-part="message-view.text"]')).toContainText(
    "Greetings with a certificate.",
  );
  await badge(page).click();
  await expect(page.locator('[data-part="security-chain"]')).toHaveAttribute("data-state", "valid");
});

test("Autocrypt turns on encryption after one received message", async ({ page }) => {
  await openApp(page);
  // Before: nothing is known about Alice.
  let dialog = await newMessage(page, {
    to: "alice@example.org",
    subject: "Hello Alice",
    text: "Hi!",
  });
  const recipient = dialog.locator('[data-part="security-compose.recipient"]');
  await expect(recipient).toHaveAttribute("data-state", "missing");
  await expect(dialog.getByRole("checkbox", { name: "Encrypt" })).not.toBeChecked();
  await discardComposer(page, dialog);
  await expect(dialog).toBeHidden();

  // Alice writes once, with her key in the Autocrypt header.
  await control(server, "imap-deliver", { raw: pgpFixture("autocrypt-alice.eml") });
  await refreshList(page);
  await expect(messageRows(page).filter({ hasText: "Hi with Autocrypt" })).toHaveCount(1, {
    timeout: 20_000,
  });

  // Now both sides prefer encryption: the composer encrypts by itself.
  await expect
    .poll(
      async () => {
        dialog = await newMessage(page, {
          to: "alice@example.org",
          subject: "Hello Alice",
          text: "Hi!",
        });
        const state = await dialog
          .locator('[data-part="security-compose.autocrypt"]')
          .getAttribute("data-state", { timeout: 3_000 })
          .catch(() => null);
        if (state !== "encrypt") {
          await discardComposer(page, dialog);
          await expect(dialog).toBeHidden();
        }
        return state;
      },
      { timeout: 20_000 },
    )
    .toBe("encrypt");
  await expect(dialog.getByRole("checkbox", { name: "Encrypt" })).toBeChecked();
  await sendProtected(page, dialog);
  const data = await smtpMessageMatching(server, /alice@example\.org[\s\S]*multipart\/encrypted/);
  expect(data).not.toContain("Hi!");
});

test("the sender check badge and a look-alike link warning", async ({ page }) => {
  await openApp(page);
  const raw = [
    "Received: from mail.example.org by mx.nail.example; Tue, 29 Sep 2026 08:00:00 +0000",
    "Authentication-Results: mx.nail.example; dkim=pass header.d=example.org header.s=s1; spf=pass smtp.mailfrom=example.org; dmarc=pass header.from=example.org",
    "From: Grace <grace@example.org>",
    "To: ada@work.example",
    "Subject: Check your account",
    "Date: Tue, 29 Sep 2026 08:00:00 +0000",
    "Message-ID: <lookalike@example.org>",
    "MIME-Version: 1.0",
    "Content-Type: text/html; charset=utf-8",
    "",
    '<p>Please check <a href="https://xn--pple-43d.com/login">your account</a>.</p>',
    "",
  ].join("\r\n");
  await control(server, "imap-deliver", { raw });
  await refreshList(page);
  const row = messageRows(page).filter({ hasText: "Check your account" });
  await expect(row).toHaveCount(1, { timeout: 20_000 });
  await row.click();

  const auth = page.locator('button[data-part="auth-badge"]');
  await expect(auth).toHaveText("Sender confirmed");
  await auth.click();
  await expect(page.locator('[data-part="auth-details.server"]')).toContainText("mx.nail.example");
  await page.keyboard.press("Escape");

  const warnings = page.locator('[data-part="security-warnings"]');
  await expect(warnings).toBeVisible();
  await expect(warnings.locator('[data-part="security-warnings.target"]')).toContainText(
    "xn--pple-43d.com",
  );
  // Plain mail has no end-to-end badge.
  await expect(badge(page)).toHaveCount(0);
});

test("sender checks count only for a receiving server the user configured", async ({ page }) => {
  await openApp(page);
  const openAccountSettings = async () => {
    await showView(page, "Settings");
    const settings = viewRegion(page, "Settings");
    await openSettingsTab(page, "Accounts");
    await settings
      .getByRole("list", { name: "Accounts" })
      .getByRole("button", { name: /Work/ })
      .click();
    return settings;
  };
  // Remove the configured server: nothing is trusted any more.
  let settings = await openAccountSettings();
  const servers = settings.getByRole("textbox", {
    name: "Trusted receiving servers (authserv-id)",
  });
  await expect(servers).toHaveValue("mx.nail.example");
  await servers.fill("");
  await settings.getByRole("button", { name: "Save" }).click();
  await expect(settings.getByText("Settings saved.")).toBeVisible();
  // One message is not enough to suggest a server.
  await settings.getByRole("button", { name: "Suggest from recent mail" }).click();
  await expect(
    settings.getByText("Your recent mail names no receiving server Nail could suggest."),
  ).toBeVisible();
  await showView(page, "Mail");

  const raw = [
    "Received: from mail.example.org by mx.nail.example; Tue, 29 Sep 2026 08:00:00 +0000",
    "Authentication-Results: mx.nail.example; dkim=pass header.d=example.org; spf=pass smtp.mailfrom=example.org; dmarc=pass header.from=example.org",
    "From: Grace <grace@example.org>",
    "To: ada@work.example",
    "Subject: Checked by an unconfirmed server",
    "Date: Tue, 29 Sep 2026 08:00:00 +0000",
    "Message-ID: <unconfirmed@example.org>",
    "Content-Type: text/plain",
    "",
    "Hello.",
    "",
  ].join("\r\n");
  await control(server, "imap-deliver", { raw });
  await refreshList(page);
  const row = messageRows(page).filter({ hasText: "Checked by an unconfirmed server" });
  await expect(row).toHaveCount(1, { timeout: 20_000 });
  await row.click();
  await expect(page.locator(READER_SUBJECT)).toContainText("Checked by an unconfirmed server");
  await expect(page.locator('button[data-part="auth-badge"]')).toHaveCount(0);

  // The user confirms the server: the same message now shows the badge.
  settings = await openAccountSettings();
  await settings
    .getByRole("textbox", { name: "Trusted receiving servers (authserv-id)" })
    .fill("mx.nail.example");
  await settings.getByRole("button", { name: "Save" }).click();
  await expect(settings.getByText("Settings saved.")).toBeVisible();
  // Back to Mail first: the app reopens the last view after a reload.
  await showView(page, "Mail");
  await page.reload();
  await messageRows(page).filter({ hasText: "Checked by an unconfirmed server" }).click();
  await expect(page.locator('button[data-part="auth-badge"]')).toHaveText("Sender confirmed");
});

/** The mail view still opens "Release notes for M1". */
async function expectReleaseNotes(page: Page): Promise<void> {
  await showView(page, "Mail");
  await messageRow(page, "Release notes").first().click();
  await expect(page.locator(READER_SUBJECT)).toContainText("Release notes for M1");
}

test("encryption at rest can be switched on and off with the data intact", async ({ page }) => {
  await openApp(page);
  const row = messageRows(page).filter({ hasText: "Release notes for M1" }).first();
  await row.click();
  const subject = page.locator(READER_SUBJECT);
  await expect(subject).toContainText("Release notes for M1");

  await showView(page, "Settings");
  const settings = viewRegion(page, "Settings");
  await openSettingsTab(page, "Data protection");
  const state = settings.locator('[data-part="security-vault.state"]');
  await expect(state).toHaveAttribute("data-state", "plain");
  await settings.getByRole("button", { name: "Turn on encryption at rest" }).click();
  await settings.getByRole("button", { name: "Encrypt now" }).click();
  await expect(state).toHaveAttribute("data-state", "encrypted", { timeout: 30_000 });
  // Everything still reads and searches.
  await expectReleaseNotes(page);
  const search = page.getByRole("searchbox");
  await search.fill("unified");
  await search.press("Enter");
  await expect(messageRows(page).filter({ hasText: "Release notes" })).not.toHaveCount(0, {
    timeout: 20_000,
  });
  await search.press("Escape");

  await showView(page, "Settings");
  await openSettingsTab(page, "Data protection");
  await settings.getByRole("button", { name: "Turn off" }).click();
  await settings.getByRole("button", { name: "Turn off and decrypt" }).click();
  await expect(state).toHaveAttribute("data-state", "plain", { timeout: 30_000 });
  await expectReleaseNotes(page);
});

test("an MCP client reads encrypted content only with its separate grant", async ({ page }) => {
  await openApp(page);
  const dialog = await newMessage(page, {
    to: "ada@home.example",
    subject: "Numbers",
    text: "The PIN is 4711.",
  });
  await dialog.getByRole("checkbox", { name: "Encrypt" }).check();
  await sendProtected(page, dialog);
  // The Sent copy is encrypted to the sender too.
  await page
    .locator('[data-part="folder-tree.section"][data-account-kind="imap"]')
    .locator('[data-part="folder-tree.item"][data-role="sent"]')
    .click();
  const sent = messageRows(page).filter({ hasText: "..." }).first();
  await expect(sent).toBeVisible({ timeout: 20_000 });
  const messageId = await sent.getAttribute("data-message");
  expect(messageId).toBeTruthy();

  const client = await createMcpClient(page, "Agent");
  await grantMcpAccess(client.access, ["Read mail"]);
  const mcp = new McpSession(client.url, client.token);
  await mcp.open();
  const denied = await mcp.request("tools/call", {
    name: "get_message",
    arguments: { message_id: messageId },
  });
  expect(JSON.stringify(denied)).toMatch(/denied/);
  expect(JSON.stringify(denied)).toMatch(/encrypted/);
  expect(JSON.stringify(denied)).not.toContain("4711");

  await grantMcpAccess(client.access, ["Encrypted messages"]);
  const allowed = await mcp.request("tools/call", {
    name: "get_message",
    arguments: { message_id: messageId },
  });
  expect(allowed.error).toBeUndefined();
  expect(JSON.stringify(allowed.result)).toContain("The PIN is 4711.");
});
