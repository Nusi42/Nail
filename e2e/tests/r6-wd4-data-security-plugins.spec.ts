import { expect, test, type Locator, type Page } from "@playwright/test";
import { axeViolations } from "../support/axe";
import { openWorkLog, workLogWindow } from "../support/connlog";
import { nailPerTest } from "../support/nail";
import { createUser } from "../support/remote";
import { openMail, openSettings, windowById } from "../support/views";
import { emulateViewerOs, expectDialogFooter, type ViewerOs } from "../support/windowTour";

/**
 * One dialog pattern everywhere (R6, ADR 0239) in the data, security,
 * plug-in and system dialogs, against `nail serve --seed demo-multi`:
 *
 * - the diagnostics wizard: Next and Cancel, then Save bundle, Back and
 *   Cancel; the backup and import wizards: their verb and Cancel;
 * - Pair a device: Create code and Cancel; the code step only Done, with
 *   Revoke code beside the code;
 * - the plug-in install wizard: Check the plug-in and Cancel;
 * - Clear… of the connection log: a destructive confirmation (Cancel
 *   focused, Enter keeps the log);
 * - the application log: a tool window without a footer (× only).
 *
 * Each runs as Windows and as macOS (the order mirrors).
 */

const server = nailPerTest({ seed: "demo-multi" });

/** The page as a viewer on `os`, with the mail view loaded. */
async function openAs(page: Page, os: ViewerOs): Promise<void> {
  await emulateViewerOs(page, os);
  await openMail(page, server.url);
}

/** Settings → `section`, then the button `button`: the window `name`. */
async function openFromSettings(
  page: Page,
  section: string,
  button: string,
  name: string,
): Promise<Locator> {
  const settings = await openSettings(page, section);
  await settings.getByRole("button", { name: button }).first().click();
  const dialog = windowById(page, name);
  await expect(dialog).toBeVisible();
  return dialog;
}

for (const os of ["windows", "macos"] as const) {
  test(`data wizards: Cancel on every step, Back beside the step verb (${os})`, async ({
    page,
  }) => {
    await openAs(page, os);
    const diagnostics = await openFromSettings(
      page,
      "Data & backup",
      "Create diagnostics bundle…",
      "diagnostics-bundle",
    );
    await expectDialogFooter(diagnostics, { os, roles: ["primary", "cancel"] });
    await diagnostics.getByRole("button", { name: "Next" }).click();
    await expect(
      diagnostics.getByRole("region", { name: "The bundle will contain exactly these files" }),
    ).toBeVisible();
    await expectDialogFooter(diagnostics, { os, roles: ["primary", "secondary", "cancel"] });
    expect(await axeViolations(page, '[data-window="diagnostics-bundle"]')).toEqual([]);
    await diagnostics.getByRole("button", { name: "Back" }).click();
    await expect(diagnostics.getByRole("button", { name: "Next" })).toBeVisible();
    await diagnostics.getByRole("button", { name: "Cancel" }).click();
    await expect(diagnostics).toBeHidden();

    for (const [button, name] of [
      ["Back up…", "backup-wizard"],
      ["Import…", "import-wizard"],
      ["Restore…", "restore-wizard"],
    ] as const) {
      const wizard = await openFromSettings(page, "Data & backup", button, name);
      await expectDialogFooter(wizard, { os, roles: ["primary", "cancel"] });
      await wizard.getByRole("button", { name: "Cancel" }).click();
      await expect(wizard).toBeHidden();
    }
  });

  test(`Pair a device: Create code, then Revoke code and Done (${os})`, async ({ page }) => {
    await createUser(server, "ada");
    await openAs(page, os);
    const pair = await openFromSettings(page, "Remote access", "Pair a device", "remote-pair");
    await expectDialogFooter(pair, { os, roles: ["primary", "cancel"] });
    await pair.getByRole("button", { name: "Create code" }).click();
    await expect(pair.locator('[data-part="remote-pair.code"]')).toBeVisible();
    await expectDialogFooter(pair, { os, roles: ["done"] });
    await expect(pair.getByRole("button", { name: "Revoke code" })).toHaveAttribute(
      "data-variant",
      "danger",
    );
    expect(await axeViolations(page, '[data-window="remote-pair"]')).toEqual([]);
    await pair.getByRole("button", { name: "Revoke code" }).click();
    await expect(pair.getByText("This code was cancelled.")).toBeVisible();
    await pair.getByRole("button", { name: "Done" }).click();
    await expect(pair).toBeHidden();
  });

  test(`plug-in install wizard: Check the plug-in and Cancel (${os})`, async ({ page }) => {
    await openAs(page, os);
    const install = await openFromSettings(
      page,
      "Plug-ins",
      "Install a plug-in…",
      "plugin-install",
    );
    await expectDialogFooter(install, { os, roles: ["primary", "cancel"] });
    await install.getByRole("button", { name: "Cancel" }).click();
    await expect(install).toBeHidden();
  });

  test(`connection log: Clear… asks, Cancel is the default (${os})`, async ({ page }) => {
    await openAs(page, os);
    const viewer = await openWorkLog(page);
    const entries = viewer.locator('[data-part="connection-log.entry"]');
    await expect(entries.first()).toBeVisible({ timeout: 20_000 });
    await viewer.getByRole("button", { name: "Clear…" }).click();
    const ask = windowById(page, "connection-log-clear");
    await expect(ask).toBeVisible();
    await expectDialogFooter(ask, { os, roles: ["primary", "cancel"], focused: "cancel" });
    await expect(ask.locator('button[data-role="primary"]')).toHaveAttribute(
      "data-variant",
      "danger",
    );
    expect(await axeViolations(page, '[data-window="connection-log-clear"]')).toEqual([]);
    // Enter on the focused Cancel: the log stays.
    await page.keyboard.press("Enter");
    await expect(ask).toBeHidden();
    await expect(entries.first()).toBeVisible();
    await viewer.getByRole("button", { name: "Clear…" }).click();
    await ask.getByRole("button", { name: "Clear connection log" }).click();
    await expect(ask).toBeHidden();
    await expect(workLogWindow(page).getByText(/^Nothing recorded yet/)).toBeVisible();
  });
}

test("the application log is a tool window: × only, no footer", async ({ page }) => {
  await openAs(page, "windows");
  const log = await openFromSettings(page, "Data & backup", "Show recent lines", "application-log");
  await expect(log.locator('[data-part="dialog.footer"]')).toHaveCount(0);
  await expect(log.getByRole("button", { name: "Refresh" })).toBeVisible();
  await log.locator('[data-part="window.close"]').click();
  await expect(log).toBeHidden();
});
