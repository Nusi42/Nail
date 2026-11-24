import { expect, test, type Page } from "@playwright/test";
import { nailPerTest, control } from "../support/nail";
import { showView, toolbarAction, openMail, DEMO_MULTI_CONVERSATIONS } from "../support/views";
import { eventChip, icalTime, openCalendar } from "../support/calendar";
import { startNewEvent } from "../support/eventEditor";
import {
  deliver as deliverTo,
  imip,
  openCard,
  repliedWith,
  request,
  smtp as smtpOf,
  WORK,
} from "../support/invitations";

/**
 * Wave M3b (ADR 0070) against `nail serve --seed demo-multi`: iMIP
 * invitations delivered to the work account's mock IMAP server, answers on
 * the mock SMTP server (`smtp-messages`), the calendar window, the event
 * editor's attendee picker with free/busy from the mock CalDAV server's
 * second principal (`bob`).
 */

const server = nailPerTest({ seed: "demo-multi" });

const openApp = (page: Page) => openMail(page, server.url, { rows: DEMO_MULTI_CONVERSATIONS });
const deliver = (page: Page, raw: string, subject: string) => deliverTo(server, page, raw, subject);
const smtp = () => smtpOf(server);

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
  await expect.poll(() => repliedWith(server, "ACCEPTED"), { timeout: 30_000 }).toBe(true);
  const calendar = await openCalendar(page, { agenda: true });
  await expect(eventChip(calendar, "Atlas kickoff")).toBeVisible();
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
  const calendar = await openCalendar(page, { agenda: true });
  await expect(eventChip(calendar, "Retro moved")).toBeVisible();
  await expect(eventChip(calendar, "Retro")).toHaveCount(0);
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
  const again = await openCalendar(page, { agenda: true });
  await expect(again.getByRole("button", { name: /^Retro moved,/ })).toHaveCount(0);
});

/** Opens Nail's agenda and a new event titled `title`; the calendar and the event editor. */
async function newEvent(page: Page, title: string) {
  await openApp(page);
  return startNewEvent(page, title);
}

test("a counter-proposal is accepted by the organizer and the update goes out", async ({
  page,
}) => {
  const { calendar, editor } = await newEvent(page, "Design review");
  await editor.getByLabel("Address").fill("grace@example.org");
  await editor.getByRole("button", { name: "Add", exact: true }).click();
  await editor.getByRole("button", { name: "Save" }).click();
  await expect(editor).toBeHidden();

  // Reopen the meeting: the organizer panel sends the invitations.
  await eventChip(calendar, "Design review").dblclick();
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
  const { editor } = await newEvent(page, "Workshop planning");
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
  const calendar = await openCalendar(page, { agenda: true });
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
