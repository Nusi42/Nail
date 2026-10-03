import { act, cleanup, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { Event as NailEvent } from "../../api/generated";
import { createEnv, renderNode, widget, type Env } from "../../test/harness";
import { handleRemoteEvent } from "./index";
import { AUDIT_PAGE } from "./queries";
import { remoteStore } from "./state";
import { PASSWORD, createAda, pairedBrowser } from "./testUtil";

function wireEvents(env: Env) {
  return env.client.subscribe((event) =>
    handleRemoteEvent(event, {
      client: env.client,
      queryClient: env.queryClient,
      store: env.store,
    }),
  );
}

afterEach(() => {
  // Unmount before resetting module-level stores.
  cleanup();
  remoteStore.getState().clear();
});

async function twoDevices() {
  const env = createEnv();
  const events: NailEvent[] = [];
  env.transport.onEvent((e) => events.push(e));
  wireEvents(env);
  const ada = await createAda(env.transport);
  const phone = await pairedBrowser(env.transport, ada.id, "Phone", "192.168.1.42");
  const laptop = await pairedBrowser(env.transport, ada.id, "Laptop", "192.168.1.43");
  return { env, events, ada, phone, laptop, user: userEvent.setup() };
}

describe("remote-devices", () => {
  it("lists devices and sessions and signs a session out with the keyboard", async () => {
    const { env, events, phone, laptop, user } = await twoDevices();
    renderNode(widget("remote-devices"), env);
    const devices = await screen.findByRole("list", { name: "Devices" });
    expect(
      within(devices)
        .getAllByRole("listitem")
        .map((li) => li.getAttribute("aria-label")),
    ).toEqual(["Phone", "Laptop"]);
    const sessions = await screen.findByRole("list", { name: "Signed-in sessions" });
    expect(within(sessions).getAllByRole("listitem")).toHaveLength(2);
    expect(within(sessions).getAllByText("Pairing code")).toHaveLength(2);

    const revoke = within(sessions).getByRole("button", { name: "Sign out ada on Phone" });
    revoke.focus();
    await user.keyboard("{Enter}");
    await waitFor(() => expect(within(sessions).getAllByRole("listitem")).toHaveLength(1));
    expect(events).toContainEqual({
      type: "RemoteSessionEnded",
      session_id: phone.session.id,
      reason: "revoked",
    });
    // The phone is signed out; the laptop is not.
    expect((await phone.browser.send({ type: "Status" })).type === "Status").toBe(true);
    expect(phone.browser.session()).toBeNull();
    expect(laptop.browser.session()).not.toBeNull();

    // Ended sessions on demand, with the reason.
    await user.click(screen.getByRole("checkbox", { name: "Also show ended sessions" }));
    expect(await screen.findByText(/signed out from this computer/)).toBeInTheDocument();
  });

  it("revokes a device after confirming, which ends its sessions", async () => {
    const { env, phone, user } = await twoDevices();
    renderNode(widget("remote-devices"), env);
    const devices = await screen.findByRole("list", { name: "Devices" });
    const item = within(devices).getByRole("listitem", { name: "Phone" });
    await user.click(within(item).getByRole("button", { name: "Revoke" }));
    const ask = within(item).getByRole("group");
    expect(ask).toHaveTextContent(/must be paired again/);
    expect(within(ask).getByRole("button", { name: "Revoke device" })).toHaveFocus();
    await user.keyboard("{Enter}");
    expect(await within(item).findByText(/^Revoked /)).toBeInTheDocument();
    expect(phone.browser.session()).toBeNull();
    const status = await phone.browser.send({ type: "Status" });
    expect(status).toMatchObject({ status: { paired: false } });
  });

  it("renames a device and signs everyone out", async () => {
    const { env, laptop, user } = await twoDevices();
    renderNode(widget("remote-devices"), env);
    const devices = await screen.findByRole("list", { name: "Devices" });
    await user.click(within(devices).getByRole("button", { name: "Rename Laptop" }));
    await user.clear(within(devices).getByRole("textbox", { name: "New name" }));
    await user.keyboard("Work laptop{Enter}");
    expect(await within(devices).findByText("Work laptop")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Sign out everyone" }));
    expect(screen.getByRole("group")).toHaveTextContent("Sign out all 2 signed-in sessions?");
    await user.click(
      within(screen.getByRole("group")).getByRole("button", { name: "Sign out everyone" }),
    );
    expect(await screen.findByText("Nobody is signed in.")).toBeInTheDocument();
    expect(laptop.browser.session()).toBeNull();
  });

  it("lists pending pairing codes and cancels them", async () => {
    const env = createEnv();
    wireEvents(env);
    const ada = await createAda(env.transport);
    await env.transport.send({
      type: "CreatePairingCode",
      user_id: ada.id,
      device_name: "Tablet",
      grant: null,
      ttl_minutes: null,
    });
    const user = userEvent.setup();
    renderNode(widget("remote-devices"), env);
    const codes = await screen.findByRole("list", { name: "Codes waiting to be used" });
    expect(within(codes).getByText(/Tablet for Ada Lovelace/)).toBeInTheDocument();
    await user.click(within(codes).getByRole("button", { name: "Cancel code" }));
    await waitFor(() =>
      expect(
        screen.queryByRole("list", { name: "Codes waiting to be used" }),
      ).not.toBeInTheDocument(),
    );
    expect(screen.getByText("No device is paired.")).toBeInTheDocument();
  });
});

describe("remote-audit", () => {
  it("shows sign-ins and failures, filters by user and pages", async () => {
    const env = createEnv();
    wireEvents(env);
    const ada = await createAda(env.transport);
    const { browser } = await pairedBrowser(env.transport, ada.id);
    await browser.send({ type: "Logout" });
    await browser.send({
      type: "Password",
      user_name: "ada",
      password: "wrong",
      totp_code: null,
      purpose: "login",
    });
    await browser.send({
      type: "Password",
      user_name: "ada",
      password: PASSWORD,
      totp_code: null,
      purpose: "login",
    });
    const user = userEvent.setup();
    renderNode(widget("remote-audit"), env);
    const table = await screen.findByRole("table", { name: "Security log" });
    const rows = within(table).getAllByRole("row").slice(1);
    expect(rows.map((r) => r.getAttribute("data-kind"))).toEqual([
      "login",
      "login",
      "logout",
      "login",
      "pairing",
      "pairing-created",
      "password-changed",
    ]);
    const failed = rows[1];
    if (failed === undefined) throw new Error("no row");
    expect(failed).toHaveAttribute("data-outcome", "failure");
    expect(within(failed).getByText("Failed")).toBeInTheDocument();
    expect(within(failed).getByText("Password")).toBeInTheDocument();
    expect(within(failed).getByText("192.168.1.42")).toBeInTheDocument();
    expect(screen.getByText("7 entries, 1 failed or denied")).toBeInTheDocument();

    // New entries appear after an access change.
    await act(async () => {
      await browser.send({ type: "Logout" });
    });
    await waitFor(() => expect(within(table).getAllByRole("row")).toHaveLength(9));

    await user.selectOptions(screen.getByRole("combobox", { name: "User" }), "All users");
    expect(screen.queryByRole("button", { name: "Load older entries" })).not.toBeInTheDocument();
  });

  it("loads older entries page by page", async () => {
    const env = createEnv();
    const s = env.transport.data.features.remote;
    for (let i = 1; i <= AUDIT_PAGE + 5; i++) {
      s.audit.push({
        id: i,
        at: "2026-09-29T10:00:00Z",
        kind: "request",
        outcome: i % 10 === 0 ? "denied" : "success",
        user_id: null,
        user_name: null,
        device_id: null,
        session_id: null,
        ip: null,
        user_agent: null,
        method: null,
        name: `Req${i}`,
        error: i % 10 === 0 ? "forbidden" : null,
        detail: null,
      });
    }
    const user = userEvent.setup();
    renderNode(widget("remote-audit"), env);
    const table = await screen.findByRole("table", { name: "Security log" });
    expect(within(table).getAllByRole("row")).toHaveLength(AUDIT_PAGE + 1);
    expect(within(table).getAllByText("unknown")).toHaveLength(AUDIT_PAGE);
    expect(within(table).getAllByText(/not allowed|permission|Forbidden/i).length).toBeGreaterThan(
      0,
    );
    await user.click(screen.getByRole("button", { name: "Load older entries" }));
    await waitFor(() => expect(within(table).getAllByRole("row")).toHaveLength(AUDIT_PAGE + 6));
    expect(screen.queryByRole("button", { name: "Load older entries" })).not.toBeInTheDocument();
  });
});
