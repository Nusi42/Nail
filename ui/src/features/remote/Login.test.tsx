import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { AuthRequest, RemoteSession } from "../../api/generated";
import { createEnv, renderWithProviders } from "../../test/harness";
import type { AuthTransport } from "./authClient";
import { RemoteLogin, RemoteReauth } from "./Login";
import { defaultRemoteUiSettings } from "./mock";
import { MockAuthServer } from "./mockAuth";
import { groupFingerprint } from "./model";
import { PASSWORD, createAda, fakePasskeys, pairedBrowser, pairingCode } from "./testUtil";

function requestTypes(browser: MockAuthServer): AuthRequest["type"][] {
  return browser.requests.map((r) => r.type);
}

async function setup(options: { totp?: boolean; running?: boolean } = {}) {
  const env = createEnv();
  const user = await createAda(env.transport, options);
  if (options.running) {
    env.transport.data.features.remote.settings = {
      ...defaultRemoteUiSettings(),
      enabled: true,
      bind_address: "0.0.0.0",
    };
  }
  const loggedIn = vi.fn<(session: RemoteSession) => void>();
  return { env, user, loggedIn, user$: userEvent.setup() };
}

describe("remote login: pairing a new browser (keyboard only)", () => {
  it("enters the code, compares the fingerprint, adds a passkey and signs in", async () => {
    const { env, user, loggedIn, user$ } = await setup({ running: true });
    const code = await pairingCode(env.transport, user.id);
    const browser = new MockAuthServer(env.transport);
    const passkeys = fakePasskeys({ credentialId: "cred-phone" });
    renderWithProviders(
      <RemoteLogin auth={browser} passkeys={passkeys} onLoggedIn={loggedIn} />,
      env,
    );

    const form = await screen.findByRole("form", { name: "Pair this browser" });
    const fingerprint = env.transport.data.features.remote.certificate.fingerprint_sha256;
    expect(within(form).getByText(groupFingerprint(fingerprint))).toBeInTheDocument();
    expect(within(form).getByText(/If the fingerprints differ, stop/)).toBeInTheDocument();

    // The code field has the focus; everything else is Tab and Enter.
    const codeField = within(form).getByRole("textbox", { name: "Pairing code" });
    expect(codeField).toHaveFocus();
    await user$.keyboard(code.toLowerCase());
    await user$.tab();
    expect(within(form).getByRole("textbox", { name: "Name for this device" })).toHaveFocus();
    await user$.keyboard("My phone{Enter}");

    const offer = await screen.findByRole("form", { name: "Add a passkey" });
    const name = within(offer).getByRole("textbox", { name: "Name for the passkey" });
    expect(name).toHaveFocus();
    expect(name).toHaveValue("My phone");
    await user$.keyboard("{Enter}");
    expect(await screen.findByText(/The passkey “My phone” is ready/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Continue" })).toHaveFocus();
    await user$.keyboard("{Enter}");

    expect(loggedIn).toHaveBeenCalledTimes(1);
    expect(loggedIn.mock.calls[0]?.[0]).toMatchObject({
      auth_method: "pairing",
      device_name: "My phone",
    });
    expect(requestTypes(browser)).toEqual([
      "Status",
      "Pair",
      "PasskeyRegisterStart",
      "PasskeyRegisterFinish",
    ]);
    expect(env.transport.data.features.remote.users[0]?.passkeys.map((p) => p.name)).toEqual([
      "My phone",
    ]);
  });

  it("says when a code is wrong and lets the user skip the passkey", async () => {
    const { env, user, loggedIn, user$ } = await setup();
    const browser = new MockAuthServer(env.transport);
    renderWithProviders(
      <RemoteLogin auth={browser} passkeys={fakePasskeys()} onLoggedIn={loggedIn} />,
      env,
    );
    await screen.findByRole("form", { name: "Pair this browser" });
    expect(screen.getByText(/This connection is not encrypted/)).toBeInTheDocument();
    const submit = screen.getByRole("button", { name: "Pair this browser" });
    expect(submit).toBeDisabled();
    await user$.keyboard("AAAA-BBBB{Enter}");
    expect(await screen.findByRole("alert")).toHaveTextContent("That code is not valid.");

    const code = await pairingCode(env.transport, user.id);
    const field = screen.getByRole("textbox", { name: "Pairing code" });
    await user$.clear(field);
    await user$.type(field, code);
    await user$.click(submit);
    await screen.findByRole("form", { name: "Add a passkey" });
    await user$.click(screen.getByRole("button", { name: "Not now" }));
    expect(loggedIn).toHaveBeenCalledWith(expect.objectContaining({ device_name: "Browser" }));
  });
});

describe("remote login: a paired browser", () => {
  async function paired(options: { totp?: boolean } = {}) {
    const ctx = await setup(options);
    const { browser } = await pairedBrowser(ctx.env.transport, ctx.user.id);
    // A passkey registered from this browser earlier.
    const start = await browser.send({ type: "PasskeyRegisterStart", name: "Phone" });
    if (start.type !== "PasskeyChallenge") throw new Error("no challenge");
    await browser.send({
      type: "PasskeyRegisterFinish",
      challenge_id: start.challenge_id,
      credential: { id: "cred-1" },
    });
    await browser.send({ type: "Logout" });
    browser.requests.length = 0;
    return { ...ctx, browser };
  }

  it("offers the passkey first and signs in with it", async () => {
    const { env, browser, loggedIn, user$ } = await paired();
    const passkeys = fakePasskeys({ credentialId: "cred-1" });
    renderWithProviders(
      <RemoteLogin auth={browser} passkeys={passkeys} onLoggedIn={loggedIn} />,
      env,
    );
    const button = await screen.findByRole("button", { name: "Sign in with a passkey" });
    expect(button).toHaveFocus();
    await user$.keyboard("{Enter}");
    await waitFor(() => expect(loggedIn).toHaveBeenCalledTimes(1));
    expect(loggedIn.mock.calls[0]?.[0]).toMatchObject({ auth_method: "passkey" });
    expect(requestTypes(browser)).toEqual(["Status", "PasskeyStart", "PasskeyFinish"]);
    expect(passkeys.calls[0]).toMatchObject({ userVerification: "required" });
  });

  it("falls back to password and TOTP when the passkey prompt is closed", async () => {
    const { env, browser, loggedIn, user$ } = await paired({ totp: true });
    const closed = Object.assign(new Error("closed"), { name: "NotAllowedError" });
    renderWithProviders(
      <RemoteLogin
        auth={browser}
        passkeys={fakePasskeys({ error: closed })}
        onLoggedIn={loggedIn}
      />,
      env,
    );
    await screen.findByRole("button", { name: "Sign in with a passkey" });
    await user$.keyboard("{Enter}");
    // Closing the prompt is not an error.
    expect(await screen.findByRole("status")).toHaveTextContent("The passkey prompt was closed.");
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();

    await user$.tab();
    expect(screen.getByRole("button", { name: "Use a password instead" })).toHaveFocus();
    await user$.keyboard("{Enter}");
    const form = await screen.findByRole("form", { name: "Sign in with a password" });
    expect(within(form).getByRole("textbox", { name: "User name" })).toHaveFocus();
    await user$.keyboard("ada");
    await user$.tab();
    await user$.keyboard(`${PASSWORD}{Enter}`);

    const totp = await screen.findByRole("form", { name: "Two-step verification" });
    const code = within(totp).getByRole("textbox", { name: "Verification code" });
    expect(code).toHaveFocus();
    await user$.keyboard("000000{Enter}");
    expect(await within(totp).findByRole("alert")).toHaveTextContent("That code is not valid.");
    expect(code).toHaveValue("");
    await user$.type(code, "123456{Enter}");
    await waitFor(() => expect(loggedIn).toHaveBeenCalledTimes(1));
    expect(loggedIn.mock.calls[0]?.[0]).toMatchObject({ auth_method: "password-totp" });
  });

  it("shows the lockout with a countdown and disables signing in", async () => {
    const { env, browser, loggedIn, user$ } = await paired();
    renderWithProviders(
      <RemoteLogin
        auth={browser}
        passkeys={fakePasskeys({ supported: false })}
        onLoggedIn={loggedIn}
      />,
      env,
    );
    expect(await screen.findByRole("note")).toHaveTextContent("This browser cannot use passkeys");
    const name = screen.getByRole("textbox", { name: "User name" });
    await user$.type(name, "ada");
    const password = screen.getByLabelText("Password");
    const signIn = screen.getByRole("button", { name: "Sign in" });
    for (let i = 0; i < 5; i++) {
      await user$.clear(password);
      await user$.type(password, `wrong ${i}`);
      await user$.click(signIn);
      await screen.findByRole("alert");
    }
    expect(screen.getByRole("alert")).toHaveTextContent("Signing in is locked for a while");
    expect(screen.getByText("Too many attempts. Try again in 300 seconds.")).toBeInTheDocument();
    expect(signIn).toBeDisabled();
    expect(loggedIn).not.toHaveBeenCalled();
  });

  it("signs in at once when the browser still has a session", async () => {
    const { env, user, loggedIn } = await setup();
    const { browser } = await pairedBrowser(env.transport, user.id);
    renderWithProviders(
      <RemoteLogin auth={browser} passkeys={fakePasskeys()} onLoggedIn={loggedIn} />,
      env,
    );
    await waitFor(() => expect(loggedIn).toHaveBeenCalledTimes(1));
  });

  it("offers a retry when Nail cannot be reached", async () => {
    const { env, loggedIn, user$ } = await setup();
    let fail = true;
    const inner = new MockAuthServer(env.transport);
    const flaky: AuthTransport = {
      send: (r) => (fail ? Promise.reject(new Error("offline")) : inner.send(r)),
    };
    renderWithProviders(
      <RemoteLogin auth={flaky} passkeys={fakePasskeys()} onLoggedIn={loggedIn} />,
      env,
    );
    expect(await screen.findByRole("alert")).toHaveTextContent("Nail cannot be reached.");
    fail = false;
    await user$.click(screen.getByRole("button", { name: "Try again" }));
    expect(await screen.findByRole("form", { name: "Pair this browser" })).toBeInTheDocument();
  });
});

describe("remote re-authentication", () => {
  it("confirms with the password and reports the new validity", async () => {
    const { env, user, user$ } = await setup();
    const { browser } = await pairedBrowser(env.transport, user.id);
    const done = vi.fn();
    const cancel = vi.fn();
    renderWithProviders(
      <RemoteReauth
        auth={browser}
        passkeys={fakePasskeys({ supported: false })}
        userName="ada"
        passkeyAvailable
        onDone={done}
        onCancel={cancel}
      />,
      env,
    );
    const group = screen.getByRole("group", { name: "Confirm it’s you" });
    expect(within(group).getByRole("textbox", { name: "User name" })).toHaveAttribute("readonly");
    expect(within(group).getByLabelText("Password")).toHaveFocus();
    await user$.keyboard("nope{Enter}");
    expect(await within(group).findByRole("alert")).toHaveTextContent("wrong");
    await user$.clear(within(group).getByLabelText("Password"));
    await user$.keyboard(`${PASSWORD}{Enter}`);
    await waitFor(() => expect(done).toHaveBeenCalledTimes(1));
    expect(browser.requests.at(-1)).toMatchObject({ type: "Password", purpose: "reauth" });
    await user$.keyboard("{Escape}");
    expect(cancel).toHaveBeenCalledTimes(1);
  });
});
