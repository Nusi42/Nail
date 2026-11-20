import { expect, test, type Page } from "@playwright/test";
import { nailPerTest } from "../support/nail";
import { expectResponsive } from "../support/responsive";
import { DEMO_MULTI_CONVERSATIONS, messageList, messageRows, openMail } from "../support/views";

/**
 * R1 WR2 (ADR 0219): the search field searches while typing (after the
 * typing pause, Enter at once), and a query mistake shows a compact
 * indicator in the field with its explanation overlaid on hover and focus,
 * never moving the list under it. On the real core (`demo-multi`).
 */

const server = nailPerTest({ seed: "demo-multi" });

/** A query over both demo accounts with three hits (as in `search-threads.spec.ts`). */
const VALID = 'subject:"garden party" OR from:users@lists.nail.example';
const VALID_HITS = 3;

const searchBox = (page: Page) => page.getByRole("searchbox", { name: "Search mail" });
const field = (page: Page) => page.locator('[data-part="search-bar.field"]');
const indicator = (page: Page) => page.locator('[data-part="search-bar.error-indicator"]');
const tip = (page: Page) => page.locator('[data-part="search-bar.error"]');

async function ready(page: Page, size?: { width: number; height: number }) {
  await openMail(page, server.url, {
    rows: DEMO_MULTI_CONVERSATIONS,
    ...(size ? { size } : {}),
  });
}

/** Types `text` key by key (no Enter) and waits for the search's hits. */
async function typeAndWait(page: Page, text: string, hits: number) {
  const input = searchBox(page);
  await input.click();
  await input.pressSequentially(text, { delay: 10 });
  await expect(messageList(page)).toHaveAttribute("data-state", "search");
  await expect(messageRows(page)).toHaveCount(hits);
}

test("typing searches after a pause, without Enter", async ({ page }) => {
  await ready(page);
  await typeAndWait(page, VALID, VALID_HITS);
  await expect(messageRows(page).filter({ hasText: "Garden party on Saturday" })).toHaveCount(1);

  // A refinement replaces the results once typing pauses again.
  await searchBox(page).pressSequentially(" is:unread", { delay: 10 });
  await expect(page.locator('[data-part="message-list.search-summary"]')).toContainText(
    "is:unread",
  );

  // Emptying the field ends the search: back to the folder.
  await searchBox(page).fill("");
  await expect(messageList(page)).not.toHaveAttribute("data-state", "search");
  await expect(messageRows(page)).toHaveCount(DEMO_MULTI_CONVERSATIONS);
});

test("Enter searches at once", async ({ page }) => {
  await ready(page);
  const input = searchBox(page);
  await input.fill(VALID);
  await input.press("Enter");
  await expect(messageRows(page)).toHaveCount(VALID_HITS);
});

test("an invalid query keeps the list and explains the mistake over it", async ({ page }) => {
  await ready(page);
  await typeAndWait(page, VALID, VALID_HITS);
  const list = messageList(page);
  const before = await list.boundingBox();

  const input = searchBox(page);
  await input.fill("frm:ada");
  await expect(indicator(page)).toBeVisible();
  await expect(input).toHaveAttribute("aria-invalid", "true");
  await expect(field(page)).toHaveAttribute("data-state", "invalid");
  // Focus in the field shows the explanation; it names the mistake.
  await expect(tip(page)).toBeVisible();
  await expect(tip(page)).toHaveAttribute("data-kind", "unknown-field");
  await expect(tip(page).locator('[data-part="search-bar.error.mark"]')).toHaveText("frm:");
  await expect(input).toHaveAttribute(
    "aria-describedby",
    (await tip(page).getAttribute("id")) ?? "",
  );
  // Enter does not search an invalid query: the last results stay.
  await input.press("Enter");
  await expect(messageRows(page)).toHaveCount(VALID_HITS);
  await expect(tip(page)).toContainText("The list still shows the results for");
  // The list did not move: the explanation overlays it.
  expect(await list.boundingBox()).toEqual(before);

  // Without focus or pointer the explanation hides; hovering shows it again.
  await input.blur();
  await page.mouse.move(0, 0);
  await expect(tip(page)).toBeHidden();
  await expect(indicator(page)).toBeVisible();
  await field(page).hover();
  await expect(tip(page)).toBeVisible();
  expect(await list.boundingBox()).toEqual(before);
  await expect(messageRows(page)).toHaveCount(VALID_HITS);
});

test("the indicator and its explanation fit at 1024×700", async ({ page }) => {
  const size = { width: 1024, height: 700 };
  await ready(page, size);
  const input = searchBox(page);
  await input.fill("frm:ada");
  await expect(indicator(page)).toBeVisible();
  await expectResponsive(page, "r1-live-search-invalid", {
    sizes: [size],
    prepare: async (p) => {
      // Hovered, not focused: the explanation without the field hints.
      await searchBox(p).blur();
      await field(p).hover();
      await expect(tip(p)).toBeVisible();
    },
  });
  // The explanation stays inside the window.
  const box = await tip(page).boundingBox();
  expect(box).not.toBeNull();
  expect((box?.x ?? 0) + (box?.width ?? 0)).toBeLessThanOrEqual(size.width);
});
