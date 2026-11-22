/**
 * Many numbered messages for bulk-action specs (R4, R5): delivered to the
 * demo's "Work" inbox on the IMAP mock, each its own conversation, newer
 * ones later (each with its own date), so the list shows the highest
 * number on top.
 */
import { expect, type Page } from "@playwright/test";
import { control, serverMessages, type NailServer } from "./nail";
import { accountSectionNamed, messageList, messageRow, sectionInbox } from "./views";

export interface BulkMail {
  /** The subject of message `n` (`<prefix> 007`). */
  subjectOf: (n: number) => string;
  /** Delivers messages 1 to `count`. */
  deliver: (server: NailServer, count: number) => Promise<void>;
  /** Opens the Work inbox, delivers `count` messages and waits until the newest is listed. */
  openWorkInbox: (page: Page, server: NailServer, count: number) => Promise<void>;
  /** How many of these messages a server mailbox holds. */
  onServer: (server: NailServer, mailbox: string) => Promise<number>;
}

/** Numbered messages whose subjects start with `prefix` (Message-IDs with `idPrefix`). */
export function bulkMail(prefix: string, idPrefix: string): BulkMail {
  const subjectOf = (n: number) => `${prefix} ${String(n).padStart(3, "0")}`;
  const deliver = async (server: NailServer, count: number) => {
    for (let n = 1; n <= count; n++) {
      const date = new Date(Date.UTC(2029, 0, 1, 0, n)).toUTCString();
      await control(server, "imap-deliver", {
        raw: `From: Bulk Sender <bulk@example.net>\r\nTo: ada@work.example\r\nSubject: ${subjectOf(n)}\r\nDate: ${date}\r\nMessage-ID: <${idPrefix}-${String(n)}@example.net>\r\n\r\n${subjectOf(n)}\r\n`,
      });
    }
  };
  return {
    subjectOf,
    deliver,
    openWorkInbox: async (page, server, count) => {
      await page.goto(server.url);
      await sectionInbox(accountSectionNamed(page, "Work")).click();
      await deliver(server, count);
      await expect(messageRow(page, subjectOf(count))).toHaveCount(1, { timeout: 60_000 });
    },
    onServer: async (server, mailbox) => {
      const messages = await serverMessages(server, mailbox);
      return messages.filter((m) => m.subject.startsWith(`${prefix} `)).length;
    },
  };
}

/** Scrolls the message list down until `subject`'s row is loaded and shown. */
export async function scrollUntilListed(page: Page, subject: string): Promise<void> {
  const list = messageList(page);
  await expect
    .poll(
      async () => {
        await list.evaluate((el) => {
          el.scrollTop = el.scrollHeight;
        });
        return messageRow(page, subject).count();
      },
      { timeout: 60_000 },
    )
    .toBe(1);
}
