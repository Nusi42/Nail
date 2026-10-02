import { existsSync, readFileSync } from "node:fs";
import path from "node:path";
import { expect, test } from "@playwright/test";
import { connectDesktop, skipFirstRun, startDesktop, type DesktopApp } from "../support/desktop";
import { freePort, tempDir } from "../support/nail";

/**
 * M4c WP125 on the real desktop app (ADR 0099): "Save as PDF" of a message
 * writes a PDF file through the shell — a hidden window loads the print
 * view (`#/print/<job>`), waits for its ready signal and prints it with
 * WebView2's PrintToPdf.
 *
 * Needs Windows with the WebView2 runtime and `npm run build:desktop`. In
 * CI it runs only with `NAIL_E2E_DESKTOP=1`; locally it always runs.
 */

test.skip(process.platform !== "win32", "the desktop app uses WebView2 (Windows)");
test.skip(
  !!process.env.CI && !process.env.NAIL_E2E_DESKTOP,
  "set NAIL_E2E_DESKTOP=1 to run the desktop smoke tests in CI",
);
test.describe.configure({ mode: "serial", timeout: 240_000 });

let app: DesktopApp | undefined;

test.afterEach(async () => {
  await app?.stop();
  app = undefined;
});

test("print to PDF writes a PDF file of the message", async () => {
  app = await startDesktop(freePort());
  const { browser, page } = await connectDesktop(app);
  try {
    await skipFirstRun(page);
    await page
      .locator('[data-part="message-list.row"]')
      .filter({ hasText: "Welcome to Nail" })
      .click();
    await expect(page.locator('[data-part="message-view.header.subject"]')).toContainText(
      "Welcome to Nail",
    );
    await page.keyboard.press("Control+p");
    const dialog = page.getByRole("dialog", { name: "Print" });
    await expect(dialog).toBeVisible();
    await dialog.getByRole("button", { name: "Save as PDF…" }).click();
    const target = path.join(tempDir("pdf"), "welcome.pdf");
    await dialog.getByLabel("PDF file").fill(target);
    await dialog.getByRole("button", { name: "Save PDF" }).click();
    await expect(dialog.getByText(/^Saved to /)).toBeVisible({ timeout: 120_000 });
    expect(existsSync(target)).toBe(true);
    const pdf = readFileSync(target);
    expect(pdf.subarray(0, 5).toString("latin1")).toBe("%PDF-");
    expect(pdf.length).toBeGreaterThan(1000);
    // The hidden print window is gone again.
    await expect.poll(() => browser.contexts().flatMap((c) => c.pages()).length).toBe(1);
  } finally {
    await browser.close().catch(() => undefined);
  }
});
