/**
 * The menu button (R8, `useMenuButton`): a second press closes the menu,
 * Escape on the button closes a menu opened with the pointer, the open key
 * opens it with its first item focused, and a chosen item runs.
 */
import { fireEvent, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ResolvedMenuEntry } from "../../api/generated";
import { createEnv, renderWithProviders } from "../../test/harness";
import { useMenuButton } from "./MenuButton";

const ITEMS: ResolvedMenuEntry[] = [
  { type: "action", action: "sync.now", args: [], label: "Check now", icon: null, shortcut: null },
];

function Probe() {
  const { buttonProps, menu } = useMenuButton(() => ({ x: 10, y: 20 }), "ArrowDown");
  return (
    <>
      <button {...buttonProps} type="button">
        Open
      </button>
      {menu(ITEMS, "Probe menu")}
    </>
  );
}

describe("menu button (R8)", () => {
  it("toggles with the pointer, closes on Escape at the button and opens by key", async () => {
    const user = userEvent.setup();
    const env = createEnv();
    renderWithProviders(<Probe />, env);
    const button = screen.getByRole("button", { name: "Open" });
    await user.click(button);
    expect(screen.getByRole("menu", { name: "Probe menu" })).toBeInTheDocument();
    expect(button).toHaveAttribute("aria-expanded", "true");
    await user.click(button);
    expect(screen.queryByRole("menu")).toBeNull();
    await user.click(button);
    fireEvent.keyDown(button, { key: "Escape" });
    expect(screen.queryByRole("menu")).toBeNull();
    expect(button).toHaveFocus();
    fireEvent.keyDown(button, { key: "ArrowDown" });
    expect(await screen.findByRole("menuitem", { name: "Check now" })).toBeInTheDocument();
    await user.click(screen.getByRole("menuitem", { name: "Check now" }));
    expect(screen.queryByRole("menu")).toBeNull();
    expect(env.transport.requests).toContainEqual({ type: "SyncNow", account_id: null });
  });
});
