import { expect, test, type Page } from "@playwright/test";
import { axeViolations } from "../support/axe";
import { eventChip, openCalendar } from "../support/calendar";
import { startNewEvent } from "../support/eventEditor";
import { deliver, openCard, repliedWith, request } from "../support/invitations";
import { control, nailPerTest } from "../support/nail";
import { DEMO_MULTI_CONVERSATIONS, openMail, showView } from "../support/views";

/**
 * R7 (ADR 0246), the quick look and the editor, against `nail serve --seed
 * demo-multi`:
 *
 * - a click on an event opens the quick look beside it (when, where, who);
 *   an unanswered invitation is answered there, Edit opens the full
 *   editor, Escape closes it;
 * - the compact editor: the day as text, the length, the option chips;
 *   what they choose reaches the CalDAV server;
 * - the invitation card: one answer control with Accept first, the day
 *   strip of what it overlaps, secondary actions in "More", and no
 *   "attachment" for the iCalendar part;
 * - scheduling: lanes per person, picking a suggestion moves the meeting
 *   frame, the arrow keys move it in 15-minute steps;
 * - the reminder toast: Dismiss first, snoozing as one split button.
 */

const server = nailPerTest({ seed: "demo-multi" });

const openApp = (page: Page) => openMail(page, server.url, { rows: DEMO_MULTI_CONVERSATIONS });

/** Today at `hour`:00 in the machine's zone (the browser's too), Unix seconds. */
function localUnix(hour: number): number {
  const d = new Date();
  d.setHours(hour, 0, 0, 0);
  return Math.floor(d.getTime() / 1000);
}

interface DavObject {
  name: string;
  data: string;
}

test("the quick look answers an invitation and opens the editor", async ({ page }) => {
  await openApp(page);
  const uid = "e2e-r7-atlas@example.org";
  await deliver(server, page, request(uid, "Atlas review", 1), "Atlas review");
  const calendar = await openCalendar(page, { agenda: true });
  await eventChip(calendar, "Atlas review").click();

  const look = page.getByRole("dialog", { name: "Atlas review" });
  await expect(look).toBeVisible();
  await expect(look.getByText("Room 1")).toBeVisible();
  await expect(look.getByText("Grace Hopper (organizer)")).toBeVisible();
  await expect(look.getByRole("button", { name: "Edit" })).toBeFocused();
  const answers = look.getByRole("group", { name: "Your answer" });
  await expect(answers.getByRole("button", { name: "Accept" })).toHaveAttribute(
    "aria-pressed",
    "false",
  );
  expect(await axeViolations(page, '[data-part="pim.popover"]')).toEqual([]);
  await answers.getByRole("button", { name: "Accept" }).click();
  await expect(look.getByText("Accepted. Your answer is on its way.")).toBeVisible();
  await expect.poll(() => repliedWith(server, "ACCEPTED", uid), { timeout: 30_000 }).toBe(true);

  // Escape closes it; the next click opens it again, and Edit opens the editor.
  await page.keyboard.press("Escape");
  await expect(look).toBeHidden();
  await eventChip(calendar, "Atlas review").click();
  await expect(look).toBeVisible();
  await page.keyboard.press("Enter");
  await expect(look).toBeHidden();
  const editor = page.getByRole("dialog", { name: "Event", exact: true });
  await expect(editor.getByLabel("Title", { exact: true })).toHaveValue("Atlas review", {
    timeout: 15_000,
  });
});

test("the compact editor saves what its chips choose", async ({ page }) => {
  await openApp(page);
  const { calendar, editor } = await startNewEvent(page, "Dentist");
  const when = editor.getByRole("group", { name: "When" });
  // The day reads as text ("Tue, Nov 24, 2026"), with the length beside the times.
  await expect(when.getByRole("button", { name: /^Start, date: / })).toHaveText(
    /^[A-Z][a-z]{2}, [A-Z][a-z]{2} \d{1,2}, \d{4}$/,
  );
  await expect(when.getByText("Length: 1 hour")).toBeVisible();
  const options = editor.getByRole("group", { name: "Options" });
  await options.getByLabel("Calendar", { exact: true }).selectOption({ label: "Personal" });
  await options.getByLabel("Reminder").selectOption({ label: "30 minutes before" });
  await options.getByLabel("Show me as").selectOption({ label: "Free" });
  await expect(editor.getByText("Visibility")).toBeHidden();
  await editor.getByText("More options").click();
  await expect(editor.getByLabel("Visibility")).toBeVisible();
  await editor.getByRole("button", { name: "Save" }).click();
  await expect(editor).toBeHidden();
  await expect(eventChip(calendar, "Dentist")).toBeVisible();
  await expect
    .poll(
      async () =>
        (await control<DavObject[]>(server, "dav-objects", { collection: "personal" })).some(
          (o) =>
            o.data.includes("SUMMARY:Dentist") &&
            o.data.includes("TRANSP:TRANSPARENT") &&
            o.data.includes("TRIGGER:-PT30M"),
        ),
      { timeout: 30_000 },
    )
    .toBe(true);
});

