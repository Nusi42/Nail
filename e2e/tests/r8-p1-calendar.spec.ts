import { expect, test, type Locator } from "@playwright/test";
import { colorDistance, MIN_COLOR_DISTANCE } from "../../ui/src/features/pim/colors";
import { control, nailOnDemand } from "../support/nail";
import { openMail } from "../support/views";
import { eventChip, icalTime, icsEvent, openCalendar, syncCalendars } from "../support/calendar";

/**
 * Wave R8 (ADR 0251 items 5 and 6) against `nail serve --seed demo-multi`:
 * every visible calendar of the demo is shown in a colour clearly distinct
 * from the others, and every event title in the week view is shown in full
 * when the card has room for it — not cut at the end, not clamped and not
 * covered by the card's icons — at 1440×900 and 1280×800. An unanswered,
 * repeating invitation with a reminder (four icons) is added to the demo's
 * own events: its icons used to cover the end of its title.
 */

const server = nailOnDemand();

/** `rgb(r, g, b)` → `#rrggbb`. */
function hexOf(rgb: string): string {
  const parts = /rgba?\((\d+),\s*(\d+),\s*(\d+)/.exec(rgb)?.slice(1) ?? [];
  return `#${parts.map((c) => Number(c).toString(16).padStart(2, "0")).join("")}`;
}

/** Where a title stands in its card: overflow and what lies at the end of its last line. */
async function titleFit(card: Locator) {
  return card.evaluate((el) => {
    const title = el.querySelector<HTMLElement>('[data-part="event-card.title"]');
    if (!title) return null;
    const range = document.createRange();
    range.selectNodeContents(title);
    const last = [...range.getClientRects()].at(-1);
    const hit = last ? document.elementFromPoint(last.right - 1, last.top + last.height / 2) : null;
    return {
      text: title.textContent,
      width: [title.scrollWidth, title.clientWidth],
      height: [title.scrollHeight, title.clientHeight],
      endsOnTitle: hit !== null && title.contains(hit),
    };
  });
}

for (const [width, height] of [
  [1440, 900],
  [1280, 800],
] as const) {
  test(`week view at ${String(width)}×${String(height)}: titles fit and calendars stay apart`, async ({
    page,
  }) => {
    await page.setViewportSize({ width, height });
    await server.start({ seed: "demo-multi" });
    await control(server, "dav-put", {
      collection: "personal",
      name: "one-to-one.ics",
      data: icsEvent(
        "one-to-one@other.example",
        "1:1 with Grace",
        icalTime(1, 15),
        icalTime(1, 16),
        [
          "RRULE:FREQ=WEEKLY",
          "ORGANIZER;CN=Grace Hopper:mailto:grace@example.org",
          "ATTENDEE;PARTSTAT=NEEDS-ACTION:mailto:ada@work.example",
          "ATTENDEE;PARTSTAT=ACCEPTED:mailto:grace@example.org",
          "BEGIN:VALARM",
          "ACTION:DISPLAY",
          "TRIGGER:-PT10M",
          "DESCRIPTION:Reminder",
          "END:VALARM",
        ],
      ),
    });
    await openMail(page, server.url);
    const calendar = await openCalendar(page);
    await calendar.getByRole("button", { name: "Week", exact: true }).click();
    await syncCalendars(page);
    const invitation = eventChip(calendar, "1:1 with Grace");
    await expect(invitation).toBeVisible();
    await expect(invitation).toHaveAttribute("data-state", /needs-action/);

    const cards = calendar.locator('[data-part="calendar.event"]');
    expect(await cards.count()).toBeGreaterThanOrEqual(4);
    for (const card of await cards.all()) {
      await card.scrollIntoViewIfNeeded();
      const fit = await titleFit(card);
      expect(fit, "a card without a title").not.toBeNull();
      if (!fit) continue;
      const name = fit.text;
      expect(fit.width[0], `${name}: cut at the end`).toBeLessThanOrEqual(fit.width[1] ?? 0);
      expect(fit.height[0], `${name}: clamped`).toBeLessThanOrEqual(fit.height[1] ?? 0);
      expect(fit.endsOnTitle, `${name}: covered at the end`).toBe(true);
    }

    // Every visible calendar's colour (the list's check box) is distinct from the others'.
    const colors = await page
      .locator(
        '[data-part="calendar-list.item"]:not([data-state~="hidden"]) [data-part="calendar-list.toggle"][data-colored]',
      )
      .evaluateAll((boxes) => boxes.map((b) => getComputedStyle(b).backgroundColor));
    expect(colors.length).toBeGreaterThanOrEqual(4);
    const hexes = colors.map(hexOf);
    for (const [i, a] of hexes.entries()) {
      for (const b of hexes.slice(i + 1)) {
        expect(colorDistance(a, b) ?? 0, `${a} / ${b}`).toBeGreaterThanOrEqual(MIN_COLOR_DISTANCE);
      }
    }
  });
}
