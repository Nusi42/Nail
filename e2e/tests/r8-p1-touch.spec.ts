import { expect, test, type Locator, type Page } from "@playwright/test";
import { openCalendar } from "../support/calendar";
import { nailPerTest } from "../support/nail";
import { openMail } from "../support/views";

/**
 * Wave R8 (ADR 0251): drawing events on touch screens, against `nail serve
 * --seed demo-multi` in Chromium with real touch input (the DevTools
 * protocol's touch events, so the pointer type is `touch`). A finger held
 * still on an empty slot draws the event when it moves; a finger that
 * moves at once scrolls the grid and draws nothing.
 */

test.use({ hasTouch: true });

const server = nailPerTest({ seed: "demo-multi" });

type TouchKind = "touchStart" | "touchMove" | "touchEnd";

/** One finger on the page: each call sends a touch event at `x`, `y` (none for the end). */
async function finger(page: Page) {
  const cdp = await page.context().newCDPSession(page);
  return async (type: TouchKind, x = 0, y = 0): Promise<void> => {
    await cdp.send("Input.dispatchTouchEvent", {
      type,
      touchPoints: type === "touchEnd" ? [] : [{ x, y }],
    });
  };
}

/** The day view, scrolled so 2 PM is in sight; answers a point at `minute` past 2 PM. */
async function dayAtTwo(
  page: Page,
): Promise<{ calendar: Locator; at: (minute: number) => [number, number] }> {
  await openMail(page, server.url);
  const calendar = await openCalendar(page);
  await calendar.getByRole("button", { name: "Day", exact: true }).click();
  const two = calendar.locator('[data-part="calendar.slot"][data-slot="0-14"]');
  await two.scrollIntoViewIfNeeded();
  const box = await two.boundingBox();
  if (!box) throw new Error("no 2 PM slot");
  return {
    calendar,
    at: (minute) => [box.x + box.width / 2, box.y + (minute / 60) * box.height],
  };
}

test.skip(
  ({ browserName }) => browserName !== "chromium",
  "touch input through Chromium's protocol",
);

test("a held finger draws a new event", async ({ page }) => {
  const { calendar, at } = await dayAtTwo(page);
  const touch = await finger(page);
  await touch("touchStart", ...at(5));
  // Held still past the hold time: from now on the finger draws.
  await page.waitForTimeout(700);
  for (const minute of [30, 60, 91]) await touch("touchMove", ...at(minute));
  await expect(calendar.locator('[data-part="calendar.draft"]')).toContainText(
    /1 hour,? 30 minutes/,
  );
  await touch("touchEnd");
  const draft = page.getByRole("dialog", { name: "New event", exact: true });
  await expect(draft).toContainText(/2:00\s?PM\s?–\s?3:30\s?PM/);
});

test("a finger that moves at once scrolls and draws nothing", async ({ page }) => {
  const { calendar, at } = await dayAtTwo(page);
  const touch = await finger(page);
  const scroll = calendar.locator('[data-part="calendar.scroll"]');
  const before = await scroll.evaluate((el) => el.scrollTop);
  await touch("touchStart", ...at(30));
  for (const minute of [10, -20, -60, -120]) await touch("touchMove", ...at(minute));
  await touch("touchEnd");
  await page.waitForTimeout(700);
  await expect(calendar.locator('[data-part="calendar.draft"]')).toHaveCount(0);
  await expect(page.getByRole("dialog", { name: "New event", exact: true })).toBeHidden();
  expect(await scroll.evaluate((el) => el.scrollTop)).toBeGreaterThan(before);
});
