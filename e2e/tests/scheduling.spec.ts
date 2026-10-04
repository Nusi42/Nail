import { expect, test, type Locator, type Page } from "@playwright/test";
import { control, startNail, type NailServer } from "../support/nail";
import { showView, toolbarAction, viewRegion } from "../support/views";

/**
 * Wave M3b (ADR 0070) against `nail serve --seed demo-multi`: iMIP
 * invitations delivered to the work account's mock IMAP server, answers on
 * the mock SMTP server (`smtp-messages`), the calendar window, the event
 * editor's attendee picker with free/busy from the mock CalDAV server's
 * second principal (`bob`).
 */

let server: NailServer;

test.beforeEach(async () => {
  server = await startNail({ seed: "demo-multi" });
});

test.afterEach(async () => {
  // `beforeEach` may have failed before assigning it.
  await (server as NailServer | undefined)?.stop();
});

const WORK = "ada@work.example";
const GRACE = "Grace Hopper <grace@example.org>";

const rows = (page: Page) => page.locator('[data-part="message-list.row"]');

async function openApp(page: Page) {
  await page.goto(server.url);
  await expect(rows(page)).toHaveCount(17);
}

/** Today + `days` at `hour`:00 UTC, as an iCalendar UTC time. */
function icalTime(days: number, hour: number): string {
  const d = new Date();
  d.setUTCDate(d.getUTCDate() + days);
  d.setUTCHours(hour, 0, 0, 0);
  return d
    .toISOString()
    .replace(/[-:]/g, "")
    .replace(/\.\d{3}/, "");
}

interface Imip {
  subject: string;
  method: string;
  uid: string;
  sequence?: number;
  stamp?: string;
  lines: string[];
  from?: string;
}

