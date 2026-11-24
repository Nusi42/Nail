import { expect, test, type Locator, type Page } from "@playwright/test";
import { control, nailOnDemand } from "../support/nail";
import { openMail } from "../support/views";
import { eventChip, icalTime, icsEvent, openCalendar, syncCalendars } from "../support/calendar";

/**
 * Wave R7, time grid (ADR 0246) against `nail serve --seed demo-multi`:
 * overlapping events cascade so every title shows, drawing on empty slots
 * creates an event with the drawn times (and a click takes the 15-minute
 * slot under the pointer), and the now line lies under the events with a
 * dot on today and the time in the gutter. Events from "another client"
 * come through the mock CalDAV server (`dav-put`).
 */

const server = nailOnDemand();

/** Puts an event on the CalDAV server today from `from` to `to` (`[hour, minute]` UTC). */
async function serverEvent(
  uid: string,
  title: string,
  from: [number, number],
  to: [number, number],
): Promise<void> {
  await control(server, "dav-put", {
    collection: "personal",
    name: `${uid}.ics`,
    data: icsEvent(`${uid}@other.example`, title, icalTime(0, ...from), icalTime(0, ...to)),
  });
}

async function dayView(page: Page): Promise<Locator> {
  await openMail(page, server.url);
  const calendar = await openCalendar(page);
  await calendar.getByRole("button", { name: "Day", exact: true }).click();
  await expect(calendar.locator('[data-part="calendar.time-grid"]')).toHaveAttribute(
    "data-days",
    "1",
  );
  return calendar;
}

/**
 * Scrolls the grid so `hour` is near the top; answers the viewport y of a
 * minute of the day and the x of the day column's middle.
 */
async function gridAt(calendar: Locator, hour: number) {
  await calendar.locator('[data-part="calendar.scroll"]').evaluate((el, h) => {
    el.scrollTop = (el.scrollHeight / 24) * h;
  }, hour);
  const box = await calendar.locator('[data-part="calendar.columns"]').boundingBox();
  if (!box) throw new Error("no time grid");
  return {
    x: box.x + box.width / 2,
    y: (minute: number) => box.y + (minute / 1440) * box.height,
  };
}

/** Minutes after midnight in the browser's zone of today at `hour`:`minute` UTC. */
async function localMinute(page: Page, hour: number, minute: number): Promise<number> {
  return page.evaluate(
    ([h, m]) => {
      const d = new Date();
      d.setUTCHours(h, m, 0, 0);
      return d.getHours() * 60 + d.getMinutes();
    },
    [hour, minute] as const,
  );
}

test("overlapping events cascade so every title shows", async ({ page }) => {
  await server.start({ seed: "demo-multi" });
  // Early in the (UTC) day, where the demo has no events of its own.
  await serverEvent("cascade-1", "Cascade one", [6, 0], [7, 0]);
  await serverEvent("cascade-2", "Cascade two", [6, 30], [7, 15]);
  await serverEvent("cascade-3", "Cascade three", [6, 45], [8, 0]);
  const calendar = await dayView(page);
  await syncCalendars(page);
  const cards = ["Cascade one", "Cascade two", "Cascade three"].map((t) => eventChip(calendar, t));
  for (const card of cards) await expect(card).toBeVisible();
  await gridAt(calendar, Math.max(0, Math.floor((await localMinute(page, 6, 0)) / 60) - 1));
  const column = await calendar.locator('[data-part="calendar.column"]').first().boundingBox();
  const boxes: { x: number; width: number }[] = [];
  for (const card of cards) {
    const box = await card.boundingBox();
    if (!box) throw new Error("no card");
    boxes.push(box);
  }
  if (!column) throw new Error("no column");
  // Each later event is indented and keeps at least 60 % of the column.
  for (const [index, box] of boxes.entries()) {
    expect(box.width).toBeGreaterThanOrEqual(column.width * 0.58);
    const before = boxes[index - 1];
    if (before) expect(box.x).toBeGreaterThan(before.x + column.width * 0.1);
  }
  // Every title is on top where it is drawn: the card under its first letters is its own.
  for (const [index, card] of cards.entries()) {
    const own = await card.evaluate((el) => {
      const title = el.querySelector('[data-part="event-card.title"]');
      const r = title?.getBoundingClientRect();
      if (!r) return false;
      const hit = document.elementFromPoint(r.left + 6, r.top + r.height / 2);
      return hit?.closest('[data-part="calendar.event"]') === el;
    });
    expect(own, `title ${String(index + 1)} visible`).toBe(true);
  }
  // The later ones carry the paper-coloured outline of a stacked card.
  await expect(eventChip(calendar, "Cascade three")).toHaveAttribute("data-state", /stacked/);
});

