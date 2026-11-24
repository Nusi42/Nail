import { expect, test, type Page } from "@playwright/test";
import { eventChip, icalTime, openCalendar } from "../support/calendar";
import { deliver, imip, repliedWith, request, smtp, WORK } from "../support/invitations";
import { control, nailPerTest } from "../support/nail";
import { rpcOk } from "../support/remote";
import { DEMO_MULTI_CONVERSATIONS, messageRows, openMail, showView } from "../support/views";

/**
 * Wave R8, the calendar and PIM backlog (ADR 0251), against `nail serve
 * --seed demo-multi`:
 *
 * - an invitation answer waits in the outbox: its toast offers Undo, which
 *   keeps the mail from going out;
 * - the quick look answers one occurrence of a recurring invitation;
 * - right-click menus of events, contacts and tasks;
 * - the command palette's "New event from text";
 * - recent mail with the person on the contact card;
 * - free time follows the working hours of Settings (the core cuts it).
 */

const server = nailPerTest({ seed: "demo-multi" });

const openApp = (page: Page) => openMail(page, server.url, { rows: DEMO_MULTI_CONVERSATIONS });

test("an invitation answer can be taken back while it waits in the outbox", async ({ page }) => {
  await openApp(page);
  const uid = "e2e-r8-undo@example.org";
  await deliver(server, page, request(uid, "Undo review", 1), "Undo review");
  const calendar = await openCalendar(page, { agenda: true });
  await eventChip(calendar, "Undo review").click();
  const look = page.getByRole("dialog", { name: "Undo review" });
  await look
    .getByRole("group", { name: "Your answer" })
    .getByRole("button", { name: "Decline" })
    .click();
  const toast = page.locator('[data-part="pim.toast"][data-kind="reply"]');
  await expect(toast).toContainText("Your answer goes out in a moment.");
  await toast.getByRole("button", { name: "Undo" }).click();
  await expect(toast).toContainText("Your answer was not sent. Your calendar keeps it.");
  // Longer than the undo-send delay: still nothing went to Grace.
  await page.waitForTimeout(12_000);
  expect(await repliedWith(server, "DECLINED", uid)).toBe(false);
});

test("the quick look answers one occurrence of a recurring invitation", async ({ page }) => {
  await openApp(page);
  const uid = "e2e-r8-weekly@example.org";
  const raw = imip({
    subject: "Weekly sync",
    method: "REQUEST",
    uid,
    lines: [
      `DTSTART:${icalTime(1, 9)}`,
      `DTEND:${icalTime(1, 10)}`,
      "RRULE:FREQ=WEEKLY;COUNT=4",
      "SUMMARY:Weekly sync",
      "ORGANIZER;CN=Grace Hopper:mailto:grace@example.org",
      `ATTENDEE;CN=Ada;PARTSTAT=NEEDS-ACTION;RSVP=TRUE:mailto:${WORK}`,
      "ATTENDEE;CN=Grace Hopper;PARTSTAT=ACCEPTED:mailto:grace@example.org",
    ],
  });
  await deliver(server, page, raw, "Weekly sync");
  const calendar = await openCalendar(page, { agenda: true });
  await eventChip(calendar, "Weekly sync").click();
  const look = page.getByRole("dialog", { name: "Weekly sync" });
  const scope = look.getByRole("group", { name: "Answer for" });
  await expect(scope.getByRole("radio", { name: "All occurrences" })).toBeChecked();
  await scope.getByRole("radio", { name: "One occurrence" }).check();
  await look
    .getByRole("group", { name: "Your answer" })
    .getByRole("button", { name: "Decline" })
    .click();
  await expect
    .poll(
      async () =>
        (await smtp(server)).some(
          (m) =>
            m.data.includes("METHOD:REPLY") &&
            m.data.includes(uid) &&
            m.data.includes("RECURRENCE-ID") &&
            m.data.includes("PARTSTAT=DECLINED"),
        ),
      { timeout: 40_000 },
    )
    .toBe(true);
});

test("events, contacts and tasks have right-click menus", async ({ page }) => {
  await openApp(page);
  const calendar = await openCalendar(page, { agenda: true });
  const event = eventChip(calendar, "Team sync");
  await event.click({ button: "right" });
  const eventMenu = page.getByRole("menu", { name: /^Actions for Team sync/ });
  await expect(eventMenu.getByRole("menuitem")).toHaveText(["Open", "Edit", "Delete"]);
  await expect(eventMenu.getByRole("menuitem", { name: "Open" })).toBeFocused();
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("Enter");
  await expect(page.getByRole("dialog", { name: "Event", exact: true })).toBeVisible();
  await page.keyboard.press("Escape");

  const contacts = await showView(page, "Contacts");
  const grace = contacts.getByRole("option", { name: /Grace Hopper/ }).first();
  await grace.click({ button: "right" });
  const contactMenu = page.getByRole("menu", { name: /^Actions for Grace Hopper/ });
  await expect(contactMenu.getByRole("menuitem")).toHaveText(["Open", "Edit", "Delete…"]);
  await page.keyboard.press("Escape");
  await expect(contactMenu).toBeHidden();
  await expect(grace).toBeFocused();

  const tasks = await showView(page, "Tasks");
  await tasks.getByRole("button", { name: "New task" }).click();
  const editor = page.getByRole("dialog", { name: "Task", exact: true });
  await editor.getByLabel("Title", { exact: true }).fill("Water the plants");
  await editor.getByRole("button", { name: "Save" }).click();
  const task = tasks.getByRole("treeitem", { name: /Water the plants/ });
  await task.focus();
  await page.keyboard.press("Shift+F10");
  const taskMenu = page.getByRole("menu", { name: /^Actions for Water the plants/ });
  await taskMenu.getByRole("menuitem", { name: /as done/ }).click();
  await expect(task).toBeHidden();
});

