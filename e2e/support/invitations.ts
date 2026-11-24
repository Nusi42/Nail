/**
 * iMIP invitations in end-to-end tests against `--seed demo-multi`
 * (scheduling and the R7 quick look, DRY): mails with a `text/calendar`
 * part delivered to the work account's mock IMAP server, the invitation
 * card of a delivered message, and what the mock SMTP server received.
 */
import { expect, type Locator, type Page } from "@playwright/test";
import { icalTime } from "./calendar";
import { control, type NailServer } from "./nail";
import { messageRows } from "./views";

/** The work account of `demo-multi` (the invited user). */
export const WORK = "ada@work.example";
/** The organizer of the test invitations. */
export const GRACE = "Grace Hopper <grace@example.org>";

export interface Imip {
  subject: string;
  method: string;
  uid: string;
  sequence?: number;
  stamp?: string;
  lines: string[];
  from?: string;
}

/** An iMIP mail to the work account. */
export function imip(m: Imip): string {
  return [
    `From: ${m.from ?? GRACE}`,
    `To: ${WORK}`,
    `Subject: ${m.subject}`,
    "Date: Thu, 01 Oct 2026 08:00:00 +0000",
    `Message-ID: <${m.uid}-${m.method}-${String(m.sequence ?? 0)}@example.org>`,
    "MIME-Version: 1.0",
    `Content-Type: text/calendar; method=${m.method}; charset=utf-8`,
    "",
    "BEGIN:VCALENDAR",
    "VERSION:2.0",
    "PRODID:-//Other client//EN",
    `METHOD:${m.method}`,
    "BEGIN:VEVENT",
    `UID:${m.uid}`,
    `DTSTAMP:${m.stamp ?? "20261001T080000Z"}`,
    `SEQUENCE:${String(m.sequence ?? 0)}`,
    ...m.lines,
    "END:VEVENT",
    "END:VCALENDAR",
    "",
  ].join("\r\n");
}

/**
 * An invitation from Grace to the work account, today + `days` from
 * `hour`:00 for an hour (UTC), in Room 1, not answered yet.
 */
export function request(
  uid: string,
  title: string,
  days: number,
  sequence = 0,
  stamp?: string,
  hour = 9,
): string {
  return imip({
    subject: title,
    method: "REQUEST",
    uid,
    sequence,
    ...(stamp ? { stamp } : {}),
    lines: [
      `DTSTART:${icalTime(days, hour)}`,
      `DTEND:${icalTime(days, hour + 1)}`,
      `SUMMARY:${title}`,
      "LOCATION:Room 1",
      "ORGANIZER;CN=Grace Hopper:mailto:grace@example.org",
      `ATTENDEE;CN=Ada;PARTSTAT=NEEDS-ACTION;RSVP=TRUE:mailto:${WORK}`,
      "ATTENDEE;CN=Grace Hopper;PARTSTAT=ACCEPTED:mailto:grace@example.org",
    ],
  });
}

/** Delivers `raw` to the work inbox and answers its row in the message list. */
export async function deliver(
  server: NailServer,
  page: Page,
  raw: string,
  subject: string,
): Promise<Locator> {
  await control(server, "imap-deliver", { raw });
  // Case-sensitive: a string `hasText` ignores case, and "Retro" also
  // matched a demo message's "retrouve" whenever its row was rendered.
  const literal = subject.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const row = messageRows(page).filter({ hasText: new RegExp(literal) });
  await expect(row).toHaveCount(1, { timeout: 20_000 });
  return row;
}

/** Opens a message and returns its invitation card. */
export async function openCard(page: Page, row: Locator, title: string): Promise<Locator> {
  await row.click();
  const card = page.getByRole("region", { name: `Invitation: ${title}` });
  await expect(card).toBeVisible();
  return card;
}

export interface SmtpMessage {
  from: string;
  to: string[];
  data: string;
}

/** What the mock SMTP server received. */
export const smtp = (server: NailServer) => control<SmtpMessage[]>(server, "smtp-messages");

/** Whether a REPLY with `partstat` for `uid` went out to Grace. */
export async function repliedWith(
  server: NailServer,
  partstat: string,
  uid?: string,
): Promise<boolean> {
  return (await smtp(server)).some(
    (m) =>
      m.to.includes("grace@example.org") &&
      m.data.includes("METHOD:REPLY") &&
      m.data.includes(`PARTSTAT=${partstat}`) &&
      (uid === undefined || m.data.includes(uid)),
  );
}
