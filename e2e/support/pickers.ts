import { expect, type Locator } from "@playwright/test";

/**
 * The shared pickers (ADRs 0222, 0227) in Playwright: open a picker by its
 * browse button and click an option, and the chips of a multi-value picker.
 */

/**
 * Opens the list of the picker labelled `label` inside `scope` by its
 * browse button: the list, and the role of its rows (`treeitem` for folders).
 */
export async function openChoices(
  scope: Locator,
  label: string,
): Promise<{ list: Locator; role: "option" | "treeitem" }> {
  const field = scope.getByRole("combobox", { name: label, exact: true });
  await scope.getByRole("button", { name: `Show choices for ${label}`, exact: true }).click();
  const role = (await field.getAttribute("aria-haspopup")) === "tree" ? "treeitem" : "option";
  const list = scope.getByRole(role === "treeitem" ? "tree" : "listbox", {
    name: `Choices for ${label}`,
    exact: true,
  });
  return { list, role };
}

/** Opens the picker labelled `label` inside `scope` and clicks the option named `option`. */
export async function pickOption(
  scope: Locator,
  label: string,
  option: string | RegExp,
): Promise<void> {
  const { list, role } = await openChoices(scope, label);
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
