import { readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { expect, test, type Page } from "@playwright/test";
import { repoRoot, startNail, type NailServer } from "../support/nail";
import { rpcOk } from "../support/remote";
import { expectResponsive } from "../support/responsive";

/**
 * P2 WP202 (ADR 0148): the conversation list on the real core — Design A
 * rows (people, count, subject, "Sender: preview"; two lines in Compact)
 * and the opt-in Design B (`threads="expandable"`: a chevron, child rows,
 * → / ← and j / k), each kept to the responsive standard at every size in
 * light, dark, Compact and High contrast. Screenshots:
 * `<build dir>/screens/p1/p2-list-*` (the standard's folder).
 */

let server: NailServer;

test.afterEach(async () => {
  await server.stop();
});

const rows = (page: Page) => page.locator('[data-part="message-list.row"]');
const SUBJECT = "Project Nightingale kickoff";

/** The default layout with the mail window's lists set to `threads="expandable"`. */
function expandableLayout(): string {
  const defaults = readFileSync(path.join(repoRoot, "defaults", "layout.kdl"), "utf8");
  const start = defaults.indexOf("    // The mail view.");
  const end = defaults.indexOf("    // The calendar, contacts and tasks views");
  const mail = defaults.slice(start, end).replaceAll('threads="collapsed"', 'threads="expandable"');
  expect(mail).toContain('threads="expandable"');
  return `layout "Mine" extends="default" {\n${mail}}\n`;
}

async function appearance(theme: string, density: "comfortable" | "compact") {
  await rpcOk(server, { type: "SetTheme", theme_id: theme }, "Themes");
  await rpcOk(
    server,
    {
      type: "SetThemePreferences",
      preferences: {
        follow_system: false,
        light: "frost",
        dark: "graphite",
        density,
        overrides: {},
      },
    },
    "Themes",
  );
}

/** Only the list's elements count (other packages own the rest of the window). */
const LIST_SCOPE = '[data-part="message-list"]';

const nightingale = (page: Page) => rows(page).filter({ hasText: SUBJECT });

test("conversation rows show people, count, subject and the newest sender's preview", async ({
  page,
}) => {
  server = await startNail({ seed: "demo-multi" });
  await page.setViewportSize({ width: 1600, height: 1000 });
  await page.goto(server.url);
  const row = nightingale(page);
  await expect(row).toHaveCount(1);
  await expect(row.locator('[data-part="message-list.row.participants"]')).not.toBeEmpty();
  await expect(row.locator('[data-part="message-list.row.count"]')).toHaveText(/^\d+$/);
  await expect(row.locator('[data-part="message-list.row.subject"]')).toHaveText(SUBJECT);
  // "Sender:" and the preview of the newest message.
  await expect(row.locator('[data-part="message-list.row.latest-from"]')).toHaveText(/\S:$/);
  await expect(row.locator('[data-part="message-list.row.preview"]')).not.toBeEmpty();
  const lines = row.locator('[data-part="message-list.row.line"]');
  await expect(lines).toHaveCount(3);

  // Compact: two lines, no preview.
  await appearance("frost", "compact");
  await expect(lines.nth(2)).toBeHidden();
  await expect(lines.nth(0)).toBeVisible();
  await expect(lines.nth(1)).toBeVisible();
  // Unread senders stay bold on the first line.
  const unread = page
    .locator(
      '[data-part="message-list.row.participants"] [data-part="message-list.row.participant"]',
    )
    .and(page.locator('[data-state~="unread"]'))
    .first();
  await expect(unread).toHaveCSS("font-weight", /^[6-9]00$/);
});

const SWEEPS = [
  { theme: "frost", density: "comfortable" },
  { theme: "graphite", density: "compact" },
  { theme: "high-contrast", density: "comfortable" },
] as const;

for (const { theme, density } of SWEEPS) {
  test(`conversation rows at every size in ${theme} (${density})`, async ({ page }) => {
    test.setTimeout(180_000);
    server = await startNail({ seed: "demo-multi" });
    await appearance(theme, density);
    await page.goto(server.url);
    await expect(nightingale(page)).toHaveCount(1);
    await expectResponsive(page, `p2-list-rows-${theme}-${density}`, {
      scope: LIST_SCOPE,
      prepare: async () => {
        // The narrow layout shows one pane at a time: the list's tab.
        const tab = page.getByRole("tab", { name: "Messages", exact: true });
        if (await tab.isVisible()) await tab.click();
        await nightingale(page).click();
      },
    });
  });
}

test("expandable conversations open into their messages", async ({ page }) => {
  server = await startNail({ seed: "demo-multi" });
  writeFileSync(path.join(server.configDir, "layout.kdl"), expandableLayout());
  await page.setViewportSize({ width: 1600, height: 1000 });
  await page.goto(server.url);
  const tree = page.getByRole("tree", { name: "Messages" });
  await expect(tree).toBeVisible();
  const row = nightingale(page);
  await expect(row).toHaveAttribute("aria-expanded", "false");
  await row.locator('[data-part="message-list.row.expander"]').click();
  await expect(row).toHaveAttribute("aria-expanded", "true");
  const children = tree.locator('[data-part="message-list.child"]');
  const count = Number(await row.locator('[data-part="message-list.row.count"]').innerText());
  // The conversation's messages (until WP201 the core's stub answers with
  // every folder's copies, so there may be more than the pill counts).
  await expect.poll(() => children.count()).toBeGreaterThanOrEqual(count);
  await expect(children.first()).toHaveAttribute("aria-level", "2");

  // A child opens alone in the reader: no conversation strip.
  await children.nth(1).click();
  await expect(children.nth(1)).toHaveAttribute("aria-selected", "true");
  await expect(page.locator('[data-part="message-view.header.subject"]')).toContainText(
    "Nightingale",
  );
  await expect(page.locator('[data-part="thread-view"]')).toHaveCount(0);

  // ← to the conversation, ← closes it, → opens it again, j moves into it.
  await page.keyboard.press("ArrowLeft");
  await expect(row).toHaveAttribute("aria-selected", "true");
  await expect(page.locator('[data-part="thread-view"]')).toBeVisible();
  await page.keyboard.press("ArrowLeft");
  await expect(children).toHaveCount(0);
  await page.keyboard.press("ArrowRight");
  await expect(row).toHaveAttribute("aria-expanded", "true");
  await page.keyboard.press("j");
  await expect(children.first()).toHaveAttribute("aria-selected", "true");
  await page.keyboard.press("k");
  await expect(row).toHaveAttribute("aria-selected", "true");
});

const EXPANDED_SWEEPS = [
  { theme: "nocturne", density: "comfortable" },
  { theme: "frost", density: "compact" },
  { theme: "high-contrast", density: "compact" },
] as const;

for (const { theme, density } of EXPANDED_SWEEPS) {
  test(`expanded conversations at every size in ${theme} (${density})`, async ({ page }) => {
    test.setTimeout(180_000);
    server = await startNail({ seed: "demo-multi" });
    writeFileSync(path.join(server.configDir, "layout.kdl"), expandableLayout());
    await appearance(theme, density);
    await page.goto(server.url);
    await expect(nightingale(page)).toHaveCount(1);
    await expectResponsive(page, `p2-list-expanded-${theme}-${density}`, {
      scope: LIST_SCOPE,
      prepare: async () => {
        const tab = page.getByRole("tab", { name: "Messages", exact: true });
        if (await tab.isVisible()) await tab.click();
        const row = nightingale(page);
        if ((await row.getAttribute("aria-expanded")) !== "true") {
          await row.locator('[data-part="message-list.row.expander"]').click();
        }
        await expect(row).toHaveAttribute("aria-expanded", "true");
        await page.locator('[data-part="message-list.child"]').nth(1).click();
      },
    });
  });
}
