/**
 * Test helpers of the rules feature (only tests import this file).
 */
import { within } from "@testing-library/react";
import type { UserEvent } from "@testing-library/user-event";

/** Switches the rule editor to its text view; returns the text field. */
export async function showTextView(
  user: UserEvent,
  editor: HTMLElement,
): Promise<HTMLTextAreaElement> {
  await user.click(within(editor).getByRole("button", { name: "Text" }));
  return within(editor).findByRole<HTMLTextAreaElement>("textbox", {
    name: "The rule in rules.kdl syntax",
  });
}
