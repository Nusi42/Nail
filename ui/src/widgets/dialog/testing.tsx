/**
 * Test helpers of the shared question window (only tests import this file).
 */
import { screen, waitFor, within } from "@testing-library/react";
import type { UserEvent } from "@testing-library/user-event";
import { renderWithProviders, type Env } from "../../test/harness";
import { questionOf, QuestionWindow } from "./QuestionWindow";

/**
 * Answers the open question of window `kind` (R6: a destructive
 * confirmation window) with its verb `verb`, after checking that Cancel has
 * the focus and the verb is red.
 */
export async function confirmQuestion(
  env: Env,
  user: UserEvent,
  kind: string,
  verb: string,
): Promise<void> {
  expect(env.store.getState().windows).toContain(kind);
  expect(questionOf(env.store, kind)).not.toBeNull();
  const close = vi.fn();
  const { container } = renderWithProviders(<QuestionWindow name={kind} close={close} />, env);
  const question = within(container);
  await waitFor(() => {
    expect(question.getByRole("button", { name: "Cancel" })).toHaveFocus();
  });
  const button = question.getByRole("button", { name: verb });
  expect(button).toHaveAttribute("data-variant", "danger");
  await user.click(button);
  await waitFor(() => {
    expect(close).toHaveBeenCalled();
  });
  expect(screen.queryByRole("alert", { name: verb })).toBeNull();
}
