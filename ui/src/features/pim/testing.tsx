/**
 * Test helpers of the PIM feature (only tests import this file).
 */
import { screen, waitFor, within } from "@testing-library/react";
import type { UserEvent } from "@testing-library/user-event";
import { renderWithProviders, type Env } from "../../test/harness";
import { pimConfirmDialogs, pimQuestion, type PimConfirm } from "./Confirm";

/**
 * Answers the open PIM question `kind` (R6: a destructive confirmation
 * window) with its verb `verb`, after checking that Cancel has the focus.
 */
export async function confirmPim(
  env: Env,
  user: UserEvent,
  kind: PimConfirm,
  verb: string,
): Promise<void> {
  expect(env.store.getState().windows).toContain(kind);
  expect(pimQuestion(env.store, kind)).not.toBeNull();
  const Dialog = pimConfirmDialogs[kind];
  const close = vi.fn();
  const { container } = renderWithProviders(<Dialog name={kind} close={close} />, env);
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
