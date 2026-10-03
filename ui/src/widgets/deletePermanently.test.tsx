import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { violations } from "../test/axe";
import { createEnv, renderWithProviders } from "../test/harness";
import { WindowLayer } from "../layout/WindowLayer";
import {
  DELETE_PERMANENTLY_DIALOG,
  askDeletePermanently,
  pendingPermanentDelete,
} from "./DeletePermanently";

const INBOX = "acc-personal/Inbox";

function setup(ids: string[]) {
  const env = createEnv();
  askDeletePermanently(env, ids);
  const view = renderWithProviders(<WindowLayer layout={env.transport.data.layout} />, env);
  return { env, view };
}

describe("Delete permanently? (M5c)", () => {
  it("opens as a modal confirmation and deletes the messages it was opened for", async () => {
    const user = userEvent.setup();
    const { env } = setup([`${INBOX}#1`, `${INBOX}#2`]);
    const dialog = await screen.findByRole("dialog", { name: "Delete permanently?" });
    expect(dialog).toHaveAttribute("aria-modal", "true");
    expect(within(dialog).getByText("Delete 2 messages permanently?")).toBeInTheDocument();
    // The safe choice has the focus.
    await waitFor(() =>
      expect(within(dialog).getByRole("button", { name: "Cancel" })).toHaveFocus(),
    );
    // A selection change behind the window does not change what is deleted.
    env.store.getState().selectMessage(`${INBOX}#7`);
    await user.click(within(dialog).getByRole("button", { name: "Delete permanently" }));
    await waitFor(() =>
      expect(env.transport.requests).toContainEqual({
        type: "DeleteMessages",
        message_ids: [`${INBOX}#1`, `${INBOX}#2`],
        permanent: true,
      }),
    );
    expect(env.store.getState().windows).toEqual([]);
  });

  it("Cancel deletes nothing", async () => {
    const user = userEvent.setup();
    const { env } = setup([`${INBOX}#3`]);
    const dialog = await screen.findByRole("dialog", { name: "Delete permanently?" });
    expect(within(dialog).getByText("Delete this message permanently?")).toBeInTheDocument();
    await user.click(within(dialog).getByRole("button", { name: "Cancel" }));
    expect(env.store.getState().windows).toEqual([]);
    expect(pendingPermanentDelete(env.store)).toEqual([]);
    expect(env.transport.requests.some((r) => r.type === "DeleteMessages")).toBe(false);
  });

  it("asks nothing without a selection and passes axe", async () => {
    const env = createEnv();
    askDeletePermanently(env, []);
    expect(env.store.getState().windows).not.toContain(DELETE_PERMANENTLY_DIALOG);
    const { view } = setup([`${INBOX}#4`]);
    await screen.findByRole("dialog", { name: "Delete permanently?" });
    expect(await violations(view.container)).toEqual([]);
  });
});
