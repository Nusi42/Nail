import { existsSync, readFileSync } from "node:fs";
import path from "node:path";
import { expect, test } from "@playwright/test";
import { expectWindowsInBackground, desktopTests, desktopPerTest } from "../support/desktop";
import { tempDir } from "../support/nail";
import { READER_SUBJECT, messageRows } from "../support/views";

/**
 * M4c WP125 on the real desktop app (ADR 0099): "Save as PDF" of a message
 * writes a PDF file through the shell — a hidden window loads the print
 * view (`#/print/<job>`), waits for its ready signal and prints it with
 * WebView2's PrintToPdf.
 *
 * Needs Windows with the WebView2 runtime and `npm run build:desktop`. In
 * CI it runs only with `NAIL_E2E_DESKTOP=1`; locally it always runs.
 */

desktopTests();

const launch = desktopPerTest();

test("print to PDF writes a PDF file of the message", async () => {
  const { browser, page } = await launch();
  await messageRows(page).filter({ hasText: "Welcome to Nail" }).click();
  await expect(page.locator(READER_SUBJECT)).toContainText("Welcome to Nail");
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
  await expectWindowsInBackground(page);
  // The PDF went through a temporary file next to the target.
  expect(existsSync(`${target}.nail-part`)).toBe(false);

  // Post-M4c: a typed path never replaces an existing file — only the
  // Save dialog's confirmation (`overwrite`) would.
  // The footer's Close (the title bar has a Close button too).
  await dialog.locator('[data-part="form.button"][data-action="close"]').click();
  await expect(dialog).toBeHidden();
  await page.keyboard.press("Control+p");
  await expect(dialog).toBeVisible();
  await dialog.getByRole("button", { name: "Save as PDF…" }).click();
  await dialog.getByLabel("PDF file").fill(target);
  await dialog.getByRole("button", { name: "Save PDF" }).click();
  await expect(dialog.getByRole("alert")).toHaveText(
    "The target exists. Choose another name or allow replacing it.",
  );
  expect(readFileSync(target).equals(pdf)).toBe(true);
});