/** An iMIP mail to the work account. */
function imip(m: Imip): string {
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

function request(uid: string, title: string, days: number, sequence = 0, stamp?: string): string {
  return imip({
    subject: title,
    method: "REQUEST",
    uid,
    sequence,
    ...(stamp ? { stamp } : {}),
    lines: [
      `DTSTART:${icalTime(days, 9)}`,
      `DTEND:${icalTime(days, 10)}`,
      `SUMMARY:${title}`,
      "LOCATION:Room 1",
      "ORGANIZER;CN=Grace Hopper:mailto:grace@example.org",
      `ATTENDEE;CN=Ada;PARTSTAT=NEEDS-ACTION;RSVP=TRUE:mailto:${WORK}`,
      "ATTENDEE;CN=Grace Hopper;PARTSTAT=ACCEPTED:mailto:grace@example.org",
    ],
  });
}

async function deliver(page: Page, raw: string, subject: string): Promise<Locator> {
  await control(server, "imap-deliver", { raw });
  // Case-sensitive: a string `hasText` ignores case, and "Retro" also
  // matched a demo message's "retrouve" whenever its row was rendered.
  const literal = subject.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const row = rows(page).filter({ hasText: new RegExp(literal) });
  await expect(row).toHaveCount(1, { timeout: 20_000 });
  return row;
}

/** Opens a message and returns its invitation card. */
async function openCard(page: Page, row: Locator, title: string): Promise<Locator> {
  await row.click();
  const card = page.getByRole("region", { name: `Invitation: ${title}` });
  await expect(card).toBeVisible();
  return card;
}

interface SmtpMessage {
  from: string;
  to: string[];
  data: string;
}

const smtp = () => control<SmtpMessage[]>(server, "smtp-messages");

async function openCalendar(page: Page): Promise<Locator> {
  await showView(page, "Calendar");
  const calendar = viewRegion(page, "Calendar");
  await expect(calendar).toBeVisible();
  await calendar.getByRole("button", { name: "Agenda", exact: true }).click();
  return calendar;
}

const chip = (scope: Locator, title: string) =>
  scope.getByRole("button", { name: new RegExp(`^${title},`) }).first();

test("an invitation is accepted: the reply goes out and the event is in the calendar", async ({
  page,
}) => {
  await openApp(page);
  const row = await deliver(
    page,
    request("e2e-planning@example.org", "Atlas kickoff", 1),
    "Atlas kickoff",
  );
  const card = await openCard(page, row, "Atlas kickoff");
  // Added to the calendar on arrival, not answered yet.
  await expect(card.getByText("Your calendar is up to date")).toBeVisible();
  await expect(card.getByText("No answer yet", { exact: true }).first()).toBeVisible();
  await card
    .getByRole("group", { name: "Your answer" })
    .getByRole("button", { name: "Accept" })
    .click();
  await expect(card.getByText("Accepted. Your answer is on its way.")).toBeVisible();
  // The REPLY leaves through the outbox (undo-send delay) to the organizer.
  await expect
    .poll(
      async () =>
        (await smtp()).some(
          (m) =>
            m.to.includes("grace@example.org") &&
            m.data.includes("METHOD:REPLY") &&
            m.data.includes("PARTSTAT=ACCEPTED"),
        ),
      { timeout: 30_000 },
    )
    .toBe(true);
  const calendar = await openCalendar(page);
  await expect(chip(calendar, "Atlas kickoff")).toBeVisible();
});

test("an organizer's update changes the event and a cancellation removes it", async ({ page }) => {
  await openApp(page);
  const uid = "e2e-retro@example.org";
  await deliver(page, request(uid, "Retro", 1), "Retro");
  // Added on arrival; the organizer moves it a day (applied on arrival).
  const update = await deliver(
    page,
    request(uid, "Retro moved", 2, 1, "20261002T080000Z"),
    "Retro moved",
  );
  const card = await openCard(page, update, "Retro moved");
  await expect(card.getByText("Your calendar is up to date")).toBeVisible({ timeout: 20_000 });
  const calendar = await openCalendar(page);
  await expect(chip(calendar, "Retro moved")).toBeVisible();
  await expect(chip(calendar, "Retro")).toHaveCount(0);
  await showView(page, "Mail");
  await expect(calendar).toBeHidden();

  // The organizer cancels: "Remove from calendar".
  const cancel = await deliver(
    page,
    imip({
      subject: "Cancelled: Retro moved",
      method: "CANCEL",
      uid,
      sequence: 2,
      stamp: "20261003T080000Z",
      lines: ["ORGANIZER:mailto:grace@example.org", `ATTENDEE:mailto:${WORK}`, "STATUS:CANCELLED"],
    }),
    "Cancelled: Retro moved",
  );
  await cancel.click();
  const cancelCard = page.getByRole("region", { name: /^Invitation:/ });
  await cancelCard.getByRole("button", { name: "Remove from calendar" }).click();
  await expect(cancelCard.getByText("Removed from your calendar.")).toBeVisible();
  const again = await openCalendar(page);
  await expect(again.getByRole("button", { name: /^Retro moved,/ })).toHaveCount(0);
});

test("a counter-proposal is accepted by the organizer and the update goes out", async ({
  page,
}) => {
  await openApp(page);
  const calendar = await openCalendar(page);
  await calendar.getByRole("button", { name: "New event" }).click();
  const editor = page.getByRole("dialog", { name: "Event", exact: true });
  await editor.getByLabel("Title", { exact: true }).fill("Design review");
  await editor.getByLabel("Address").fill("grace@example.org");
  await editor.getByRole("button", { name: "Add", exact: true }).click();
  await editor.getByRole("button", { name: "Save" }).click();
  await expect(editor).toBeHidden();

  // Reopen the meeting: the organizer panel sends the invitations.
  await chip(calendar, "Design review").click();
  await expect(editor).toBeVisible();
  const meeting = editor.getByRole("region", { name: "Meeting: Design review" });
  await meeting.getByRole("button", { name: "Send invitations" }).click();
  let uid = "";
  await expect
    .poll(
      async () => {
        const sent = (await smtp()).find(
          (m) => m.data.includes("METHOD:REQUEST") && m.data.includes("SUMMARY:Design review"),
        );
        uid = /UID:(\S+)/.exec(sent?.data ?? "")?.[1] ?? "";
        return uid;
      },
      { timeout: 30_000 },
    )
    .not.toBe("");
  await editor.getByRole("button", { name: "Close" }).first().click();
  await showView(page, "Mail");

  // Grace proposes another time.
  const counter = await deliver(
    page,
    imip({
      subject: "New time proposed: Design review",
      method: "COUNTER",
      uid,
      lines: [
        `DTSTART:${icalTime(3, 15)}`,
        `DTEND:${icalTime(3, 16)}`,
        "SUMMARY:Design review",
        `ORGANIZER:mailto:${WORK}`,
        "ATTENDEE;CN=Grace Hopper;PARTSTAT=ACCEPTED:mailto:grace@example.org",
        "COMMENT:Afternoon is better",
      ],
    }),
    "New time proposed: Design review",
  );
  const card = await openCard(page, counter, "Design review");
  await expect(card.getByText("Afternoon is better").first()).toBeVisible();
  await card.getByRole("button", { name: "Accept the new time" }).click();
  await expect(card.getByText("The meeting moved; everyone gets the update.")).toBeVisible();
  await expect
    .poll(
      async () =>
        (await smtp()).some(
          (m) =>
            m.data.includes("METHOD:REQUEST") &&
            m.data.includes(uid) &&
            m.data.includes(`DTSTART:${icalTime(3, 15)}`),
        ),
      { timeout: 30_000 },
    )
    .toBe(true);
});

test("free/busy shows the busy blocks of another account on the CalDAV server", async ({
  page,
}) => {
  await openApp(page);
  const calendar = await openCalendar(page);
  await calendar.getByRole("button", { name: "New event" }).click();
  const editor = page.getByRole("dialog", { name: "Event", exact: true });
  await editor.getByLabel("Title", { exact: true }).fill("Workshop planning");
  await editor.getByLabel("Address").fill("bob@example.org");
  await editor.getByRole("button", { name: "Add", exact: true }).click();
  const availability = editor.locator('[data-part="freebusy"]');
  // Bob's demo events are tomorrow in UTC (10:00 and 14:00 UTC); the
  // timeline starts at today in the local zone, which can be another day.
  const now = new Date();
  const utcTomorrow = Date.UTC(now.getUTCFullYear(), now.getUTCMonth(), now.getUTCDate() + 1);
  const localToday = Date.UTC(now.getFullYear(), now.getMonth(), now.getDate());
  for (let day = 0; day < Math.round((utcTomorrow - localToday) / 86_400_000); day++) {
    await availability.getByRole("button", { name: "Next day" }).click();
  }
  const bob = availability.getByRole("img", { name: /^bob@example\.org: .*busy/ });
  await expect(bob).toBeVisible({ timeout: 20_000 });
  await expect(
    availability.locator('[data-part="freebusy.row"][data-source="caldav"]'),
  ).toHaveCount(1);
  await expect(
    availability
      .locator('[data-part="freebusy.row"][data-source="caldav"]')
      .locator('[data-part="freebusy.busy"]'),
  ).toHaveCount(2);
});

interface DavObject {
  name: string;
  etag: string;
  data: string;
}

const davObjects = (collection: string) =>
  control<DavObject[]>(server, "dav-objects", { collection });

test("invitations go to the CalDAV calendar chosen in the settings and updates change it there", async ({
  page,
}) => {
  await openApp(page);
  // Settings: new invitations go to the CalDAV calendar "Personal".
  const calendar = await openCalendar(page);
  await toolbarAction(
    page,
    page.getByRole("toolbar", { name: "Calendar actions" }),
    "Invitation settings",
  );
  const settings = page.getByRole("dialog", { name: "Invitation settings" });
  await settings.getByLabel("Calendar for new invitations").selectOption({ label: "Personal" });
  await settings.getByRole("button", { name: "Save", exact: true }).click();
  await expect(settings).toBeHidden();
  await showView(page, "Mail");
  await expect(calendar).toBeHidden();

  const uid = "e2e-orion@example.org";
  await deliver(page, request(uid, "Orion sync", 1), "Orion sync");
  // Added on arrival: the event is written to the mock CalDAV server.
  await expect
    .poll(
      async () =>
        (await davObjects("personal")).some(
          (o) => o.data.includes(`UID:${uid}`) && o.data.includes("SUMMARY:Orion sync"),
        ),
      { timeout: 30_000 },
    )
    .toBe(true);
  // The organizer's update changes the copy on the server.
  await deliver(
    page,
    request(uid, "Orion sync moved", 2, 1, "20261002T080000Z"),
    "Orion sync moved",
  );
  await expect
    .poll(
      async () =>
        (await davObjects("personal")).some(
          (o) => o.data.includes(`UID:${uid}`) && o.data.includes("SUMMARY:Orion sync moved"),
        ),
      { timeout: 30_000 },
    )
    .toBe(true);
  expect((await davObjects("personal")).filter((o) => o.data.includes(`UID:${uid}`))).toHaveLength(
    1,
  );
});
