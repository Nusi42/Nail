import { expect, type Page } from "@playwright/test";
import { nailFixture } from "../support/nail";
import { commandManifest, installPlugin, packFiles } from "../support/plugins";
import { messageRows, readerAction, runFromPalette } from "../support/views";
import { openMailAs, type ViewerOs } from "../support/windowTour";

/**
 * R8 (mail and windows UI) against a private `nail serve --seed demo`: a
 * plug-in's dialog with several secondary buttons uses the shared footer
 * (one group in the secondary place, mirrored on macOS), and the plug-in
 * manager window keeps its unsaved changes apart from the Settings page.
 */
const test = nailFixture();

/** A plug-in whose reader command asks with three buttons besides Cancel. */
const ASKING = packFiles({
  manifest: commandManifest("asker", "Asker", { id: "ask", title: "Ask me" }, ["ui.dialogs"]),
  main: `export default {
    activate(ctx) {
      ctx.commands.register("ask", async () => {
        await nail.ui.showDialog({
          title: "Choose",
          body: [{ type: "text", value: "Pick one" }],
          buttons: [
            { id: "later", label: "Later" },
            { id: "go", label: "Go", primary: true },
            { id: "skip", label: "Skip it" },
          ],
        });
      });
    },
  };`,
});

/** The labels of a footer's end group, left to right. */
const endLabels = (page: Page) =>
  page
    .getByRole("dialog", { name: "Choose" })
    .locator('[data-part="dialog.footer.end"] button')
    .allTextContents();

for (const [os, order] of [
  ["windows", ["Go", "Later", "Skip it", "Cancel"]],
  ["macos", ["Cancel", "Skip it", "Later", "Go"]],
] as const satisfies readonly (readonly [ViewerOs, readonly string[]])[]) {
  test(`a plug-in dialog's secondary buttons share the footer's secondary place (${os})`, async ({
    page,
    nail,
  }) => {
    await openMailAs(page, nail.url, os);
    await installPlugin(page, ASKING, /^Asker 1\.0\.0$/, { place: true });
    await runFromPalette(page, "mail", /^Mail/);
    await messageRows(page).first().click();
    await readerAction(page, "Ask me");
    const dialog = page.getByRole("dialog", { name: "Choose" });
    await expect(dialog).toBeVisible();
    expect(await endLabels(page)).toEqual(order);
    await expect(dialog.locator('[data-part="dialog.footer"] > button')).toHaveCount(0);
    await dialog.getByRole("button", { name: "Skip it" }).click();
    await expect(dialog).toBeHidden();
  });
}

test("the plug-in manager window keeps its unsaved changes apart from Settings", async ({
  page,
  nail,
}) => {
  await openMailAs(page, nail.url, "windows");
  // The Settings page's Plug-ins section, and the manager window over it.
  const settings = await installPlugin(page, ASKING, /^Asker 1\.0\.0$/);
  await runFromPalette(page, "manage plug-ins", "Manage plug-ins");
  const manager = page.getByRole("dialog", { name: "Plug-ins" });
  await expect(manager).toBeVisible();
  const unsaved = /^Unsaved changes to “/;
  await manager.getByRole("switch", { name: "Asker is on" }).click();
  // Its own bar shows the change; the Settings page has none and shows the saved state.
  await expect(manager.getByRole("group", { name: unsaved })).toBeVisible();
  await expect(settings.getByRole("group", { name: unsaved })).toHaveCount(0);
  await expect(settings.getByRole("switch", { name: "Asker is on" })).toBeChecked();
  // Discarding in the window drops its change.
  await manager
    .getByRole("group", { name: unsaved })
    .getByRole("button", { name: "Discard", exact: true })
    .click();
  await expect(manager.getByRole("switch", { name: "Asker is on" })).toBeChecked();
  await expect(manager.getByRole("group", { name: unsaved })).toHaveCount(0);
});
