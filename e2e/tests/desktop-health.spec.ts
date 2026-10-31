import { chromium, expect, test, type Page } from "@playwright/test";
import { desktopPerTest, desktopTests, shellControl } from "../support/desktop";
import { messageRows } from "../support/views";
import { defined } from "../support/defined";
import { UI_LOG_TARGET } from "../../ui/src/api/generated/constants";

/**
 * The desktop app keeps its window alive (ADR 0207): when WebView2's
 * render process of the main window dies, the shell logs it and reloads
 * the page, which comes back showing the mail; the page's own start
 * reports reach the core's app.log.
 *
 * Needs Windows with the WebView2 runtime and `npm run build:desktop`. In
 * CI it runs only with `NAIL_E2E_DESKTOP=1`; locally it always runs.
 */

desktopTests();

const launchApp = desktopPerTest();

interface Health {
  reloads: number;
  gave_up: boolean;
}

/** The page's lines of the app log (`GetAppLog` through Tauri IPC). */
async function pageLogLines(page: Page): Promise<string[]> {
  const answer = (await page.evaluate(async () => {
    const internals = (
      window as unknown as {
        __TAURI_INTERNALS__: { invoke(cmd: string, args: unknown): Promise<unknown> };
      }
    ).__TAURI_INTERNALS__;
    return internals.invoke("nail_call", { request: { type: "GetAppLog" } });
  })) as { log?: { lines: string[] } };
  return (answer.log?.lines ?? []).filter((line) => line.includes(UI_LOG_TARGET));
}

/** How many page starts the lines name. */
const starts = (lines: string[]) => lines.filter((line) => line.includes("page started")).length;

test("a crashed page is logged and reloaded", async () => {
  // The page's reports are info lines.
  const { app, page } = await launchApp({ env: { NAIL_LOG: `warn,${UI_LOG_TARGET}=info` } });
  await expect(messageRows(page).first()).toBeVisible({ timeout: 60_000 });
  await expect.poll(async () => starts(await pageLogLines(page))).toBeGreaterThanOrEqual(1);

  // Kill the main window's render process, as a crash or the OS would.
  // Playwright gives up on a crashed page: a new connection finds the
  // reloaded one.
  const cdp = await page.context().newCDPSession(page);
  void cdp.send("Page.crash").catch(() => undefined);
  await page.waitForEvent("crash").catch(() => undefined);

  // Any page of a new connection that shows the mail again (the crashed
  // target may still be listed).
  const browser = await chromium.connectOverCDP(app.cdpUrl);
  try {
    let live: Page | undefined;
    await expect
      .poll(
        async () => {
          for (const candidate of browser.contexts().flatMap((c) => c.pages())) {
            const rows = await candidate
              .evaluate(() => document.querySelectorAll('[data-part="message-list.row"]').length)
              .catch(() => 0);
            if (rows > 0) {
              live = candidate;
              return true;
            }
          }
          return false;
        },
        { timeout: 60_000 },
      )
      .toBe(true);
    const reloaded = defined(live, "the reloaded page");
    const health = (await shellControl(reloaded, "webview-health")) as Record<string, Health>;
    expect(health.main).toEqual({ reloads: 1, gave_up: false });
    // The reloaded page reported its start again.
    await expect.poll(async () => starts(await pageLogLines(reloaded))).toBeGreaterThanOrEqual(2);
  } finally {
    await browser.close().catch(() => undefined);
  }
});