test("the invitation card answers with one control and shows what it overlaps", async ({
  page,
}) => {
  await openApp(page);
  await deliver(
    server,
    page,
    request("e2e-r7-first@example.org", "Orbit kickoff", 1),
    "Orbit kickoff",
  );
  const row = await deliver(
    server,
    page,
    request("e2e-r7-second@example.org", "Orbit planning", 1),
    "Orbit planning",
  );
  const card = await openCard(page, row, "Orbit planning");
  await expect(card.getByText(/^This overlaps/)).toBeVisible({ timeout: 20_000 });
  const strip = card.getByRole("list", { name: "Orbit planning and what it overlaps" });
  await expect(strip.getByText("Orbit kickoff")).toBeVisible();
  // The iCalendar part is the card, not an attachment.
  await expect(page.getByRole("list", { name: "Attachments" })).toHaveCount(0);
  await expect(page.getByText("Unnamed attachment")).toHaveCount(0);
  const answers = card.getByRole("group", { name: "Your answer" });
  await expect(answers.getByRole("button")).toHaveText(["Accept", "Maybe", "Decline"]);
  expect(await axeViolations(page, '[data-part="invitation"]')).toEqual([]);

  // Secondary actions wait in "More".
  await card.getByRole("button", { name: "More" }).click();
  await page.getByRole("menuitem", { name: "Forward invitation" }).click();
  await expect(card.getByRole("form", { name: "Forward invitation" })).toBeVisible();
  await card.getByRole("button", { name: "Cancel" }).click();

  await answers.getByRole("button", { name: "Maybe" }).click();
  await expect(card.getByText("Marked as maybe. Your answer is on its way.")).toBeVisible();
  await expect
    .poll(() => repliedWith(server, "TENTATIVE", "e2e-r7-second@example.org"), {
      timeout: 30_000,
    })
    .toBe(true);
});

test("scheduling: a suggestion moves the meeting frame, the arrow keys nudge it", async ({
  page,
}) => {
  await openApp(page);
  const { editor } = await startNewEvent(page, "Workshop");
  await editor.getByLabel("Address").fill("bob@example.org");
  await editor.getByRole("button", { name: "Add", exact: true }).click();
  const availability = editor.locator('[data-part="freebusy"]');
  // Tomorrow always has working hours left, whatever the time of day now.
  await availability.getByRole("button", { name: "Next day" }).click();
  const frame = availability.getByRole("slider", { name: /^Meeting time: / });
  const pick = availability.getByRole("button", { name: /^Move the meeting to / }).first();
  await expect(pick).toBeVisible({ timeout: 20_000 });
  const chosen = (await pick.textContent()) ?? "";
  await pick.click();
  await expect(frame).toHaveAttribute("aria-valuetext", `Meeting time: ${chosen}`);
  // The chosen time is not offered again, and the texts agree.
  await expect(availability.getByRole("button", { name: new RegExp(`, ${chosen}$`) })).toHaveCount(
    0,
  );
  await expect(availability.getByText(/no time on this day/)).toHaveCount(0);
  // The role labels are for screen readers only.
  const role = editor.locator("label", { hasText: "Role of bob@example.org" });
  await expect(role).toHaveAttribute("data-part", "pim.look.hidden");
  expect((await role.boundingBox())?.width ?? 0).toBeLessThanOrEqual(1);

  const before = Number(await frame.getAttribute("aria-valuenow"));
  await frame.focus();
  await page.keyboard.press("ArrowRight");
  await expect(frame).toHaveAttribute("aria-valuenow", String(before + 15));
});

test("a reminder toast says how soon and snoozes from one split button", async ({ page }) => {
  await openApp(page);
  await control(server, "clock-set", { at: localUnix(7) });
  // "New event" starts today at 9:00.
  const { editor } = await startNewEvent(page, "Checkup");
  const options = editor.getByRole("group", { name: "Options" });
  await options.getByLabel("Calendar", { exact: true }).selectOption({ label: "Personal" });
  await options.getByLabel("Reminder").selectOption({ label: "At the start" });
  await editor.getByRole("button", { name: "Save" }).click();
  await expect(editor).toBeHidden();
  await showView(page, "Mail");

  await control(server, "clock-set", { at: localUnix(9) + 5 });
  const toast = page.getByRole("group", { name: "Reminder: Checkup" });
  await expect(toast).toBeVisible({ timeout: 15_000 });
  await expect(toast.locator('[data-part="pim.reminder.meta"]')).toContainText("Personal");
  await expect(toast.locator('[data-part="pim.reminder.when"]')).not.toBeEmpty();
  await expect(toast.getByRole("button")).toHaveText(["Dismiss", "Snooze 10 minutes", "", "Open"]);
  expect(await axeViolations(page, '[data-part="pim.toasts"]')).toEqual([]);
  await toast.getByRole("button", { name: "More snooze times" }).click();
  // The test clock is today 9:00, whatever the time is: snooze to tomorrow morning.
  await page.getByRole("menuitem", { name: "Tomorrow morning" }).click();
  await expect(toast).toHaveCount(0);

  // Tomorrow at 9:00 it is back; Dismiss ends it.
  await control(server, "clock-set", { at: localUnix(9) + 86_400 + 10 });
  await expect(toast).toBeVisible({ timeout: 15_000 });
  await toast.getByRole("button", { name: "Dismiss" }).click();
  await expect(toast).toHaveCount(0);
});
