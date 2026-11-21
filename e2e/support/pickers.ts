import { expect, type Locator } from "@playwright/test";

/**
 * The shared pickers (ADRs 0222, 0227) in Playwright: open a picker by its
 * browse button and click an option, and the chips of a multi-value picker.
 */

/** Opens the picker labelled `label` inside `scope` and clicks the option named `option`. */
export async function pickOption(
  scope: Locator,
  label: string,
  option: string | RegExp,
): Promise<void> {
  const field = scope.getByRole("combobox", { name: label, exact: true });
  await scope.getByRole("button", { name: `Show choices for ${label}`, exact: true }).click();
  const role = (await field.getAttribute("aria-haspopup")) === "tree" ? "treeitem" : "option";
  const list = scope.getByRole(role === "treeitem" ? "tree" : "listbox", {
    name: `Choices for ${label}`,
    exact: true,
  });
  await list.getByRole(role, { name: option }).first().click();
}

/** The chips of the multi-value picker labelled `label` inside `scope`. */
export const chipsOf = (scope: Locator, label: string): Locator =>
  scope
    .getByRole("list", { name: `Chosen for ${label}`, exact: true })
    .locator('[data-part="picker.chip.label"]');

/** Expects the chips of the multi-value picker labelled `label` to be `names`. */
export async function expectChips(
  scope: Locator,
  label: string,
  names: readonly (string | RegExp)[],
): Promise<void> {
  if (names.length === 0) {
    await expect(scope.getByRole("list", { name: `Chosen for ${label}`, exact: true })).toHaveCount(
      0,
    );
  } else {
    await expect(chipsOf(scope, label)).toHaveText([...names]);
  }
}
