/**
 * Test helpers of the quick look, the invitation card and the reminder
 * toast (R7, ADR 0246): their secondary actions sit in menu buttons
 * (`PimMenu`), so tests open the menu and choose an item by name.
 */
import { screen, within } from "@testing-library/react";
import type { UserEvent } from "@testing-library/user-event";

/** Opens the menu button `menu` inside `scope` and chooses the item `item`. */
export async function chooseFromMenu(
  user: UserEvent,
  scope: HTMLElement,
  menu: string,
  item: string,
): Promise<void> {
  await user.click(within(scope).getByRole("button", { name: menu }));
  await user.click(screen.getByRole("menuitem", { name: item }));
}
