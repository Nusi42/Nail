/**
 * Right-click menus of PIM items (R8, ADR 0251) against the mock core:
 * events in the calendar views, contacts in the contact list and tasks in
 * the task list. A right-click opens the item's menu at the pointer (the
 * app's layout menu stays away), Shift+F10 opens it from the keyboard on
 * its first item, Up / Down / Escape work like every PIM menu and Escape
 * gives the focus back to the item; the entries run the item's commands.
 */
import { act, fireEvent, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { createEnv, renderNode, settleQueries, widget, type Env } from "../../test/harness";
import { must } from "../../test/must";
import { CONTACT_DIALOG, DELETE_CONTACTS_DIALOG, EVENT_DIALOG, TASK_DIALOG } from "./names";
import { pimStoreFor } from "./store";

const pimOf = (env: Env) => env.transport.data.features.pim;

/** Right-clicks `element` and answers the menu that opened. */
async function rightClick(element: HTMLElement, name?: string): Promise<HTMLElement> {
  const event = fireEvent.contextMenu(element, { clientX: 40, clientY: 60, button: 2 });
  // Handled: the app's one contextmenu handler leaves it alone (ADR 0184).
  expect(event).toBe(false);
  return screen.findByRole("menu", name === undefined ? {} : { name });
}

/** The open menu's item names. */
function menuItems(menu: HTMLElement): string[] {
  return within(menu)
    .getAllByRole("menuitem")
    .map((item) => item.textContent);
}

/** Puts the focus on `element` (its focus handler updates the store). */
const focus = (element: HTMLElement) => {
  act(() => {
    element.focus();
  });
};

/** The day view with the demo's lunch. */
async function dayWithLunch(env: Env): Promise<HTMLElement> {
  renderNode(widget("calendar", { options: { view: "day" } }), env);
  const lunch = await screen.findByRole("button", { name: /Lunch with Grace/ });
  await settleQueries(env);
  return lunch;
}

describe("right-click menus of PIM items", () => {
  it("events: Open, Edit and Delete at the pointer; Edit opens the editor", async () => {
    const user = userEvent.setup({ delay: null });
    const env = createEnv();
    const lunch = await dayWithLunch(env);
    const menu = await rightClick(lunch, "Actions for Lunch with Grace");
    expect(menuItems(menu)).toEqual(["Open", "Edit", "Delete"]);
    expect(within(menu).getByRole("menuitem", { name: "Open" })).toHaveFocus();
    await user.keyboard("{ArrowDown}");
    expect(within(menu).getByRole("menuitem", { name: "Edit" })).toHaveFocus();
    await user.keyboard("{Enter}");
    expect(env.store.getState().windows).toContain(EVENT_DIALOG);
    expect(pimStoreFor(env.store).getState().eventEditor).toMatchObject({
      mode: "edit",
      eventId: "ev-lunch",
    });
    expect(screen.queryByRole("menu")).toBeNull();
  });

  it("events: Shift+F10 opens the menu on its first item, Escape returns to the card, Delete deletes", async () => {
    const user = userEvent.setup({ delay: null });
    const env = createEnv();
    const lunch = await dayWithLunch(env);
    focus(lunch);
    await user.keyboard("{Shift>}{F10}{/Shift}");
    const menu = await screen.findByRole("menu", { name: "Actions for Lunch with Grace" });
    expect(within(menu).getByRole("menuitem", { name: "Open" })).toHaveFocus();
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("menu")).toBeNull();
    expect(lunch).toHaveFocus();
    await user.click(within(await rightClick(lunch)).getByRole("menuitem", { name: "Delete" }));
    await waitFor(() => {
      expect(pimOf(env).events.some((e) => e.id === "ev-lunch")).toBe(false);
    });
  });

  it("events: the agenda's entries have the same menu", async () => {
    const env = createEnv();
    renderNode(widget("calendar", { options: { view: "agenda" } }), env);
    const lunch = (await screen.findAllByRole("button", { name: /^Lunch with Grace/ }))[0];
    const menu = await rightClick(must(lunch), "Actions for Lunch with Grace");
    expect(menuItems(menu)).toEqual(["Open", "Edit", "Delete"]);
  });

  it("events: Open shows the quick look; a read-only event's menu has no Delete", async () => {
    const user = userEvent.setup({ delay: null });
    const env = createEnv();
    must(pimOf(env).events.find((e) => e.id === "ev-lunch")).read_only = true;
    const lunch = await dayWithLunch(env);
    const menu = await rightClick(lunch);
    expect(menuItems(menu)).toEqual(["Open", "Edit"]);
    await user.click(within(menu).getByRole("menuitem", { name: "Open" }));
    expect(pimStoreFor(env.store).getState().quickLook?.instance.event_id).toBe("ev-lunch");
  });

  it("contacts: Open shows the card, Edit opens the editor, Delete asks first", async () => {
    const user = userEvent.setup({ delay: null });
    const env = createEnv();
    renderNode(widget("contact-list"), env);
    const ada = await screen.findByRole("option", { name: /Ada Lovelace/ });
    const id = must(ada.dataset.contact);
    const menu = await rightClick(ada);
    expect(menu).toHaveAccessibleName(/^Actions for Ada Lovelace/);
    expect(menuItems(menu)).toEqual(["Open", "Edit", "Delete…"]);
    await user.click(within(menu).getByRole("menuitem", { name: "Open" }));
    expect(pimStoreFor(env.store).getState().contactId).toBe(id);
    await user.click(within(await rightClick(ada)).getByRole("menuitem", { name: "Edit" }));
    expect(env.store.getState().windows).toContain(CONTACT_DIALOG);
    await user.click(within(await rightClick(ada)).getByRole("menuitem", { name: "Delete…" }));
    // The shared delete question opens; nothing is deleted yet.
    expect(env.store.getState().windows).toContain(DELETE_CONTACTS_DIALOG);
    expect(pimOf(env).contacts.some((c) => c.id === id)).toBe(true);
  });

  it("tasks: Edit, Mark as done and Delete; Shift+F10 on the focused task", async () => {
    const user = userEvent.setup({ delay: null });
    const env = createEnv();
    renderNode(widget("task-list"), env);
    const groceries = await screen.findByRole("treeitem", { name: /Buy groceries/ });
    focus(groceries);
    await user.keyboard("{Shift>}{F10}{/Shift}");
    const menu = await screen.findByRole("menu", { name: "Actions for Buy groceries" });
    expect(menuItems(menu)).toEqual(["Edit", "Mark “Buy groceries” as done", "Delete"]);
    await user.click(within(menu).getByRole("menuitem", { name: "Edit" }));
    expect(env.store.getState().windows).toContain(TASK_DIALOG);
    await user.click(
      within(await rightClick(groceries)).getByRole("menuitem", { name: /as done/ }),
    );
    await waitFor(() => {
      expect(must(pimOf(env).tasks.find((t) => t.id === "tk-groceries")).fields.status).toBe(
        "completed",
      );
    });
  });
});
