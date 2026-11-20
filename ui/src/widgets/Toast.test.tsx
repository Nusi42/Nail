import { act, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ReactElement } from "react";
import { Providers, createEnv, renderWithProviders, type Env } from "../test/harness";
import { TOAST_MS, Toast } from "./Toast";

/** Rerenders `ui` with the providers of `env`. */
function provided(env: Env, ui: ReactElement): ReactElement {
  return <Providers env={env}>{ui}</Providers>;
}

describe("the shared toast", () => {
  it("says what was done, offers Undo and hides after its time", () => {
    vi.useFakeTimers();
    try {
      const dismiss = vi.fn();
      const undo = vi.fn();
      renderWithProviders(
        <Toast
          kind="done"
          seq={1}
          text="Done it"
          source="test"
          undo={{ label: "Undo", run: undo }}
          onDismiss={dismiss}
        />,
        createEnv(),
      );
      const toast = screen.getByRole("status", { hidden: true });
      expect(toast).toHaveTextContent("Done it");
      expect(toast).toHaveAttribute("data-part", "mail-actions.toast");
      expect(toast).toHaveAttribute("data-source", "test");
      act(() => {
        screen.getByRole("button", { name: "Undo", hidden: true }).click();
      });
      expect(undo).toHaveBeenCalledTimes(1);
      // Dismiss is the icon button every toast has.
      expect(screen.getByRole("button", { name: "Dismiss", hidden: true })).toHaveAttribute(
        "data-part",
        "mail-actions.toast.dismiss",
      );
      act(() => {
        vi.advanceTimersByTime(TOAST_MS.done - 1);
      });
      expect(dismiss).not.toHaveBeenCalled();
      act(() => {
        vi.advanceTimersByTime(1);
      });
      expect(dismiss).toHaveBeenCalledTimes(1);
    } finally {
      vi.useRealTimers();
    }
  });

  it("shows one toast at a time: a newer one dismisses the one shown", () => {
    const env = createEnv();
    const first = vi.fn();
    const second = vi.fn();
    const view = renderWithProviders(
      <Toast kind="done" seq={1} text="Mail moved" onDismiss={first} />,
      env,
    );
    expect(first).not.toHaveBeenCalled();
    view.rerender(
      provided(
        env,
        <>
          <Toast kind="done" seq={1} text="Mail moved" onDismiss={first} />
          <Toast kind="error" seq={1} text="Rule failed" onDismiss={second} />
        </>,
      ),
    );
    expect(first).toHaveBeenCalledTimes(1);
    expect(second).not.toHaveBeenCalled();
  });

  it("keeps a toast of its own feature when that feature shows the next one", async () => {
    const env = createEnv();
    const dismiss = vi.fn();
    const view = renderWithProviders(
      <Toast kind="done" seq={1} text="One" onDismiss={dismiss} />,
      env,
    );
    view.rerender(provided(env, <Toast kind="undone" seq={2} text="Two" onDismiss={dismiss} />));
    await userEvent.setup().hover(screen.getByRole("status", { hidden: true }));
    expect(dismiss).not.toHaveBeenCalled();
    expect(screen.getByRole("status", { hidden: true })).toHaveTextContent("Two");
  });
});
