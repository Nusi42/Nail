import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { NodeView } from "../../layout/LayoutRenderer";
import { createEnv, renderWithProviders, widget, type Env } from "../../test/harness";
import { manageSieveIssues, parseAddressList } from "./model";
import { requestsOf } from "../../test/env";

function withSieve(): Env {
  const env = createEnv();
  const work = env.transport.data.accountsConfig.accounts.find((a) => a.id === "acc-work");
  if (!work) throw new Error("no work account");
  work.sieve = { enabled: null, host: null, port: null, tls: null, script_name: null };
  return env;
}

describe("sieve model", () => {
  it("validates ManageSieve settings like the core", () => {
    expect(manageSieveIssues({ host: null, port: null, script_name: null })).toEqual([]);
    expect(manageSieveIssues({ host: "a b", port: 0, script_name: "x/y" })).toEqual([
      "host",
      "port",
      "script-name",
    ]);
    expect(manageSieveIssues({ host: "h", port: 4190, script_name: " " })).toEqual(["script-name"]);
    expect(manageSieveIssues({ host: "h", port: 4190, script_name: "a\u0007" })).toEqual([
      "script-name",
    ]);
    expect(manageSieveIssues({ host: "h", port: 1, script_name: "Nail rules" })).toEqual([]);
    expect(parseAddressList("a@x, b@y;c@z  ")).toEqual(["a@x", "b@y", "c@z"]);
  });
});

describe("sieve-status", () => {
  it("shows the state, uploads, previews the script and where rules run", async () => {
    const user = userEvent.setup();
    const env = withSieve();
    renderWithProviders(<NodeView node={widget("sieve-status")} path="t" />, env);
    const section = await screen.findByRole("region", { name: "Server-side rules" });
    await user.selectOptions(await within(section).findByLabelText("Account"), "acc-work");
    expect(await within(section).findByText("Waiting to upload")).toBeVisible();
    await user.click(within(section).getByRole("button", { name: "Upload now" }));
    expect(await within(section).findByText("Up to date on the server")).toBeVisible();
    expect(within(section).getByText(/fileinto/)).toBeVisible();
    expect(within(section).getByText("roundcube")).toBeVisible();
    await user.click(within(section).getByRole("button", { name: "Show script" }));
    const script = await within(section).findByLabelText("Sieve script");
    expect(script).toHaveTextContent("require");
    const table = within(section).getByRole("table", { name: "Where your rules run" });
    expect(within(table).getAllByRole("row").length).toBeGreaterThan(1);
    expect(within(table).getByText("Server")).toBeVisible();
    expect(requestsOf(env, "SyncSieve")).toEqual([{ type: "SyncSieve", account_id: "acc-work" }]);
  });

  it("turns ManageSieve on for an account and validates the settings", async () => {
    const user = userEvent.setup();
    const env = createEnv();
    renderWithProviders(<NodeView node={widget("sieve-status")} path="t" />, env);
    const box = await screen.findByLabelText("Run rules on the server when possible");
    expect(box).not.toBeChecked();
    await user.click(box);
    await user.type(screen.getByLabelText("Server"), "sieve example");
    await user.click(screen.getByRole("button", { name: "Save" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("without spaces");
    await user.clear(screen.getByLabelText("Server"));
    await user.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => {
      expect(requestsOf(env, "UpdateAccount")).toHaveLength(1);
    });
    expect(requestsOf(env, "UpdateAccount")[0]?.config.sieve).toEqual({
      enabled: true,
      host: null,
      port: null,
      tls: null,
      script_name: null,
    });
  });

  it("shows failures with the core's text", async () => {
    const env = withSieve();
    env.transport.data.features.sieve.failWith = "network";
    renderWithProviders(<NodeView node={widget("sieve-status")} path="t" />, env);
    const section = await screen.findByRole("region", { name: "Server-side rules" });
    const select = await within(section).findByLabelText("Account");
    await userEvent.setup().selectOptions(select, "acc-work");
    expect(await within(section).findByText("Upload failed")).toBeVisible();
    expect(within(section).getAllByRole("alert").length).toBeGreaterThan(0);
  });
});

describe("vacation-editor", () => {
  it("validates, saves and reloads a vacation reply", async () => {
    const user = userEvent.setup();
    const env = withSieve();
    renderWithProviders(<NodeView node={widget("vacation-editor")} path="t" />, env);
    const section = await screen.findByRole("region", { name: "Vacation reply" });
    await user.selectOptions(await within(section).findByLabelText("Account"), "acc-work");
    await user.click(await within(section).findByLabelText("Send a vacation reply"));
    await user.click(within(section).getByRole("button", { name: "Save" }));
    expect(await within(section).findByRole("alert")).toHaveTextContent("Write the reply text.");
    await user.type(within(section).getByLabelText("Message"), "Back on Monday.");
    await user.type(within(section).getByLabelText("Also my addresses"), "me@work.example, x");
    await user.click(within(section).getByRole("button", { name: "Save" }));
    expect(await within(section).findByRole("alert")).toHaveTextContent("not an email address");
    await user.clear(within(section).getByLabelText("Also my addresses"));
    await user.type(within(section).getByLabelText("Also my addresses"), "me@work.example");
    await user.click(within(section).getByRole("button", { name: "Save" }));
    expect(await within(section).findByRole("status")).toHaveTextContent("Saved.");
    expect(requestsOf(env, "UpdateVacation")[0]).toMatchObject({
      account_id: "acc-work",
      settings: { enabled: true, body: "Back on Monday.", addresses: ["me@work.example"] },
    });
  });

  it("asks to turn server rules on when the account has none", async () => {
    const env = createEnv();
    renderWithProviders(<NodeView node={widget("vacation-editor")} path="t" />, env);
    expect(await screen.findByText(/Turn on server-side rules/)).toBeVisible();
  });
});
