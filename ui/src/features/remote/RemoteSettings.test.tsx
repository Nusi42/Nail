import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { Request } from "../../api/generated";
import { createEnv, renderNode, widget, type Env } from "../../test/harness";
import { handleRemoteEvent } from "./index";
import { groupFingerprint } from "./model";

function wireEvents(env: Env) {
  return env.client.subscribe((event) => {
    handleRemoteEvent(event, {
      client: env.client,
      queryClient: env.queryClient,
      store: env.store,
    });
  });
}

function requestsOf<T extends Request["type"]>(env: Env, type: T) {
  return env.transport.requests.filter((r): r is Extract<Request, { type: T }> => r.type === type);
}

async function open() {
  const user = userEvent.setup();
  const env = createEnv();
  wireEvents(env);
  renderNode(widget("remote-settings"), env);
  const form = await screen.findByRole("form", { name: "Remote access" });
  return { user, env, form };
}

describe("remote-settings", () => {
  it("is off on loopback by default and shows internet mode locked with the reason", async () => {
    const { form } = await open();
    expect(screen.getByRole("status")).toHaveTextContent("Not running.");
    expect(within(form).getByRole("checkbox", { name: /Serve Nail/ })).not.toBeChecked();
    expect(within(form).getByRole("combobox", { name: "Listen on" })).toHaveValue("loopback");
    const internet = within(form).getByRole("checkbox", { name: "Allow access from the internet" });
    expect(internet).toBeDisabled();
    expect(internet).not.toBeChecked();
    expect(internet).toHaveAccessibleDescription(
      /Locked: internet access stays off until an external security test/,
    );
    expect(within(form).getByRole("button", { name: "Save" })).toBeDisabled();
  });

  it("serves on the LAN with TLS and shows the addresses and the fingerprint (keyboard only)", async () => {
    const { user, env, form } = await open();
    const enabled = within(form).getByRole("checkbox", { name: /Serve Nail/ });
    enabled.focus();
    await user.keyboard(" ");
    expect(enabled).toBeChecked();
    await user.tab();
    const bind = within(form).getByRole("combobox", { name: "Listen on" });
    expect(bind).toHaveFocus();
    await user.selectOptions(bind, "lan");
    expect(within(form).getByRole("note")).toHaveTextContent(/Windows may ask/);
    // TLS "off" is refused on the LAN before anything is sent.
    await user.selectOptions(
      within(form).getByRole("combobox", { name: "Encryption (TLS)" }),
      "off",
    );
    expect(within(form).getByRole("list", { name: "Problems" })).toHaveTextContent(
      "Encryption is required when other devices can connect.",
    );
    const save = within(form).getByRole("button", { name: "Save" });
    expect(save).toBeDisabled();
    await user.selectOptions(
      within(form).getByRole("combobox", { name: "Encryption (TLS)" }),
      "self-signed",
    );
    const networks = within(form).getByRole("textbox", { name: "Allowed networks" });
    await user.type(networks, "192.168.1.0/24");
    save.focus();
    await user.keyboard("{Enter}");

    expect(await screen.findByText("Remote access settings saved.")).toBeInTheDocument();
    expect(requestsOf(env, "UpdateRemoteUiSettings")[0]?.settings).toMatchObject({
      enabled: true,
      bind_address: "0.0.0.0",
      tls: "self-signed",
      allowed_networks: ["192.168.1.0/24"],
      internet_mode: false,
    });
    const status = screen.getByRole("region", { name: "Status" });
    expect(within(status).getByRole("status")).toHaveTextContent("Running.");
    expect(within(status).getByText("https://192.168.1.20:4780/")).toBeInTheDocument();
    const fingerprint = env.transport.data.features.remote.certificate.fingerprint_sha256;
    expect(within(status).getByText(groupFingerprint(fingerprint))).toBeInTheDocument();
  });

  it("regenerates the certificate after asking", async () => {
    const { user, env } = await open();
    env.transport.data.features.remote.settings.enabled = true;
    const before = env.transport.data.features.remote.certificate.fingerprint_sha256;
    await env.queryClient.invalidateQueries();
    const status = await screen.findByRole("region", { name: "Status" });
    await user.click(await within(status).findByRole("button", { name: "Make a new certificate" }));
    const ask = within(status).getByRole("group");
    expect(ask).toHaveTextContent(/Browsers that trusted the old certificate/);
    // Escape keeps the old one.
    await user.keyboard("{Escape}");
    expect(within(status).queryByRole("group")).not.toBeInTheDocument();
    await user.click(within(status).getByRole("button", { name: "Make a new certificate" }));
    await user.click(
      within(within(status).getByRole("group")).getByRole("button", {
        name: "Make a new certificate",
      }),
    );
    expect(await within(status).findByText("A new certificate is in use.")).toBeInTheDocument();
    const after = env.transport.data.features.remote.certificate.fingerprint_sha256;
    expect(after).not.toBe(before);
    expect(within(status).getByText(groupFingerprint(after))).toBeInTheDocument();
  });

  it("names the problem the core reports and shows unsupported requests", async () => {
    const { user, env, form } = await open();
    await user.clear(within(form).getByRole("spinbutton", { name: "Port" }));
    expect(within(form).getByRole("list", { name: "Problems" })).toHaveTextContent(
      "The port must be a number from 1 to 65535.",
    );
    await user.type(within(form).getByRole("spinbutton", { name: "Port" }), "4790");
    env.transport.data.features.remote.unsupported = ["UpdateRemoteUiSettings"];
    await user.click(within(form).getByRole("button", { name: "Save" }));
    expect(await within(form).findByRole("alert")).toHaveTextContent(/not supported/i);
    await user.click(within(form).getByRole("button", { name: "Undo changes" }));
    expect(within(form).getByRole("spinbutton", { name: "Port" })).toHaveValue(4780);
  });

  it("checks host names and custom addresses as the core does", async () => {
    const { user, form } = await open();
    await user.type(within(form).getByRole("textbox", { name: "Host name" }), "192.168.1.2");
    expect(within(form).getByRole("list", { name: "Problems" })).toHaveTextContent(
      "The host name is not valid",
    );
    await user.clear(within(form).getByRole("textbox", { name: "Host name" }));
    await user.selectOptions(within(form).getByRole("combobox", { name: "Listen on" }), "custom");
    const address = within(form).getByRole("textbox", { name: "Address" });
    await user.type(address, "nail.local");
    expect(address).toHaveAttribute("aria-invalid", "true");
    await user.clear(address);
    await user.type(address, "192.168.1.20");
    await waitFor(() =>
      expect(within(form).queryByRole("list", { name: "Problems" })).not.toBeInTheDocument(),
    );
  });
});
