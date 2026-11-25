import { expect, test } from "@playwright/test";
import { nailPerTest } from "../support/nail";
import { saveSettings } from "../support/settings";
import { DEMO_MULTI_CONVERSATIONS, openMail, openSettings } from "../support/views";

/**
 * R8 review, against `nail serve --seed demo-multi` (accounts whose junk
 * filter can be set): the junk filter's "not junk" limit is checked against
 * the threshold, saved in the core and shown again after a reload.
 */

const server = nailPerTest({ seed: "demo-multi" });

test('the junk filter\'s "not junk" limit is checked against the threshold and saved', async ({
  page,
}) => {
  await openMail(page, server.url, { rows: DEMO_MULTI_CONVERSATIONS });
  const view = await openSettings(page, "Mail & junk");
  const junk = view.locator('[data-part="junk-settings"]');
  await junk.getByRole("spinbutton", { name: "Junk threshold (percent)" }).fill("60");
  const limit = junk.getByRole("spinbutton", { name: '"Not junk" limit (percent)' });
  await limit.fill("70");
  await expect(junk.getByText('Choose a "not junk" limit below the junk threshold.')).toBeVisible();
  await limit.fill("40");
  await expect(junk.getByText('Choose a "not junk" limit below the junk threshold.')).toHaveCount(
    0,
  );
  await saveSettings(page);
  // Saved in the core: shown again after the page reloads.
  await page.reload();
  const again = await openSettings(page, "Mail & junk");
  await expect(
    again
      .locator('[data-part="junk-settings"]')
      .getByRole("spinbutton", { name: '"Not junk" limit (percent)' }),
  ).toHaveValue("40");
});
