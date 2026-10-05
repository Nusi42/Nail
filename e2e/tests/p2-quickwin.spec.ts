import { mkdirSync } from "node:fs";
import path from "node:path";
import { expect, test, type Page } from "@playwright/test";
import { buildOutputDir, startNail, type NailServer } from "../support/nail";
import { rpcOk } from "../support/remote";
import { layoutIssues, formatIssues } from "../support/responsive";

/**
 * P2 quick win (Design C, ADR 0148) on the real core: the list shows one row
 * per conversation with its count pill, and the reader's strip shows the
 * newest three messages of the selected conversation with the older ones
 * folded into "N earlier messages" — no band, no scroll area of its own.
 * Screenshots for the owner (light Frost and dark Graphite, 1600 × 1000 and
 * 1024 × 700): `<build dir>/screens/p2/quickwin/`.
 */

const SIZES = [
  { width: 1600, height: 1000 },
  { width: 1024, height: 700 },
] as const;
const THEMES = ["frost", "graphite"] as const;

let server: NailServer;

test.beforeEach(async () => {
  server = await startNail({ seed: "demo-multi" });
});

test.afterEach(async () => {
  await server.stop();
});

const rows = (page: Page) => page.locator('[data-part="message-list.row"]');

function screenshotPath(name: string): string {
  const dir = buildOutputDir(path.join("screens", "p2", "quickwin"));
  mkdirSync(dir, { recursive: true });
  return path.join(dir, `${name}.png`);
}

/** Selects the long Nightingale conversation (four in the Inbox, three replies in Sent). */
async function openNightingale(page: Page) {
  const row = rows(page).filter({ hasText: "Project Nightingale kickoff" });
  await expect(row).toHaveCount(1);
  await row.click();
  const strip = page.locator('[data-part="thread-view"]');
  await expect(strip).toBeVisible();
  return { row, strip };
}

test("conversations show as one row with a count, the strip folds older messages", async ({
  page,
}) => {
  await page.setViewportSize(SIZES[0]);
  await page.goto(server.url);
  // 32 messages in the unified inbox are 17 conversations (P2 demo threads).
  await expect(rows(page)).toHaveCount(17);
  const { row, strip } = await openNightingale(page);
  await expect(row).toHaveAttribute("data-state", /\bthread\b/);
  // Four in the Inbox (the contract stub; WP201 joins the three replies from
  // Sent, ADR 0148, and this becomes 7).
  await expect(row.locator('[data-part="message-list.row.count"]')).toHaveText("4");
  // A conversation of one message has no count.
  const single = rows(page).filter({ hasText: "Welcome to Nail" });
  await expect(single.locator('[data-part="message-list.row.count"]')).toHaveText("");
  await expect(single.locator('[data-part="message-list.row.count"]')).toBeHidden();

  // The strip: the newest three, the other four folded; no own scroll bar.
  const items = strip.locator('[data-part="thread-view.item"]');
  await expect(items).toHaveCount(3);
  const fold = strip.getByRole("button", { name: "4 earlier messages" });
  await expect(fold).toHaveAttribute("aria-expanded", "false");
  const style = await strip.evaluate((el) => {
    const s = getComputedStyle(el);
    return { overflowY: s.overflowY, maxHeight: s.maxHeight };
  });
  expect(style).toEqual({ overflowY: "visible", maxHeight: "none" });
  const avatar = items.first().locator('[data-part="thread-view.item.avatar"]');
  await expect(avatar).toHaveCSS("width", "28px");
  await fold.click();
  await expect(items).toHaveCount(7);
});

for (const theme of THEMES) {
  test(`quick-win screenshots in ${theme}`, async ({ page }) => {
    await rpcOk(server, { type: "SetTheme", theme_id: theme }, "Themes");
    for (const size of SIZES) {
      await page.setViewportSize(size);
      await page.goto(server.url);
      await expect(rows(page)).toHaveCount(17);
      const { strip } = await openNightingale(page);
      await expect(strip.locator('[data-part="thread-view.item"]')).toHaveCount(3);
      await expect(page.locator('[data-part="message-view.header.subject"]')).toBeVisible();
      // The strip and the list keep to the responsive standard.
      const issues = (await layoutIssues(page)).filter(
        (i) => i.element.includes("thread-view") || i.element.includes("message-list"),
      );
      expect(issues, formatIssues(issues)).toEqual([]);
      await page.screenshot({
        path: screenshotPath(`mail-${theme}-${String(size.width)}x${String(size.height)}`),
      });
    }
  });
}
