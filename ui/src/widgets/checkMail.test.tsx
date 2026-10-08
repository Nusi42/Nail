import { act, fireEvent, screen, waitFor } from "@testing-library/react";
import { createEnv, renderNode, widget } from "../test/harness";
import { RELOAD_KEYS } from "../actions/ActionsProvider";

function setOnline(online: boolean): void {
  Object.defineProperty(navigator, "onLine", { configurable: true, value: online });
  act(() => {
    window.dispatchEvent(new Event(online ? "online" : "offline"));
  });
}

afterEach(() => {
  Reflect.deleteProperty(navigator, "onLine");
});

describe("Check for new mail (ADR 0183)", () => {
  it("sits beside New message, checks every account and turns while accounts sync", async () => {
    const env = createEnv({ state: { activeAccountId: "acc-work" } });
    renderNode(widget("folder-tree", { options: { sections: "favorites accounts" } }), env);
    const button = await screen.findByRole("button", { name: "Check for new mail" });
    expect(button).toHaveAttribute("title", "Check for new mail");
    expect(button.closest('[data-part="folder-tree.header"]')).toContainElement(
      screen.getByRole("button", { name: "New message" }),
    );
    expect(button).not.toHaveAttribute("data-state");
    fireEvent.click(button);
    await waitFor(() => {
      expect(env.transport.requests).toContainEqual({ type: "SyncNow", account_id: null });
    });
    act(() => {
      env.store.getState().setSync({ accountId: "acc-work", done: 0, total: 3 });
    });
    expect(button).toHaveAttribute("data-state", "busy");
    expect(button).toHaveAttribute("aria-busy", "true");
    act(() => {
      env.store.getState().setSync({ accountId: "acc-work", done: 3, total: 3 });
    });
    expect(button).not.toHaveAttribute("data-state");

    setOnline(false);
    expect(button).toBeDisabled();
    setOnline(true);
    expect(button).toBeEnabled();
  });

  it("can be left out (check-mail=#false)", async () => {
    const env = createEnv();
    renderNode(
      widget("folder-tree", { options: { sections: "favorites", "check-mail": false } }),
      env,
    );
    await screen.findByRole("button", { name: "New message" });
    expect(screen.queryByRole("button", { name: "Check for new mail" })).toBeNull();
  });

  it("never lets F5 / Ctrl+R reload the page: they check for new mail, even in a field", async () => {
    const env = createEnv();
    renderNode(widget("folder-tree", { options: { sections: "favorites" } }), env);
    const input = document.createElement("input");
    input.setAttribute("aria-label", "Search");
    document.body.append(input);
    input.focus();
    try {
      for (const [key, ctrlKey] of [
        ["F5", false],
        ["r", true],
      ] as const) {
        const event = new KeyboardEvent("keydown", {
          key,
          ctrlKey,
          bubbles: true,
          cancelable: true,
        });
        act(() => {
          input.dispatchEvent(event);
        });
        expect(event.defaultPrevented).toBe(true);
      }
      await waitFor(() => {
        expect(env.transport.requests.filter((r) => r.type === "SyncNow")).toHaveLength(2);
      });
      // Shift+F5 / Ctrl+F5 / Ctrl+Shift+R have no binding: nothing, and no reload.
      const event = new KeyboardEvent("keydown", {
        key: "F5",
        shiftKey: true,
        bubbles: true,
        cancelable: true,
      });
      act(() => {
        input.dispatchEvent(event);
      });
      expect(event.defaultPrevented).toBe(true);
      expect([...RELOAD_KEYS]).toEqual(["f5", "shift+f5", "ctrl+f5", "ctrl+r", "ctrl+shift+r"]);
    } finally {
      input.remove();
    }
  });
});
