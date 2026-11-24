import { readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { expect, test, type Page } from "@playwright/test";
import { control, nailPerTest, serverMessages } from "../support/nail";
import {
  composerWindow,
  messageRow,
  messageRows,
  openMail,
  refreshList,
  DEMO_MULTI_CONVERSATIONS,
} from "../support/views";
import {
  draftInOutbox,
  newMessage,
  sendMessage,
  smtpMessageMatching,
  smtpMessages,
} from "../support/compose";

/**
 * The rich composer (ADR 0023) against the `demo-multi` mock servers: HTML
 * and Markdown mails with their MIME parts as the SMTP server received
 * them, reply quoting in both modes, and drafts autosaved to the server's
 * Drafts folder and continued after a reload, from the Outbox or from the
 * Drafts folder (also drafts written in another client).
 */

const server = nailPerTest({ seed: "demo-multi" });

const openApp = (page: Page) => openMail(page, server.url, { rows: DEMO_MULTI_CONVERSATIONS });

/** Closes an open composer window (the accounts change may not have applied yet). */
async function closeComposer(page: Page) {
  const dialog = composerWindow(page);
  if (await dialog.isVisible()) {
    await dialog.getByRole("button", { name: "Discard" }).click();
    // A message with a saved draft or changes asks first (M5c).
    const confirm = page.locator('[data-part="window"][data-window^="composer-discard"]');
    if (await confirm.isVisible()) {
      await confirm.getByRole("button", { name: "Discard draft" }).click();
    }
    await expect(page.locator('[data-part="window"][data-window^="composer"]')).toHaveCount(0);
  }
}

/** Sets the composer mode of the Work identity in accounts.kdl (the core applies hand edits). */
function setComposerMode(mode: "html" | "markdown") {
  const file = path.join(server.configDir, "accounts.kdl");
  const text = readFileSync(file, "utf8").replace(
    // Only the identity's properties change; its child block (crypto) stays.
    /identity "ada@work\.example" name="Ada Lovelace"( composer="[a-z]+")?/,
    `identity "ada@work.example" name="Ada Lovelace" composer="${mode}"`,
  );
  writeFileSync(file, text);
}

test("HTML and Markdown mails reach the SMTP server with their MIME parts", async ({ page }) => {
  await openApp(page);

  // HTML: bold text from the formatting toolbar.
  let dialog = await newMessage(page, { subject: "HTML greetings" });
  await dialog.getByRole("radio", { name: "HTML" }).click();
  await expect(dialog.getByRole("radio", { name: "HTML" })).toBeChecked();
  const html = dialog.getByRole("textbox", { name: "Message" });
  await html.click();
  await page.keyboard.type("Hello ");
  await dialog.getByRole("button", { name: "Bold" }).click();
  await page.keyboard.type("Grace");
  await sendMessage(page, dialog);
  let data = await smtpMessageMatching(server, "Subject: HTML greetings");
  expect(data).toMatch(/Content-Type: multipart\/alternative/i);
  expect(data).toMatch(/Content-Type: text\/plain/i);
  expect(data).toMatch(/Content-Type: text\/html/i);
  expect(data).toContain("<strong>Grace</strong>");

  // Markdown with the source as text/markdown and an attachment.
  dialog = await newMessage(page, { subject: "Markdown notes" });
  await dialog.getByRole("radio", { name: "Markdown" }).click();
  await expect(dialog.getByRole("radio", { name: "Markdown" })).toBeChecked();
  const source = dialog.getByRole("textbox", { name: "Message" });
  await source.click();
  await page.keyboard.type("# Review\n\nThe **plan** is ready.\n\n- one\n- two\n");
  await dialog.getByRole("checkbox", { name: "Also send the Markdown source" }).check();
  await dialog.getByLabel("Attach…").setInputFiles({
    name: "plan.txt",
    mimeType: "text/plain",
    buffer: Buffer.from("step 1\nstep 2\n"),
  });
  await expect(dialog.locator('[data-part="composer.attachment.name"]')).toHaveText("plan.txt");
  // The side-by-side preview is rendered by the core.
  await expect(dialog.getByRole("button", { name: "Preview" })).toHaveAttribute(
    "aria-pressed",
    "true",
  );
  const preview = dialog.frameLocator('[data-part="composer.preview.frame"]');
  await expect(preview.locator("h1")).toHaveText("Review");
  await sendMessage(page, dialog);
  data = await smtpMessageMatching(server, "Subject: Markdown notes");
  expect(data).toMatch(/Content-Type: multipart\/mixed/i);
  expect(data).toMatch(/Content-Type: multipart\/alternative/i);
  expect(data).toMatch(/Content-Type: text\/markdown/i);
  expect(data).toMatch(/Content-Type: text\/html/i);
  expect(data).toContain("<h1");
  expect(data).toContain("<strong>plan</strong>");
  expect(data).toContain("The **plan** is ready.");
  expect(data).toMatch(/filename="?plan\.txt/);
});

test("replies quote the original in HTML and in Markdown", async ({ page }) => {
  await openApp(page);
  const original = messageRows(page).filter({ hasText: "Welcome to Nail" }).first();

  // HTML: the original in a blockquote under a localized header.
  setComposerMode("html");
  await expect(async () => {
    await closeComposer(page);
    await original.click();
    await page.keyboard.press("r");
    const dialog = composerWindow(page);
    await expect(dialog.getByRole("radio", { name: "HTML" })).toBeChecked({ timeout: 1000 });
  }).toPass({ timeout: 15_000 });
  let dialog = composerWindow(page);
  await expect(dialog.getByRole("textbox", { name: "Subject" })).toHaveValue(/^Re: /);
  const editor = dialog.locator('[data-part="composer.html.content"]');
  await expect(editor.locator("blockquote")).toHaveCount(1);
  await expect(editor).toContainText("wrote:");
  await editor.click();
  await page.keyboard.press("Control+Home");
  await page.keyboard.type("Thanks!");
  await sendMessage(page, dialog);
  let data = await smtpMessageMatching(server, "Subject: Re: Welcome to Nail");
  expect(data).toMatch(/^In-Reply-To: </m);
  expect(data).toContain("<blockquote");

  // Markdown: `>` lines.
  setComposerMode("markdown");
  await expect(async () => {
    await closeComposer(page);
    await original.click();
    await page.keyboard.press("r");
    dialog = composerWindow(page);
    await expect(dialog.getByRole("radio", { name: "Markdown" })).toBeChecked({ timeout: 1000 });
  }).toPass({ timeout: 15_000 });
  const source = dialog.locator('[data-part="composer.markdown.content"]');
  await expect(source).toContainText("wrote:");
  await expect(source.locator(".cm-line").filter({ hasText: /^> / }).first()).toBeVisible();
  await sendMessage(page, dialog);
  data = await (async () => {
    let found = "";
    await expect
      .poll(async () => {
        const replies = (await smtpMessages(server)).filter((m) =>
          m.data.includes("Subject: Re: Welcome to Nail"),
        );
        found = replies[1]?.data ?? "";
        return found !== "";
      })
      .toBe(true);
    return found;
  })();
  expect(data).toMatch(/^> /m);
  expect(data).toContain("<blockquote");
});

test("drafts are autosaved to the server and continued after a reload", async ({ page }) => {
  await openApp(page);
  const item = await draftInOutbox(page, server, "Plans for Friday", "Half-written thoughts");
  await item.getByRole("button", { name: "Continue writing" }).click();
  const reopened = composerWindow(page);
  await expect(reopened.getByRole("textbox", { name: "Subject" })).toHaveValue("Plans for Friday");
  await expect(reopened.getByRole("textbox", { name: "Message" })).toHaveValue(
    "Half-written thoughts",
  );
  await expect(reopened.getByRole("textbox", { name: "To" })).toHaveValue("grace@navy.example");

  // Sending removes the server copy.
  await sendMessage(page, reopened);
  await expect
    .poll(async () => (await serverMessages(server, "Drafts")).map((m) => m.subject))
    .not.toContain("Plans for Friday");
});

/** The subjects in the work account's Drafts mailbox on the mock server. */
const serverDrafts = async () => (await serverMessages(server, "Drafts")).map((m) => m.subject);

test("drafts are continued from the Drafts folder, also ones written elsewhere", async ({
  page,
}) => {
  await openApp(page);
  const dialog = await newMessage(page, { subject: "Notes for Monday" });
  await dialog.getByRole("textbox", { name: "Message" }).fill("First thoughts");
  await expect(dialog.locator('[data-part="composer.saved"]')).toHaveText("Draft saved.", {
    timeout: 10_000,
  });
  await expect.poll(serverDrafts).toContain("Notes for Monday");

  // After a reload the Outbox favorite shows the waiting draft; the Drafts
  // favorite lists its server copy, and a double-click continues it.
  await page.reload();
  await expect(messageRows(page)).toHaveCount(DEMO_MULTI_CONVERSATIONS);
  await expect(
    page.locator('[data-part="folder-tree.favorite"][data-favorite="outbox"]'),
  ).toBeVisible();
  await page.locator('[data-part="folder-tree.favorite"][data-favorite="drafts"]').click();
  await messageRow(page, "Notes for Monday").dblclick();
  const reopened = composerWindow(page);
  await expect(reopened.getByRole("textbox", { name: "Subject" })).toHaveValue("Notes for Monday");
  const text = reopened.getByRole("textbox", { name: "Message" });
  await expect(text).toHaveValue("First thoughts");
  await text.fill("First thoughts, now finished");
  await sendMessage(page, reopened);
  expect(await smtpMessageMatching(server, "Subject: Notes for Monday")).toContain("now finished");
  await expect.poll(serverDrafts).not.toContain("Notes for Monday");

  // A draft another client left on the server opens with Enter; saving it
  // replaces that message instead of adding a second copy.
  await control(server, "imap-deliver", {
    mailbox: "Drafts",
    flags: ["\\Draft"],
    raw: [
      "From: Ada Lovelace <ada@work.example>",
      "To: grace@navy.example",
      "Subject: From the phone",
      "Message-ID: <phone-1@work.example>",
      "",
      "Typed on the phone",
      "",
    ].join("\n"),
  });
  await refreshList(page);
  const foreign = messageRow(page, "From the phone");
  await expect(foreign).toBeVisible({ timeout: 20_000 });
  await foreign.click();
  await page.keyboard.press("Enter");
  const composer = composerWindow(page);
  await expect(composer.getByRole("textbox", { name: "Message" })).toHaveValue(
    /Typed on the phone/,
  );
  await expect(composer.getByRole("textbox", { name: "To" })).toHaveValue("grace@navy.example");
  await composer.getByRole("textbox", { name: "Subject" }).fill("From the phone, edited");
  await expect(composer.locator('[data-part="composer.saved"]')).toHaveText("Draft saved.", {
    timeout: 10_000,
  });
  await expect.poll(serverDrafts).toEqual(["From the phone, edited"]);
});
