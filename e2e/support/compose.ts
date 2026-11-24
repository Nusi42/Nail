/**
 * Writing and sending mail in end-to-end tests (wave Q2, DRY): a new
 * composer window with its fields filled in, sending with the status
 * bar's confirmation, and what the `demo-multi` mock servers received
 * (`smtp-messages`, or an API account's `graph-sent` / `ews-sent` /
 * `gmail-sent`).
 */
import { expect, type Locator, type Page } from "@playwright/test";
import { control, serverMessages, type NailServer } from "./nail";
import { composerWindow, DEMO_MULTI_CONVERSATIONS, messageRows, runFromPalette } from "./views";

/**
 * Lengthens the composer's autosave (`__nailAutosaveMs`, read by the UI) for
 * the pages `page` opens: scenarios that close a composer with unsaved
 * changes must not race it.
 */
export async function lengthenAutosave(page: Page): Promise<void> {
  await page.addInitScript(() => {
    (globalThis as Record<string, unknown>).__nailAutosaveMs = 10 * 60 * 1000;
  });
}

/** The recipient most scenarios write to (a contact of the demo data). */
export const GRACE = "grace@navy.example";

/** How long a sent message may take to reach a mock server. */
export const SEND_TIMEOUT = 20_000;

/** A new message: `to` (default {@link GRACE}), the subject and an optional text. */
export interface NewMessage {
  to?: string;
  subject: string;
  text?: string;
}

/** Opens a new composer window (key `c`) and fills in `message`. */
export async function newMessage(page: Page, message: NewMessage): Promise<Locator> {
  await page.keyboard.press("c");
  const composer = composerWindow(page);
  await expect(composer).toBeVisible();
  await composer.getByRole("textbox", { name: "To" }).fill(message.to ?? GRACE);
  await composer.getByRole("textbox", { name: "Subject" }).fill(message.subject);
  if (message.text !== undefined) {
    await composer.getByRole("textbox", { name: "Message" }).fill(message.text);
  }
  return composer;
}

/**
 * Writes a draft `subject` (with `text`) on `demo-multi`, waits until the
 * autosave put it into the server's Drafts, reloads (the open composer is
 * gone) and opens the Outbox; returns the draft's entry there.
 */
export async function draftInOutbox(
  page: Page,
  server: NailServer,
  subject: string,
  text: string,
): Promise<Locator> {
  const composer = await newMessage(page, { subject });
  await composer.getByRole("textbox", { name: "Message" }).fill(text);
  // Autosave: shortly after the last edit, locally and to the server's Drafts.
  await expect(composer.locator('[data-part="composer.saved"]')).toHaveText("Draft saved.", {
    timeout: 10_000,
  });
  await expect
    .poll(async () => (await serverMessages(server, "Drafts")).map((m) => m.subject))
    .toContain(subject);
  // A reload forgets the open composer; the outbox window lists the draft.
  await page.reload();
  await expect(messageRows(page)).toHaveCount(DEMO_MULTI_CONVERSATIONS);
  await runFromPalette(page, "outbox", "Outbox");
  return page
    .getByRole("region", { name: "Drafts" })
    .locator('[data-part="outbox.item"]')
    .filter({ hasText: subject });
}

/** Sends `composer` and waits until the status bar reports the message sent. */
export async function sendMessage(page: Page, composer: Locator): Promise<void> {
  await composer.getByRole("button", { name: "Send", exact: true }).click();
  await expect(composer).toBeHidden();
  await expect(page.getByRole("status", { name: "Status" }).getByText("Message sent.")).toBeVisible(
    { timeout: SEND_TIMEOUT },
  );
}

/** A message the mock SMTP server received. */
export interface SmtpMessage {
  from: string;
  to: string[];
  data: string;
}

/** Every message the mock SMTP server received so far. */
export const smtpMessages = (server: NailServer) => control<SmtpMessage[]>(server, "smtp-messages");

/** Waits for the first message the SMTP server received whose data matches `pattern`; its data. */
export async function smtpMessageMatching(
  server: NailServer,
  pattern: RegExp | string,
): Promise<string> {
  const matches = (data: string) =>
    typeof pattern === "string" ? data.includes(pattern) : pattern.test(data);
  let data = "";
  await expect
    .poll(
      async () => {
        data = (await smtpMessages(server)).find((m) => matches(m.data))?.data ?? "";
        return data !== "";
      },
      { timeout: SEND_TIMEOUT },
    )
    .toBe(true);
  return data;
}

/**
 * Waits until an API account's mock server (`graph-sent`, `ews-sent`,
 * `gmail-sent`: the raw messages it sent) has a message containing `text`.
 */
export async function expectSentThrough(
  server: NailServer,
  command: "graph-sent" | "ews-sent" | "gmail-sent",
  text: string,
): Promise<void> {
  await expect
    .poll(
      async () => (await control<string[]>(server, command)).some((raw) => raw.includes(text)),
      { timeout: 30_000 },
    )
    .toBe(true);
}
