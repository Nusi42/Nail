/**
 * The invitation answer's confirmation and the Propose / Forward forms use
 * the shared footer (R6 review): their buttons follow the viewer's system.
 */
import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ReactNode } from "react";
import { createEnv, renderWithProviders } from "../../test/harness";
import { must } from "../../test/must";
import { DialogOsContext } from "../../widgets/dialog";
import { AnswerForm, ForwardForm } from "./Answer";

function renderOn(os: "windows" | "macos", ui: ReactNode) {
  return renderWithProviders(
    <DialogOsContext.Provider value={os}>{ui}</DialogOsContext.Provider>,
    createEnv(),
  );
}

/** The footer's button labels, left to right. */
const footer = (root: HTMLElement) =>
  [...must(root.querySelector('[data-part="dialog.footer"]')).querySelectorAll("button")].map(
    (b) => b.textContent,
  );

describe("answering an invitation (R6 review)", () => {
  it("asks in the shared confirmation: Send, Only update my calendar, Back — mirrored on macOS", async () => {
    const user = userEvent.setup();
    const onAnswer = vi.fn();
    const form = (
      <AnswerForm
        current={null}
        available={["accept", "tentative", "decline"]}
        organizer="Ada"
        confirm
        busy={false}
        onAnswer={onAnswer}
      />
    );
    const windows = renderOn("windows", form);
    await user.click(within(windows.container).getByRole("button", { name: /Accept/ }));
    const question = must(
      windows.container.querySelector<HTMLElement>('[data-part="dialog.confirm"]'),
    );
    expect(question).toHaveAttribute("data-confirm", "send-answer");
    expect(footer(windows.container)).toEqual(["Send", "Only update my calendar", "Back"]);
    // A normal confirmation: Send has the focus.
    await waitFor(() => {
      expect(within(question).getByRole("button", { name: "Send" })).toHaveFocus();
    });
    await user.click(within(question).getByRole("button", { name: "Only update my calendar" }));
    expect(onAnswer).toHaveBeenCalledWith("accept", null, false);
    windows.unmount();

    const mac = renderOn("macos", form);
    await user.click(within(mac.container).getByRole("button", { name: /Accept/ }));
    expect(footer(mac.container)).toEqual(["Back", "Only update my calendar", "Send"]);
    await user.click(screen.getByRole("button", { name: "Back" }));
    expect(mac.container.querySelector('[data-part="dialog.confirm"]')).toBeNull();
  });

  it("orders the Forward form's footer by role", () => {
    const props = { busy: false, problem: null, onSubmit: vi.fn(), onCancel: vi.fn() };
    expect(footer(renderOn("windows", <ForwardForm {...props} />).container)).toEqual([
      "Forward",
      "Cancel",
    ]);
    expect(footer(renderOn("macos", <ForwardForm {...props} />).container)).toEqual([
      "Cancel",
      "Forward",
    ]);
  });
});
