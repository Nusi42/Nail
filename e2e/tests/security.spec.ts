import { readFileSync } from "node:fs";
import path from "node:path";
import { expect, test, type Locator, type Page } from "@playwright/test";
import { McpSession } from "../support/mcp";
import { control, repoRoot, startNail, type NailServer } from "../support/nail";
import {
  composerWindow,
  discardComposer,
  openSettingsTab,
  sectionInbox,
  showView,
  viewRegion,
} from "../support/views";

/**
 * End-to-end encryption and data protection (M2a, ADR 0034) against the
 * `demo-multi` mock servers. Both demo accounts have an OpenPGP key and an
 * S/MIME certificate from the test CA (`DemoMulti::seed_security`): mail
 * the "work" account sends goes to the mock SMTP server, from where the
 * test delivers it to the POP3 "home" account and reads it there.
 */

let server: NailServer;

test.beforeEach(async () => {
  server = await startNail({ seed: "demo-multi" });
});

test.afterEach(async () => {
  await server.stop();
});

interface Received {
  from: string;
  to: string[];
  data: string;
}

const pgpFixture = (name: string) =>
  readFileSync(
    path.join(repoRoot, "crates", "nail-testkit", "corpus", "crypto", "pgp", name),
    "utf8",
  );

async function openApp(page: Page) {
  await page.goto(server.url);
  await expect(page.locator('[data-part="message-list.row"]')).toHaveCount(17);
}

/** The SMTP data of the first received mail whose data matches `pattern`. */
async function receivedMatching(pattern: RegExp): Promise<string> {
  let data = "";
  await expect
    .poll(
      async () => {
        const mails = await control<Received[]>(server, "smtp-messages");
        data = mails.find((m) => pattern.test(m.data))?.data ?? "";
        return data !== "";
      },
      { timeout: 20_000 },
    )
    .toBe(true);
  return data;
}

/** Opens the composer and fills in a message. */
async function compose(page: Page, to: string, subject: string, text: string): Promise<Locator> {
  await page.keyboard.press("c");
  const dialog = composerWindow(page);
  await expect(dialog).toBeVisible();
  await dialog.getByRole("textbox", { name: "To" }).fill(to);
  await dialog.getByRole("textbox", { name: "Subject" }).fill(subject);
  await dialog.getByRole("textbox", { name: "Message" }).fill(text);
  return dialog;
}

/** Waits until the composer knows every recipient's key, then sends. */
async function sendProtected(page: Page, dialog: Locator) {
  await expect(dialog.locator('[data-part="security-compose.recipient"]').first()).toHaveAttribute(
    "data-state",
    "usable",
  );
  await dialog.getByRole("button", { name: "Send", exact: true }).click();
  await expect(dialog).toBeHidden();
  await expect(page.getByRole("status", { name: "Status" }).getByText("Message sent.")).toBeVisible(
    { timeout: 20_000 },
  );
}

/**
 * Delivers raw mail (sent by Ada from the work account) to the POP3 "home"
 * account, shows that account and returns the new message's row.
 */
async function deliverHome(page: Page, raw: string): Promise<Locator> {
  await sectionInbox(
    page.locator('[data-part="folder-tree.section"][data-account-kind="pop3"]'),
  ).click();
  const rows = page.locator('[data-part="message-list.row"]');
  await expect(rows).toHaveCount(3);
  await control(server, "pop3-deliver", { raw });
  await page.locator('[data-part="message-list"]').focus();
  await page.keyboard.press("F5");
  const row = rows.filter({ hasText: "Ada Lovelace" });
  await expect(row).toHaveCount(1, { timeout: 20_000 });
  return row;
}

const badge = (page: Page) => page.locator('button[data-part="security-badge"]');

