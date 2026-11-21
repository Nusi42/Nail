/**
 * The automatic replies dialog's account field (R3, ADR 0228): with several
 * Exchange accounts it is a strict account picker that offers only them.
 */
import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { renderWithProviders } from "../../test/harness";
import { pickOption } from "../../widgets/pickers/testing";
import { EXCHANGE_ACCOUNT, EXCHANGE_EMAIL } from "./mockSeed";
import { AUTOMATIC_REPLIES_DIALOG } from "./names";
import { dialog, setup } from "./testing";

describe("automatic replies: the account", () => {
  it("picks one of the Exchange accounts", async () => {
    const env = setup();
    const exchange = env.transport.data.accounts.find((a) => a.id === EXCHANGE_ACCOUNT);
    if (!exchange) throw new Error("no Exchange account");
    env.transport.data.accounts.push({
      ...exchange,
      id: "acc-exchange-2",
      name: "Fabrikam",
      email: EXCHANGE_EMAIL.replace("contoso", "fabrikam"),
    });
    const user = userEvent.setup();
    const Replies = dialog(AUTOMATIC_REPLIES_DIALOG);
    const { container } = renderWithProviders(
      <Replies name={AUTOMATIC_REPLIES_DIALOG} close={vi.fn()} />,
      env,
    );
    const account = await screen.findByRole("combobox", { name: "Account" });
    await waitFor(() => {
      expect(account).toHaveValue(exchange.name);
    });
    await user.click(screen.getByRole("button", { name: "Show choices for Account" }));
    const list = await screen.findByRole("listbox", { name: "Choices for Account" });
    // Only Exchange accounts.
    expect(within(list).getAllByRole("option")).toHaveLength(2);
    await user.keyboard("{Escape}");
    await pickOption(user, container, "Account", /^Fabrikam/);
    expect(account).toHaveValue("Fabrikam");
  });
});
