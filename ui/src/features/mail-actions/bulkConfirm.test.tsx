/**
 * The confirmation of an action on a whole list (R6 review): the shared
 * `ConfirmDialog` — a normal action's verb has the focus and Enter runs it,
 * deleting is red with the focus on Cancel; the footer follows the
 * viewer's system.
 */
import { act, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { MailAction, MailTargetResolution } from "../../api/generated";
import { createEnv, renderWithProviders } from "../../test/harness";
import { DialogOsContext } from "../../widgets/dialog";
import { confirmBulk } from "./bulk";
import { BulkConfirmDialog } from "./BulkConfirm";

const RESOLUTION: MailTargetResolution = {
  message_ids: [],
  messages: 240,
  account_ids: ["acc-work"],
  conversations: 0,
  as_of: "2026-11-22T10:00:00Z",
};

function ask(action: MailAction, os: "windows" | "macos" = "windows") {
  const env = createEnv();
  let answer: Promise<boolean> = Promise.resolve(false);
  act(() => {
    answer = confirmBulk(env.store, action, RESOLUTION);
  });
  const view = renderWithProviders(
    <DialogOsContext.Provider value={os}>
      <BulkConfirmDialog />
    </DialogOsContext.Provider>,
    env,
  );
  return { env, answer, question: within(view.container) };
}

describe("the whole-list confirmation (R6 review)", () => {
  it("focuses the verb of a normal action, and Enter confirms", async () => {
    const user = userEvent.setup();
    const { answer, question } = ask({ type: "archive" });
    const verb = question.getByRole("button", { name: "Archive all" });
    await waitFor(() => {
      expect(verb).toHaveFocus();
    });
    expect(verb).toHaveAttribute("data-variant", "primary");
    await user.keyboard("{Enter}");
    await expect(answer).resolves.toBe(true);
  });

  it("makes deleting red with the focus on Cancel", async () => {
    const user = userEvent.setup();
    const { answer, question } = ask({ type: "delete-permanently" });
    await waitFor(() => {
      expect(question.getByRole("button", { name: "Cancel" })).toHaveFocus();
    });
    expect(screen.getByText(/cannot be undone/i)).toBeInTheDocument();
    const buttons = question.getAllByRole("button");
    expect(buttons.find((b) => b.getAttribute("data-role") === "primary")).toHaveAttribute(
      "data-variant",
      "danger",
    );
    await user.click(question.getByRole("button", { name: "Cancel" }));
    await expect(answer).resolves.toBe(false);
  });

  it("orders the footer for the viewer's system", () => {
    const roles = (os: "windows" | "macos") =>
      ask({ type: "archive" }, os)
        .question.getAllByRole("button")
        .map((b) => b.getAttribute("data-role"));
    expect(roles("windows")).toEqual(["primary", "cancel"]);
    expect(roles("macos")).toEqual(["cancel", "primary"]);
  });
});