test("a signed and encrypted OpenPGP mail between two accounts hides its subject on the wire and reads as verified", async ({
  page,
}) => {
  await openApp(page);
  const dialog = await compose(
    page,
    "ada@home.example",
    "Secret plans",
    "The rocket leaves at dawn.",
  );
  await dialog.getByRole("checkbox", { name: "Sign" }).check();
  await dialog.getByRole("checkbox", { name: "Encrypt" }).check();
  await sendProtected(page, dialog);

  // Header protection: the SMTP server saw "..." and no plain text.
  const data = await receivedMatching(/multipart\/encrypted/);
  expect(data).toContain("Subject: ...");
  expect(data).not.toContain("Secret plans");
  expect(data).not.toContain("rocket");
  expect(data).toContain("Autocrypt: addr=ada@work.example");

  const row = await deliverHome(page, data);
  await expect(row).toContainText("...");
  await row.click();
  const view = page.locator('[data-part="message-view"]');
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
  const dialog = await compose(page, "ada@home.example", "Payment", "Pay 10 EUR to Grace.");
  await dialog.getByRole("checkbox", { name: "Sign" }).check();
  await sendProtected(page, dialog);
  const data = await receivedMatching(/multipart\/signed/);
  expect(data).toContain("Pay 10 EUR");
  const row = await deliverHome(page, data.replace("Pay 10 EUR", "Pay 99 EUR"));
  await row.click();
  await expect(badge(page)).toHaveAttribute("data-signature", "invalid");
  await expect(badge(page)).toHaveAttribute("data-tone", "danger");
});

