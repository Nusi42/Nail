/**
 * Test helpers for the shared pickers (unit tests only; not exported by the
 * barrel): open a picker by its browse button and pick an option, and read
 * the chips of a multi-value picker.
 */
import { within } from "@testing-library/react";
import type { UserEvent } from "@testing-library/user-event";

/** Opens the picker labelled `label` inside `scope` and clicks the option named `option`. */
export async function pickOption(
  user: UserEvent,
  scope: HTMLElement,
  label: string,
  option: string | RegExp,
): Promise<void> {
  await user.click(await within(scope).findByRole("button", { name: `Show choices for ${label}` }));
  const input = within(scope).getByRole("combobox", { name: label });
  const role = input.getAttribute("aria-haspopup") === "tree" ? "treeitem" : "option";
  await user.click(await within(scope).findByRole(role, { name: option }));
}

/** The chips (their names) of the multi-value picker labelled `label` inside `scope`. */
export function chipsOf(scope: HTMLElement, label: string): string[] {
  const list = within(scope).queryByRole("list", { name: `Chosen for ${label}` });
  if (!list) return [];
  return within(list)
    .getAllByRole("listitem")
    .map((chip) => chip.querySelector('[data-part="picker.chip.label"]')?.textContent.trim() ?? "");
}
