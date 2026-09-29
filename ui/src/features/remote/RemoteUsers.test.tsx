import { act, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { Request } from "../../api/generated";
import { createEnv, renderNode, renderWithProviders, widget, type Env } from "../../test/harness";
import { handleRemoteEvent } from "./index";
import { defaultRemoteUiSettings } from "./mock";
import { MockAuthServer } from "./mockAuth";
import { groupFingerprint, presetGrant } from "./model";
import { PAIR_DIALOG, PairDeviceDialog } from "./PairDevice";
import { remoteStore } from "./state";
import { createAda } from "./testUtil";

function wireEvents(env: Env) {
  return env.client.subscribe((event) =>
    handleRemoteEvent(event, {
      client: env.client,
      queryClient: env.queryClient,
      store: env.store,
    }),
  );
}

function requestsOf<T extends Request["type"]>(env: Env, type: T) {
  return env.transport.requests.filter((r): r is Extract<Request, { type: T }> => r.type === type);
}

afterEach(() => remoteStore.getState().clear());

describe("remote-users", () => {
  it("adds a user with a preset using only the keyboard", async () => {
    const user = userEvent.setup();
    const env = createEnv();
    wireEvents(env);
    renderNode(widget("remote-users"), env);
    expect(await screen.findByText("No remote users yet.")).toBeInTheDocument();
    const form = screen.getByRole("form", { name: "Add a user" });
    within(form).getByRole("textbox", { name: "Login name" }).focus();
    await user.keyboard("grace");
    await user.tab();
    await user.keyboard("Grace Hopper");
    await user.tab();
    expect(within(form).getByRole("combobox", { name: "Access" })).toHaveFocus();
    await user.selectOptions(within(form).getByRole("combobox", { name: "Access" }), "read-only");
    await user.tab();
    await user.keyboard("{Enter}");

    const detail = await screen.findByRole("region", { name: "User grace" });
    expect(within(detail).getByText("Grace Hopper")).toBeInTheDocument();
    expect(requestsOf(env, "CreateRemoteUser")[0]).toEqual({
      type: "CreateRemoteUser",
      name: "grace",
      display_name: "Grace Hopper",
      grant: presetGrant("read-only"),
    });
    expect(within(detail).getByText(/No password yet/)).toBeInTheDocument();

    // A second user with the same name is refused.
    await user.type(within(form).getByRole("textbox", { name: "Login name" }), "GRACE");
    await user.click(within(form).getByRole("button", { name: "Add user" }));
    expect(await within(form).findByRole("alert")).toHaveTextContent(
      "A user with this name already exists.",
    );
  });

  it("sets a password, enrolls TOTP with a QR code and manages passkeys", async () => {
    const user = userEvent.setup();
    const env = createEnv();
    wireEvents(env);
    const ada = await createAda(env.transport);
    await env.transport.send({ type: "SetRemotePassword", user_id: ada.id, password: null });
    env.transport.data.features.remote.users[0]?.passkeys.push({
      id: "passkey-1",
      name: "Phone",
      created_at: "2026-09-29T10:00:00Z",
      last_used_at: null,
      device_id: null,
    });
    renderNode(widget("remote-users"), env);
    const detail = await screen.findByRole("region", { name: "User ada" });

    // Password: too short, mismatch, then saved.
    const pw = within(detail).getByRole("form", { name: "Password" });
    await user.type(within(pw).getByLabelText("Password"), "short");
    await user.click(within(pw).getByRole("button", { name: "Set password" }));
    expect(within(pw).getByRole("alert")).toHaveTextContent("at least 12 characters");
    await user.clear(within(pw).getByLabelText("Password"));
    await user.type(within(pw).getByLabelText("Password"), "a long passphrase");
    await user.type(within(pw).getByLabelText("Repeat the password"), "a long passphrasE");
    await user.click(within(pw).getByRole("button", { name: "Set password" }));
    expect(within(pw).getByRole("alert")).toHaveTextContent("do not match");
    await user.clear(within(pw).getByLabelText("Repeat the password"));
    await user.type(within(pw).getByLabelText("Repeat the password"), "a long passphrase");
    await user.click(within(pw).getByRole("button", { name: "Set password" }));
    expect(await within(detail).findByText("A password is set.")).toBeInTheDocument();
    expect(env.transport.data.features.remote.secrets.passwords[ada.id]).toBe("a long passphrase");

    // TOTP.
    await user.click(within(detail).getByRole("button", { name: "Set up" }));
    const enroll = await within(detail).findByRole("form", { name: "Set up an authenticator app" });
    expect(
      within(enroll).getByRole("img", { name: "QR code for the authenticator app" }),
    ).toBeInTheDocument();
    const secret = env.transport.data.features.remote.secrets.totp[ada.id]?.secret ?? "";
    expect(within(enroll).getByLabelText("Key for manual entry")).toHaveTextContent(
      secret.match(/.{4}/g)?.join(" ") ?? "",
    );
    const code = within(enroll).getByRole("textbox", { name: "Code from the app" });
    expect(code).toHaveFocus();
    await user.keyboard("111111{Enter}");
    expect(await within(detail).findByText(/That code does not match/)).toBeInTheDocument();
    await user.clear(code);
    await user.keyboard("123456{Enter}");
    expect(await within(detail).findByText("Two-step verification is on.")).toBeInTheDocument();
    expect(within(detail).getByText(/On: signing in with the password/)).toBeInTheDocument();

    // Passkeys: rename by keyboard, then delete with confirmation.
    const keys = within(detail).getByRole("list", { name: "Passkeys" });
    await user.click(within(keys).getByRole("button", { name: "Rename passkey Phone" }));
    const newName = within(keys).getByRole("textbox", { name: "New name" });
    expect(newName).toHaveFocus();
    await user.clear(newName);
    await user.keyboard("Pixel{Enter}");
    expect(await within(keys).findByText("Pixel")).toBeInTheDocument();
    await user.click(within(keys).getByRole("button", { name: "Delete" }));
    await user.click(within(keys).getByRole("button", { name: "Delete passkey" }));
    expect(await within(detail).findByText(/No passkeys/)).toBeInTheDocument();
  });

  it("unlocks a locked user and disables / deletes users", async () => {
    const user = userEvent.setup();
    const env = createEnv();
    wireEvents(env);
    await createAda(env.transport);
    const stored = env.transport.data.features.remote.users[0];
    if (stored) stored.locked_until = new Date(Date.now() + 600_000).toISOString();
    renderNode(widget("remote-users"), env);
    const detail = await screen.findByRole("region", { name: "User ada" });
    expect(within(detail).getByRole("alert")).toHaveTextContent(/locked until/);
    await user.click(within(detail).getByRole("button", { name: "Unlock now" }));
    expect(await within(detail).findByText("The user can sign in again.")).toBeInTheDocument();
    expect(env.transport.data.features.remote.users[0]?.locked_until).toBeNull();

    await user.click(within(detail).getByRole("button", { name: "Disable user" }));
    expect(await within(detail).findByRole("button", { name: "Enable user" })).toBeInTheDocument();
    expect(within(detail).getByRole("button", { name: "Pair a device" })).toBeDisabled();
    await user.click(within(detail).getByRole("button", { name: "Delete user" }));
    await user.click(within(detail).getByRole("button", { name: "Delete user" }));
    expect(await screen.findByText("No remote users yet.")).toBeInTheDocument();
  });

  it("narrows a user's access", async () => {
    const user = userEvent.setup();
    const env = createEnv();
    wireEvents(env);
    await createAda(env.transport);
    renderNode(widget("remote-users"), env);
    const detail = await screen.findByRole("region", { name: "User ada" });
    const access = within(detail).getByRole("combobox", { name: "Access" });
    expect(access).toHaveValue("full");
    await user.selectOptions(access, "no-send");
    await user.click(within(detail).getByRole("checkbox", { name: /Administration/ }));
    await user.click(within(detail).getByRole("button", { name: "Save access" }));
    await waitFor(() =>
      expect(env.transport.data.features.remote.users[0]?.grant).toEqual(presetGrant("no-send")),
    );
  });
});

describe("pairing, desktop side", () => {
  it("shows the code, the address and the fingerprint, and follows the pairing", async () => {
    const user = userEvent.setup();
    const env = createEnv();
    wireEvents(env);
    env.transport.data.features.remote.settings = {
      ...defaultRemoteUiSettings(),
      enabled: true,
      bind_address: "0.0.0.0",
      host_name: "nail-desktop.local",
    };
    const ada = await createAda(env.transport);
    await env.transport.send({
      type: "CreateRemoteUser",
      name: "reader",
      display_name: null,
      grant: presetGrant("read-only"),
    });
    remoteStore.getState().setPairUser(ada.id);
    const close = vi.fn();
    renderWithProviders(<PairDeviceDialog name={PAIR_DIALOG} close={close} />, env);

    const userSelect = await screen.findByRole("combobox", { name: "User" });
    expect(userSelect).toHaveValue(ada.id);
    await user.type(screen.getByRole("textbox", { name: "Name for this device" }), "Tablet");
    await user.selectOptions(screen.getByRole("combobox", { name: "Access" }), "read-only");
    await user.click(screen.getByRole("checkbox", { name: /Administration/ }));
    await user.selectOptions(screen.getByRole("combobox", { name: "Code valid for" }), "5");
    await user.click(screen.getByRole("button", { name: "Create pairing code" }));

    const panel = await screen.findByRole("region", { name: "Pairing code" });
    const request = env.transport.requests.find((r) => r.type === "CreatePairingCode");
    expect(request).toMatchObject({
      device_name: "Tablet",
      grant: presetGrant("read-only", { admin: true }),
      ttl_minutes: 5,
    });
    const shown = within(panel).getByLabelText("Pairing code").textContent ?? "";
    expect(shown).toMatch(/^[A-Z2-9]{4}-[A-Z2-9]{4}$/);
    expect(
      within(panel).getByText(/open https:\/\/nail-desktop.local:4780\/ and enter/),
    ).toBeInTheDocument();
    expect(
      within(panel).getByRole("img", { name: "QR code for https://nail-desktop.local:4780/" }),
    ).toBeInTheDocument();
    const fingerprint = env.transport.data.features.remote.certificate.fingerprint_sha256;
    expect(within(panel).getByText(groupFingerprint(fingerprint))).toBeInTheDocument();
    expect(within(panel).getByRole("status")).toHaveTextContent("Waiting for the new device");

    // A browser enters the code: the dialog notices.
    const browser = new MockAuthServer(env.transport);
    await act(async () => {
      await browser.send({ type: "Pair", code: shown, device_name: "ignored" });
    });
    expect(
      await within(panel).findByText(/Paired. The new device is signed in./),
    ).toBeInTheDocument();
    expect(env.transport.data.features.remote.devices[0]?.name).toBe("Tablet");
    await user.click(within(panel).getByRole("button", { name: "Close" }));
    expect(close).toHaveBeenCalled();
  });

  it("cancels a code and explains when there is nobody to pair for", async () => {
    const user = userEvent.setup();
    const env = createEnv();
    wireEvents(env);
    const { unmount } = renderWithProviders(
      <PairDeviceDialog name={PAIR_DIALOG} close={() => undefined} />,
      env,
    );
    expect(await screen.findByRole("note")).toHaveTextContent("There is no remote user yet.");
    unmount();

    await createAda(env.transport);
    renderWithProviders(<PairDeviceDialog name={PAIR_DIALOG} close={() => undefined} />, env);
    await user.click(await screen.findByRole("button", { name: "Create pairing code" }));
    const panel = await screen.findByRole("region", { name: "Pairing code" });
    expect(within(panel).getByText(/Remote access is not running/)).toBeInTheDocument();
    expect(within(panel).queryByRole("img")).not.toBeInTheDocument();
    await user.click(within(panel).getByRole("button", { name: "Cancel code" }));
    expect(await within(panel).findByText("This code was cancelled.")).toBeInTheDocument();
  });
});