test("S/MIME signs and encrypts with the test certificates", async ({ page }) => {
  await openApp(page);
  const dialog = await compose(
    page,
    "ada@home.example",
    "Certified",
    "Greetings with a certificate.",
  );
  await dialog.getByRole("combobox", { name: "Technology" }).selectOption("smime");
  await dialog.getByRole("checkbox", { name: "Sign" }).check();
  await dialog.getByRole("checkbox", { name: "Encrypt" }).check();
  await sendProtected(page, dialog);
  const data = await receivedMatching(/application\/pkcs7-mime/);
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
  let dialog = await compose(page, "alice@example.org", "Hello Alice", "Hi!");
  const recipient = dialog.locator('[data-part="security-compose.recipient"]');
  await expect(recipient).toHaveAttribute("data-state", "missing");
  await expect(dialog.getByRole("checkbox", { name: "Encrypt" })).not.toBeChecked();
  await discardComposer(page, dialog);
  await expect(dialog).toBeHidden();

  // Alice writes once, with her key in the Autocrypt header.
  await control(server, "imap-deliver", { raw: pgpFixture("autocrypt-alice.eml") });
  await page.locator('[data-part="message-list"]').focus();
  await page.keyboard.press("F5");
  await expect(
    page.locator('[data-part="message-list.row"]').filter({ hasText: "Hi with Autocrypt" }),
  ).toHaveCount(1, { timeout: 20_000 });

  // Now both sides prefer encryption: the composer encrypts by itself.
  await expect
    .poll(
      async () => {
        dialog = await compose(page, "alice@example.org", "Hello Alice", "Hi!");
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
  const data = await receivedMatching(/alice@example\.org[\s\S]*multipart\/encrypted/);
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
  await page.locator('[data-part="message-list"]').focus();
  await page.keyboard.press("F5");
  const row = page
    .locator('[data-part="message-list.row"]')
    .filter({ hasText: "Check your account" });
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
  await page.locator('[data-part="message-list"]').focus();
  await page.keyboard.press("F5");
  const row = page
    .locator('[data-part="message-list.row"]')
    .filter({ hasText: "Checked by an unconfirmed server" });
  await expect(row).toHaveCount(1, { timeout: 20_000 });
  await row.click();
  await expect(page.locator('[data-part="message-view.header.subject"]')).toContainText(
    "Checked by an unconfirmed server",
  );
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
  await page
    .locator('[data-part="message-list.row"]')
    .filter({ hasText: "Checked by an unconfirmed server" })
    .click();
  await expect(page.locator('button[data-part="auth-badge"]')).toHaveText("Sender confirmed");
});

test("encryption at rest can be switched on and off with the data intact", async ({ page }) => {
  await openApp(page);
  const row = page
    .locator('[data-part="message-list.row"]')
    .filter({ hasText: "Release notes for M1" })
    .first();
  await row.click();
  const subject = page.locator('[data-part="message-view.header.subject"]');
  await expect(subject).toContainText("Release notes for M1");

  await showView(page, "Settings");
  const settings = viewRegion(page, "Settings");
  await openSettingsTab(page, "Data protection");
  const state = settings.locator('[data-part="security-vault.state"]');
  await expect(state).toHaveAttribute("data-state", "plain");
  await settings.getByRole("button", { name: "Turn on encryption at rest" }).click();
  await settings.getByRole("button", { name: "Encrypt now" }).click();
  await expect(state).toHaveAttribute("data-state", "encrypted", { timeout: 30_000 });
  await showView(page, "Mail");

  // Everything still reads and searches.
  await page
    .locator('[data-part="message-list.row"]')
    .filter({ hasText: "Release notes" })
    .first()
    .click();
  await expect(subject).toContainText("Release notes for M1");
  const search = page.getByRole("searchbox");
  await search.fill("unified");
  await search.press("Enter");
  await expect(
    page.locator('[data-part="message-list.row"]').filter({ hasText: "Release notes" }),
  ).not.toHaveCount(0, { timeout: 20_000 });
  await search.press("Escape");

  await showView(page, "Settings");
  await openSettingsTab(page, "Data protection");
  await settings.getByRole("button", { name: "Turn off" }).click();
  await settings.getByRole("button", { name: "Turn off and decrypt" }).click();
  await expect(state).toHaveAttribute("data-state", "plain", { timeout: 30_000 });
  await showView(page, "Mail");
  await page
    .locator('[data-part="message-list.row"]')
    .filter({ hasText: "Release notes" })
    .first()
    .click();
  await expect(subject).toContainText("Release notes for M1");
});

test("an MCP client reads encrypted content only with its separate grant", async ({ page }) => {
  await openApp(page);
  const dialog = await compose(page, "ada@home.example", "Numbers", "The PIN is 4711.");
  await dialog.getByRole("checkbox", { name: "Encrypt" }).check();
  await sendProtected(page, dialog);
  // The Sent copy is encrypted to the sender too.
  await page
    .locator('[data-part="folder-tree.section"][data-account-kind="imap"]')
    .locator('[data-part="folder-tree.item"][data-role="sent"]')
    .click();
  const sent = page.locator('[data-part="message-list.row"]').filter({ hasText: "..." }).first();
  await expect(sent).toBeVisible({ timeout: 20_000 });
  const messageId = await sent.getAttribute("data-message");
  expect(messageId).toBeTruthy();

  await showView(page, "Settings");
  const settings = viewRegion(page, "Settings");
  await openSettingsTab(page, "MCP clients");
  const serverLine = settings.locator('[data-part="mcp-clients.server"]');
  await expect(serverLine).toHaveAttribute("data-state", "running");
  const url = /(http:\/\/127\.0\.0\.1:\d+\/mcp)/.exec((await serverLine.textContent()) ?? "")?.[1];
  await settings.getByRole("textbox", { name: "Client name" }).fill("Agent");
  await settings.getByRole("button", { name: "Create client" }).click();
  const token = (await settings.locator('[data-part="mcp-token.value"]').textContent())?.trim();
  const form = settings.getByRole("form", { name: "Access of Agent" });
  await form.getByRole("checkbox", { name: "Read mail" }).check();
  await form.getByRole("checkbox", { name: /All accounts/ }).check();
  await form.getByRole("button", { name: "Save access" }).click();
  await expect(form.getByRole("status")).toHaveText("Access saved.");

  const mcp = new McpSession(url!, token!);
  await mcp.open();
  const denied = await mcp.request("tools/call", {
    name: "get_message",
    arguments: { message_id: messageId },
  });
  expect(JSON.stringify(denied)).toMatch(/denied/);
  expect(JSON.stringify(denied)).toMatch(/encrypted/);
  expect(JSON.stringify(denied)).not.toContain("4711");

  await form.getByRole("checkbox", { name: "Encrypted messages" }).check();
  await form.getByRole("button", { name: "Save access" }).click();
  await expect(form.getByRole("status")).toHaveText("Access saved.");
  const allowed = await mcp.request("tools/call", {
    name: "get_message",
    arguments: { message_id: messageId },
  });
  expect(allowed.error).toBeUndefined();
  expect(JSON.stringify(allowed.result)).toContain("The PIN is 4711.");
});
