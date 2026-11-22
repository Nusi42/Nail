/**
 * R6 (ADR 0239): the Exchange dialogs in the shared pattern, in the app's
 * window layer with its close guard — an editor's ×, Esc and Cancel ask
 * with the shared prompt once something changed; a delete question
 * focuses Cancel and its red verb is never the default.
 */
import { act, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { WindowLayer } from "../../layout/WindowLayer";
import { withCloseGuard } from "../../test/closeGuard";
import { renderWithProviders, type Env } from "../../test/harness";
import { must } from "../../test/must";
import { EXCHANGE_ACCOUNT } from "./mockSeed";
import { CATEGORY_DELETE_DIALOG, CATEGORY_EDITOR_DIALOG } from "./names";
import { exchangeStoreFor } from "./store";
import { setup } from "./testing";

function windowOf(name: string): HTMLElement {
  return must(document.querySelector<HTMLElement>(`[data-part="window"][data-window="${name}"]`));
}

/** The app's windows on the Exchange demo, the close guard installed. */
function withWindows(): Env {
  const env = setup();
  withCloseGuard(env);
  renderWithProviders(<WindowLayer layout={env.transport.data.layout} />, env);
  return env;
}

describe("Exchange dialogs (R6)", () => {
  it("asks before a new category's typed name is lost; Don't save drops it", async () => {
    const user = userEvent.setup();
    const env = withWindows();
    act(() => {
      exchangeStoreFor(env.store)
        .getState()
        .editCategory({ accountId: EXCHANGE_ACCOUNT, categoryId: null });
      env.store.getState().openWindow(CATEGORY_EDITOR_DIALOG);
    });
    const editor = windowOf(CATEGORY_EDITOR_DIALOG);
    const name = await within(editor).findByRole("textbox", { name: "Name" });
    await waitFor(() => {
      expect(name).toHaveFocus();
    });
    const roles = [...editor.querySelectorAll('[data-part="dialog.footer.end"] > button')].map(
      (b) => b.getAttribute("data-role"),
    );
    expect(roles).toEqual(["primary", "cancel"]);
    await user.type(name, "Board");
    await user.click(within(editor).getByRole("button", { name: "Cancel" }));
    const prompt = await waitFor(() => windowOf(`unsaved-changes:${CATEGORY_EDITOR_DIALOG}`));
    expect(within(prompt).getByText("Save changes to “Board”?")).toBeInTheDocument();
    await user.click(within(prompt).getByRole("button", { name: "Don't save" }));
    expect(env.store.getState().windows).toEqual([]);
    expect(env.transport.requests.some((r) => r.type === "CreateExchangeCategory")).toBe(false);
  });

  it("deletes a category only from its red verb; the focus starts on Cancel", async () => {
    const user = userEvent.setup();
    const env = withWindows();
    const { categories } = await env.client.request(
      { type: "ListExchangeCategories", account_id: EXCHANGE_ACCOUNT },
      "ExchangeCategories",
    );
    act(() => {
      exchangeStoreFor(env.store).getState().deleteCategory(must(categories[0]));
      env.store.getState().openWindow(CATEGORY_DELETE_DIALOG);
    });
    const question = windowOf(CATEGORY_DELETE_DIALOG);
    const cancel = within(question).getByRole("button", { name: "Cancel" });
    await waitFor(() => {
      expect(cancel).toHaveFocus();
    });
    const verb = must(question.querySelector<HTMLButtonElement>('[data-role="primary"]'));
    expect(verb).toHaveAttribute("data-variant", "danger");
    expect(verb).toHaveAttribute("type", "button");
    await user.click(verb);
    await waitFor(() => {
      expect(screen.queryByRole("dialog")).toBeNull();
    });
  });
});