test("the command palette creates an event from a sentence", async ({ page }) => {
  await openApp(page);
  await page.keyboard.press("Control+k");
  const palette = page.getByRole("dialog", { name: "Command palette" });
  await palette.getByRole("combobox", { name: "Search actions" }).fill("event from text");
  await palette
    .getByRole("option")
    .filter({ hasText: /^New event from text/ })
    .first()
    .click();
  const window = page.getByRole("dialog", { name: "New event from text" });
  const field = window.getByRole("textbox", { name: "Quick add an event" });
  await expect(field).toBeFocused();
  await field.fill("Lunch with Bob tomorrow 12:30");
  const chips = window.getByRole("list", { name: "What Nail understood" });
  await expect(chips.locator('[data-kind="time"]')).toContainText("12:30");
  await field.press("Enter");
  await expect(window.getByText(/^Added “Lunch with Bob” on/)).toBeVisible({ timeout: 15_000 });
  await page.keyboard.press("Escape");
  const calendar = await openCalendar(page, { agenda: true });
  await expect(eventChip(calendar, "Lunch with Bob")).toHaveAttribute("aria-label", /12:30\s?PM/);
});

test("the contact card lists recent mail with the person and opens it", async ({ page }) => {
  await openApp(page);
  await control(server, "imap-deliver", {
    raw: [
      "From: Grace Hopper <grace@navy.example>",
      `To: ${WORK}`,
      "Subject: Compiler notes",
      `Date: ${new Date().toUTCString()}`,
      "Message-ID: <e2e-r8-recent@navy.example>",
      "",
      "Here are my notes.",
      "",
    ].join("\r\n"),
  });
  await expect(messageRows(page).filter({ hasText: "Compiler notes" })).toHaveCount(1, {
    timeout: 20_000,
  });
  const contacts = await showView(page, "Contacts");
  await contacts
    .getByRole("option", { name: /Grace Hopper/ })
    .first()
    .click();
  const recent = contacts.locator('[data-field="recent-mail"]');
  await expect(recent.getByRole("heading", { name: "Recent mail" })).toBeVisible();
  await recent.getByRole("button", { name: /^Open “Compiler notes”/ }).click();
  const reader = page.locator('[data-part="message-view"]').first();
  await expect(reader).toContainText("Here are my notes.", { timeout: 15_000 });
});

test("free time follows the working hours of Settings", async ({ page }) => {
  await openApp(page);
  // Working hours 1 PM – 3 PM, every day.
  const { preferences } = await rpcOk(
    server,
    { type: "GetCalendarPreferences" },
    "CalendarPreferences",
  );
  await rpcOk(
    server,
    {
      type: "SetCalendarPreferences",
      preferences: {
        ...(preferences as object),
        work_hours: { start_minute: 13 * 60, end_minute: 15 * 60 },
        work_days: ["mon", "tue", "wed", "thu", "fri", "sat", "sun"],
      },
    },
    "CalendarPreferences",
  );
  const calendar = await openCalendar(page);
  await calendar.getByRole("button", { name: "Free time", exact: true }).click();
  const slots = calendar.locator('[data-part="calendar.free-time.slot"]');
  await expect(slots.first()).toBeVisible({ timeout: 15_000 });
  const labels = await slots.evaluateAll((els) => els.map((el) => el.getAttribute("aria-label")));
  expect(labels.length).toBeGreaterThan(0);
  for (const label of labels) {
    // "Free on …, 1:00 PM – 3:00 PM: plan an event" (or a part of those hours).
    const times = [...(label ?? "").matchAll(/(\d{1,2}):(\d{2})\s?(AM|PM)/g)].map(
      ([, h, m, ap]) => ((Number(h) % 12) + (ap === "PM" ? 12 : 0)) * 60 + Number(m),
    );
    expect(times, label ?? "").toHaveLength(2);
    for (const minute of times) {
      expect(minute, label ?? "").toBeGreaterThanOrEqual(13 * 60);
      expect(minute, label ?? "").toBeLessThanOrEqual(15 * 60);
    }
  }
});
