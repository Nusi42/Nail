import { act, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { Request } from "../../api/generated";
import type { ActionContext } from "../../actions/registry";
import { createEnv, renderNode, renderWithProviders, widget, type Env } from "../../test/harness";
import { AUTOCRYPT_IMPORT_DIALOG, AutocryptImportDialog } from "./Autocrypt";
import { actions, dialogs } from "./index";

function requestsOf<T extends Request["type"]>(env: Env, type: T) {
  return env.transport.requests.filter((r): r is Extract<Request, { type: T }> => r.type === type);
}

const CODE = "1234-5678-9012-3456-7890-1234-5678-9012-3456";

function firstMessage(env: Env): string {
  const id = [...env.transport.data.bodies.keys()][0];
  if (!id) throw new Error("no message");
  return id;
}

describe("security-autocrypt", () => {
  it("sends a setup message and shows its code once", async () => {
    const user = userEvent.setup();
    const env = createEnv();
    renderNode(widget("security-autocrypt"), env);
    const send = screen.getByRole("region", { name: "Use your key on another device" });
    const identity = within(send).getByRole("combobox", { name: "Key of" });
    await waitFor(() => expect(within(identity).getAllByRole("option").length).toBe(3));
    await user.click(within(send).getByRole("button", { name: "Send setup message" }));
    const code = await within(send).findByRole("region", { name: "Setup code" });
    expect(within(code).getByRole("alert")).toHaveTextContent("cannot show it again");
    const value = within(code).getByText(/^(\d{4}-){8}\d{4}$/);
    expect(requestsOf(env, "SendAutocryptSetup")).toEqual([
      { type: "SendAutocryptSetup", identity_id: "ada@example.org" },
    ]);
    await user.click(within(code).getByRole("button", { name: "Done" }));
    expect(screen.queryByText(value.textContent ?? "")).not.toBeInTheDocument();
  });

  it("explains a missing OpenPGP key", async () => {
    const user = userEvent.setup();
    const env = createEnv();
    renderNode(widget("security-autocrypt"), env);
    const send = screen.getByRole("region", { name: "Use your key on another device" });
    const identity = within(send).getByRole("combobox", { name: "Key of" });
    await waitFor(() => expect(within(identity).getAllByRole("option").length).toBe(3));
    await user.selectOptions(identity, "ada@work.example");
    await user.click(within(send).getByRole("button", { name: "Send setup message" }));
    expect(await within(send).findByRole("alert")).toHaveTextContent(
      "This address has no OpenPGP key yet.",
    );
  });

  it("imports the selected setup message with its code", async () => {
    const user = userEvent.setup();
    const env = createEnv();
    const id = firstMessage(env);
    env.transport.data.features.security.setupInbox[id] = CODE;
    renderNode(widget("security-autocrypt"), env);
    const receive = screen.getByRole("region", { name: "Import a key from another device" });
    expect(within(receive).getByText(/Select the setup message/)).toBeInTheDocument();
    act(() => env.store.getState().selectMessage(id));
    const form = await within(receive).findByRole("form", {
      name: "Import a key from another device",
    });
    expect(within(receive).getByText(/^Setup message: /)).toBeInTheDocument();
    const input = within(form).getByRole("textbox", { name: "Setup code" });
    const submit = within(form).getByRole("button", { name: "Import key" });
    await user.type(input, "1234 5678");
    expect(input).toHaveValue("1234-5678");
    expect(submit).toBeDisabled();
    await user.type(input, "0".repeat(28));
    await user.click(submit);
    expect(await within(form).findByRole("alert")).toHaveTextContent("The setup code is wrong.");
    await user.clear(input);
    await user.type(input, CODE.replaceAll("-", ""));
    await user.click(submit);
    expect(await within(receive).findByRole("status")).toHaveTextContent("Imported: 1 new");
    expect(requestsOf(env, "ImportAutocryptSetup").at(-1)).toEqual({
      type: "ImportAutocryptSetup",
      message_id: id,
      passcode: CODE,
    });
  });

  it("says when the message is not a setup message", async () => {
    const user = userEvent.setup();
    const env = createEnv();
    act(() => env.store.getState().selectMessage(firstMessage(env)));
    renderNode(widget("security-autocrypt"), env);
    const form = await screen.findByRole("form", { name: "Import a key from another device" });
    await user.type(within(form).getByRole("textbox", { name: "Setup code" }), "1".repeat(36));
    await user.click(within(form).getByRole("button", { name: "Import key" }));
    expect(await within(form).findByRole("alert")).toHaveTextContent(
      "This message is not an Autocrypt setup message.",
    );
  });
});

describe("autocrypt import dialog and action", () => {
  function ctx(env: Env): ActionContext {
    return {
      client: env.client,
      store: env.store,
      queryClient: env.queryClient,
      layout: env.transport.data.layout,
      t: (id) => id,
      locale: "en",
    };
  }

  it("opens the dialog only with a selected message", () => {
    const env = createEnv();
    const action = actions["security.autocrypt.import"];
    expect(action?.enabled?.(ctx(env), [])).toBe(false);
    act(() => env.store.getState().selectMessage(firstMessage(env)));
    expect(action?.enabled?.(ctx(env), [])).toBe(true);
    void action?.run(ctx(env), []);
    expect(env.store.getState().windows).toContain(AUTOCRYPT_IMPORT_DIALOG);
    expect(dialogs[AUTOCRYPT_IMPORT_DIALOG]).toBe(AutocryptImportDialog);
  });

  it("imports in the dialog and closes", async () => {
    const user = userEvent.setup();
    const env = createEnv();
    const id = firstMessage(env);
    env.transport.data.features.security.setupInbox[id] = CODE;
    const close = vi.fn();
    renderWithProviders(
      <AutocryptImportDialog name={AUTOCRYPT_IMPORT_DIALOG} close={close} />,
      env,
    );
    expect(screen.getByText(/Select the setup message/)).toBeInTheDocument();
    act(() => env.store.getState().selectMessage(id));
    await user.type(await screen.findByRole("textbox", { name: "Setup code" }), CODE);
    await user.click(screen.getByRole("button", { name: "Import key" }));
    expect(await screen.findByRole("status")).toHaveTextContent("1 new");
    await user.click(screen.getByRole("button", { name: "Done" }));
    expect(close).toHaveBeenCalled();
  });
});