test("drawing on empty slots creates an event with the drawn times", async ({ page }) => {
  await server.start({ seed: "demo-multi" });
  const calendar = await dayView(page);
  const grid = await gridAt(calendar, 13);
  // Press at 14:05, drag to 15:31: 2:00 – 3:30 PM, with a live label.
  await page.mouse.move(grid.x, grid.y(14 * 60 + 5));
  await page.mouse.down();
  await page.mouse.move(grid.x, grid.y(15 * 60), { steps: 4 });
  await page.mouse.move(grid.x, grid.y(15 * 60 + 31), { steps: 4 });
  await expect(calendar.locator('[data-part="calendar.draft"]')).toContainText("1 h 30 min");
  await page.mouse.up();
  const draft = page.getByRole("dialog", { name: "New event", exact: true });
  const title = draft.getByRole("textbox", { name: "Title", exact: true });
  await expect(title).toBeFocused();
  await title.fill("Drawn on the grid");
  await title.press("Enter");
  await expect(draft).toBeHidden();
  const drawn = eventChip(calendar, "Drawn on the grid");
  await expect(drawn).toBeVisible();
  await expect(drawn).toHaveAttribute("aria-label", /2:00\s?PM\s?–\s?3:30\s?PM/);

  // A click takes the 15-minute slot under the pointer (4:20 → 4:15), one hour long.
  const again = await gridAt(calendar, 13);
  await page.mouse.click(again.x, again.y(16 * 60 + 20));
  await expect(draft).toBeVisible();
  await expect(draft).toContainText(/4:15\s?PM\s?–\s?5:15\s?PM/);
  await title.fill("Clicked slot");
  await draft.getByRole("button", { name: "Save", exact: true }).click();
  await expect(eventChip(calendar, "Clicked slot")).toHaveAttribute(
    "aria-label",
    /4:15\s?PM\s?–\s?5:15\s?PM/,
  );

  // Esc drops a draft; "More options" opens the editor with the drawn times.
  await page.mouse.click(again.x, again.y(18 * 60 + 5));
  await expect(draft).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(draft).toBeHidden();
  await expect(calendar.locator('[data-part="calendar.draft"]')).toHaveCount(0);
  await page.mouse.click(again.x, again.y(18 * 60 + 5));
  await draft.getByRole("button", { name: "More options", exact: true }).click();
  await expect(page.getByRole("dialog", { name: "Event", exact: true })).toBeVisible();
});

test("the now line lies under the events, with a dot on today and the time in the gutter", async ({
  page,
}) => {
  await server.start({ seed: "demo-multi" });
  // An event around now, so the line crosses it.
  const now = new Date();
  const hour = now.getUTCHours();
  await serverEvent("around-now", "Around now", [hour, 0], [hour + 1, 59]);
  await openMail(page, server.url);
  const calendar = await openCalendar(page);
  await syncCalendars(page);
  const card = eventChip(calendar, "Around now");
  await expect(card).toBeVisible();
  const line = calendar.locator('[data-part="calendar.now"][data-state="today"]');
  await expect(line).toHaveCount(1);
  await expect(calendar.locator('[data-part="calendar.now"][data-state="week"]')).toHaveCount(1);
  await expect(calendar.locator('[data-part="calendar.now-label"]')).toHaveText(/\d/);
  expect(await line.evaluate((el) => getComputedStyle(el).height)).toBe("2px");
  expect(await line.evaluate((el) => getComputedStyle(el, "::before").borderRadius)).toBe("50%");
  // Under the event: the card is what lies where the line crosses it.
  await card.scrollIntoViewIfNeeded();
  const onTop = await line.evaluate((el) => {
    const r = el.getBoundingClientRect();
    const event = [...document.querySelectorAll('[data-part="calendar.event"]')].find((e) =>
      e.getAttribute("aria-label")?.startsWith("Around now,"),
    );
    const box = event?.getBoundingClientRect();
    if (!box) return "no card";
    const hit = document.elementFromPoint(box.left + box.width / 2, r.top + 1);
    return hit?.closest('[data-part="calendar.event"]') === event ? "card" : "line";
  });
  expect(onTop).toBe("card");
  // Today: the date in a filled circle, no shaded column.
  const date = calendar.locator(
    '[data-part="calendar.day-head"][aria-current="date"] [data-part="calendar.day-head.date"]',
  );
  expect(await date.evaluate((el) => getComputedStyle(el).backgroundColor)).not.toBe(
    "rgba(0, 0, 0, 0)",
  );
  const today = await calendar
    .locator('[data-part="calendar.day-head"][aria-current="date"]')
    .getAttribute("data-day");
  const slotColors = await calendar
    .locator('[data-part="calendar.slot"]')
    .evaluateAll(
      (slots, day) => [
        ...new Set(
          slots
            .filter((s) => s.getAttribute("data-day") === day)
            .map((s) => getComputedStyle(s).backgroundColor),
        ),
      ],
      today,
    );
  expect(slotColors).toEqual(["rgba(0, 0, 0, 0)"]);
});
