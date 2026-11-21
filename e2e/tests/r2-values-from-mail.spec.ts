import { expect, test } from "@playwright/test";
import { nailPerTest } from "../support/nail";
import { ruleFromMessageRow } from "../support/rules";

/**
 * R2 (ADR 0222): values from the selected mail. Opened from a mail's row
 * menu, the rules window offers that mail's values beside a condition's
 * value field; picking the sender's domain turns the sender condition into
 * `from ends-with "@<domain>"`, and Test rule finds the mail it came from.
 */

const server = nailPerTest({ seed: "demo-multi" });

const SUBJECT = "Release notes for M1";
const DOMAIN = "lists.nail.example";

test("a rule from a mail's row menu takes the sender domain from that mail", async ({ page }) => {
  await page.goto(server.url);
  const editor = await ruleFromMessageRow(page, SUBJECT);
  const sender = editor
    .locator('[data-part="rules.condition"]')
    .filter({ has: page.getByRole("combobox", { name: "Field" }) })
    .first();
  await expect(sender.getByRole("combobox", { name: "Text" })).toHaveValue(
    /@lists\.nail\.example$/,
  );

  await sender.getByRole("button", { name: "Values from this mail" }).click();
  const menu = page.getByRole("menu", { name: "Values from this mail" });
  await expect(menu.getByText("From", { exact: true })).toBeVisible();
  // The From group comes first (the condition's own field); To has the domain too.
  await menu
    .getByRole("menuitem", { name: `Domain: ${DOMAIN}` })
    .first()
    .click();
  await expect(menu).toBeHidden();
  await expect(sender.getByRole("combobox", { name: "Text" })).toHaveValue(`@${DOMAIN}`);
  await expect(sender.getByRole("combobox", { name: "Comparison" })).toHaveValue("ends-with");

  await editor.getByRole("button", { name: "Test rule" }).click();
  const results = editor.getByRole("region", { name: "Test results" });
  await expect(results.getByRole("status")).toHaveText(
    /^(One message|[1-9]\d* messages) of \d+ match/,
  );
  await expect(
    results.locator('[data-part="rules.test.match"]').filter({ hasText: SUBJECT }),
  ).not.toHaveCount(0);
});
