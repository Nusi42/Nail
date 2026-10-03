/**
 * M5b review: the Contacts and Tasks views get a header like the calendar
 * (`header=#true`): the title, "New …" as the one primary button and, for
 * contacts, the search and a "More" menu; actions that do not apply are
 * hidden instead of shown disabled.
 */
import { act, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { createEnv, renderNode, widget, type Env } from "../../test/harness";
import { CONTACT_DIALOG, GROUPS_DIALOG, TASK_DIALOG } from "./names";
import { pimStoreFor } from "./store";

/** The first contact row's id. */
function firstContact(): string {
  const id = document
    .querySelector('[data-part="contact-list.item"]')
    ?.getAttribute("data-contact");
  if (!id) throw new Error("no contact yet");
  return id;
}

async function settle(env: Env): Promise<void> {
  await waitFor(() => expect(env.queryClient.isFetching()).toBe(0));
}

describe("Contacts view header", () => {
  it("shows the title, the search, New contact as primary and the rest in More", async () => {
    const user = userEvent.setup();
    const env = createEnv();
    renderNode(widget("contact-list", { options: { header: true } }), env);
    await settle(env);
    const header = document.querySelector<HTMLElement>('[data-part="pim.view-header"]');
    expect(header).not.toBeNull();
    const view = within(header as HTMLElement);
    expect(view.getByRole("heading", { name: "Contacts" })).toHaveAttribute(
      "data-part",
      "pim.view-header.title",
    );
    expect(view.getByRole("searchbox", { name: "Search contacts" })).toBeInTheDocument();
    const create = view.getByRole("button", { name: "New contact" });
    expect(create).toHaveAttribute("data-variant", "primary");
    // No row of buttons, no disabled Delete.
    expect(screen.queryByRole("group", { name: "Contact actions" })).toBeNull();
    expect(screen.queryByRole("button", { name: /^Delete/ })).toBeNull();
    // More: Groups and Find duplicates; Delete only with a contact chosen.
    await user.click(view.getByRole("button", { name: "More" }));
    const menu = await screen.findByRole("menu");
    expect(
      within(menu)
        .getAllByRole("menuitem")
        .map((i) => i.textContent),
    ).toEqual(["Groups…", "Find duplicates…"]);
    await user.click(within(menu).getByRole("menuitem", { name: "Groups…" }));
    expect(env.store.getState().windows).toContain(GROUPS_DIALOG);
    await user.keyboard("{Escape}");
    const first = await waitFor(() => firstContact());
    act(() => pimStoreFor(env.store).getState().showContact(first));
    await user.click(view.getByRole("button", { name: "More" }));
    const again = await screen.findByRole("menu");
    expect(within(again).getByRole("menuitem", { name: /^Delete/ })).toBeInTheDocument();
    await user.keyboard("{Escape}");
    await user.click(create);
    expect(env.store.getState().windows).toContain(CONTACT_DIALOG);
  });

  it("hides Delete without a contact in the plain widget too", async () => {
    const env = createEnv();
    renderNode(widget("contact-list"), env);
    await settle(env);
    expect(screen.getByRole("group", { name: "Contact actions" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /^Delete/ })).toBeNull();
    const first = await waitFor(() => firstContact());
    act(() => pimStoreFor(env.store).getState().showContact(first));
    expect(await screen.findByRole("button", { name: /^Delete/ })).toBeInTheDocument();
  });
});

describe("Tasks view header", () => {
  it("shows the title and New task as the primary button", async () => {
    const user = userEvent.setup();
    const env = createEnv();
    renderNode(widget("task-list", { options: { header: true } }), env);
    await settle(env);
    const header = document.querySelector<HTMLElement>('[data-part="pim.view-header"]');
    const view = within(header as HTMLElement);
    expect(view.getByRole("heading", { name: "Tasks" })).toBeInTheDocument();
    const create = view.getByRole("button", { name: "New task" });
    expect(create).toHaveAttribute("data-variant", "primary");
    // Only once: not again in the filter row.
    expect(screen.getAllByRole("button", { name: "New task" })).toHaveLength(1);
    await user.click(create);
    expect(env.store.getState().windows).toContain(TASK_DIALOG);
  });
});
