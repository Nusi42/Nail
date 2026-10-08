import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { DiscoveredAccount } from "../api/generated";
import { securityDemoBySender } from "../features/security/mock";
import { createEnv, renderNode, widget, type Env } from "../test/harness";
import { accountProblems, newIdentity, parseAuthservIds } from "./AccountSettings";
import {
  accountSignIn,
  configFrom,
  manualCandidate,
  preferredCandidate,
  secretsFor,
  withAccountSignIn,
} from "./AccountSetup";
import { defaultPort, protocolChoices, withProtocol, withTls } from "./forms";
import { pruneToMembers, virtualProblems, newVirtualAccount } from "./VirtualAccountEditor";
import { must } from "../test/must";

/** Forwards OAuth results to the UI state, as the app's event handler does. */
function forwardOAuth(env: Env) {
  return env.transport.onEvent((event) => {
    if (event.type === "OAuthFinished") {
      env.store.getState().setOAuthResult(event.flow_id, event.error);
    }
  });
}

describe("account-setup", () => {
  it("adds a password account: discover, confirm, test, save", async () => {
    const user = userEvent.setup();
    const env = createEnv({ state: { windows: ["account-setup"] } });
    const { container } = renderNode(widget("account-setup"), env);
    const setup = screen.getByRole("region", { name: "Add an account" });
    expect(setup).toHaveAttribute("data-step", "address");

    await user.type(screen.getByRole("textbox", { name: "Your name" }), "Ada");
    await user.type(screen.getByRole("textbox", { name: "Email address" }), "ada@example.net");
    await user.click(screen.getByRole("button", { name: "Continue" }));

    await waitFor(() => expect(setup).toHaveAttribute("data-step", "servers"));
    expect(screen.getByRole("radio", { name: /Your provider's configuration/ })).toBeChecked();
    expect(
      screen.getByText("Incoming (IMAP) imap.example.net:993, outgoing smtp.example.net:465"),
    ).toBeInTheDocument();
    const testButton = screen.getByRole("button", { name: "Test connection" });
    expect(testButton).toBeDisabled();
    await user.type(screen.getByLabelText("Password"), "secret");
    await user.click(testButton);

    await waitFor(() => expect(setup).toHaveAttribute("data-step", "test"));
    expect(screen.getByText("Nail can connect to your servers.")).toBeInTheDocument();
    const results = container.querySelectorAll('[data-part="account-setup.result"]');
    expect([...results].map((r) => r.getAttribute("data-state"))).toEqual(["ok", "ok"]);
    const tested = env.transport.requests.find((r) => r.type === "TestAccount");
    expect(tested).toMatchObject({
      config: { email: "ada@example.net", name: "Ada" },
      secrets: [
        { endpoint: "incoming", value: { kind: "password", password: "secret" } },
        { endpoint: "outgoing", value: { kind: "password", password: "secret" } },
      ],
    });

    await user.click(screen.getByRole("button", { name: "Add account" }));
    expect(await screen.findByText(/“Ada” was added/)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Go to the inbox" }));
    expect(env.store.getState().activeAccountId).toBe("example");
    expect(env.store.getState().windows).toEqual([]);
  });

  it("signs in with OAuth when the provider needs it", async () => {
    const user = userEvent.setup();
    const env = createEnv();
    // Keep the sign-in pending long enough to observe the transient state even
    // on a loaded machine; the mock's default 20 ms delay made this flaky.
    env.transport.data.behaviour.workMs = 1500;
    const off = forwardOAuth(env);
    renderNode(widget("account-setup"), env);
    await user.type(screen.getByRole("textbox", { name: "Email address" }), "ada@gmail.com");
    await user.click(screen.getByRole("button", { name: "Continue" }));
    expect(await screen.findByText("Google Mail", {}, { timeout: 5000 })).toBeInTheDocument();
    expect(screen.getByRole("combobox", { name: "Sign-in method" })).toHaveValue("oauth2:google");
    expect(screen.queryByLabelText("Password")).toBeNull();
    await user.click(screen.getByRole("button", { name: "Sign in with Google" }));
    expect(await screen.findByText("Finish signing in in your browser…")).toBeInTheDocument();
    expect(await screen.findByText("Signed in.", {}, { timeout: 5000 })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Test connection" }));
    await screen.findByText("Nail can connect to your servers.", {}, { timeout: 5000 });
    const tested = env.transport.requests.find((r) => r.type === "TestAccount");
    expect(tested?.type === "TestAccount" && tested.secrets[0]?.value.kind).toBe("oauth2");
    off();
  });

  it("reports failed sign-ins, failed tests and lets the user fix settings", async () => {
    const user = userEvent.setup();
    const env = createEnv();
    env.transport.data.behaviour.oauthError = "cancelled";
    const off = forwardOAuth(env);
    renderNode(widget("account-setup"), env);
    await user.type(screen.getByRole("textbox", { name: "Email address" }), "ada@gmail.com");
    await user.click(screen.getByRole("button", { name: "Continue" }));
    await user.click(await screen.findByRole("button", { name: "Sign in with Google" }));
    expect(
      await screen.findByText("Sign-in failed: The operation was cancelled."),
    ).toBeInTheDocument();
    // Switch to a password and a wrong one: the test fails, but saving stays possible.
    fireEvent.change(screen.getByRole("combobox", { name: "Sign-in method" }), {
      target: { value: "password" },
    });
    await user.type(screen.getByLabelText("Password"), "wrong");
    await user.click(screen.getByRole("button", { name: "Test connection" }));
    expect(await screen.findAllByText("The server rejected the login.")).toHaveLength(2);
    expect(screen.getByRole("button", { name: "Add anyway" })).toBeEnabled();
    await user.click(screen.getByRole("button", { name: "Back" }));
    expect(screen.getByLabelText("Password")).toHaveValue("wrong");
    off();
  });

  it("asks for settings by hand when discovery finds nothing", async () => {
    const user = userEvent.setup();
    const env = createEnv();
    renderNode(widget("account-setup"), env);
    await user.type(screen.getByRole("textbox", { name: "Email address" }), "not-an-address");
    await user.click(screen.getByRole("button", { name: "Continue" }));
    expect(screen.getByRole("alert")).toHaveTextContent("Enter a complete email address");
    await user.clear(screen.getByRole("textbox", { name: "Email address" }));
    await user.type(screen.getByRole("textbox", { name: "Email address" }), "ada@home.invalid");
    await user.click(screen.getByRole("button", { name: "Continue" }));
    expect(await screen.findByText(/No settings were found/)).toBeInTheDocument();
    const incoming = screen.getByRole("group", { name: "Incoming mail" });
    const host = within(incoming).getByRole("textbox", { name: "Server" });
    expect(host).toHaveValue("imap.home.invalid");
    // IMAP is the default (ADR 0180): selected, and the first choice offered.
    const protocol = within(incoming).getByRole("combobox", { name: "Protocol" });
    expect(protocol).toHaveValue("imap");
    expect(
      within(protocol)
        .getAllByRole("option")
        .map((o) => o.textContent),
    ).toEqual(["IMAP", "POP3"]);
    await user.clear(host);
    await user.type(host, "unreachable.home.invalid");
    fireEvent.change(within(incoming).getByRole("combobox", { name: "Security" }), {
      target: { value: "starttls" },
    });
    expect(within(incoming).getByRole("spinbutton", { name: "Port" })).toHaveValue(143);
    fireEvent.change(within(incoming).getByRole("combobox", { name: "Protocol" }), {
      target: { value: "pop3" },
    });
    expect(within(incoming).getByRole("spinbutton", { name: "Port" })).toHaveValue(110);
    await user.type(screen.getByLabelText("Password"), "pw");
    await user.click(screen.getByRole("button", { name: "Test connection" }));
    expect(await screen.findByText("The server could not be reached.")).toBeInTheDocument();
  });

  it("never preselects unverified settings and warns about them", async () => {
    const user = userEvent.setup();
    const env = createEnv();
    const forged: DiscoveredAccount = {
      ...manualCandidate("ada@forged.example"),
      source: "autoconfig",
      provider_name: "Forged Mail",
      incoming: {
        ...manualCandidate("ada@forged.example").incoming,
        server: { ...manualCandidate("x@attacker.example").incoming.server },
      },
      warnings: ["insecure-source", "foreign-hosts"],
    };
    const srv: DiscoveredAccount = { ...manualCandidate("ada@forged.example"), source: "srv" };
    // Even when a source ranked it first, the verified proposal is chosen.
    env.transport.data.discovery["forged.example"] = [forged, srv];
    const { container } = renderNode(widget("account-setup"), env);
    await user.type(screen.getByRole("textbox", { name: "Email address" }), "ada@forged.example");
    await user.click(screen.getByRole("button", { name: "Continue" }));
    const trusted = await screen.findByRole("radio", { name: /DNS records of the domain/ });
    expect(trusted).toBeChecked();
    expect(screen.queryByText("Forged Mail")).toBeNull();
    const warnings = container.querySelectorAll('[data-part="account-setup.candidate.warning"]');
    expect([...warnings].map((w) => w.getAttribute("data-warning"))).toEqual([
      "insecure-source",
      "foreign-hosts",
    ]);
    expect(screen.queryByRole("alert")).toBeNull();

    // Choosing the unverified one shows its servers and a warning.
    await user.click(screen.getByRole("radio", { name: /Received without encryption/ }));
    expect(screen.getByRole("alert")).toHaveTextContent("could not be verified");

    // With nothing else found, it is shown for editing, with the warning.
    env.transport.data.discovery["forged.example"] = [forged];
    await user.click(screen.getByRole("button", { name: "Back" }));
    await user.click(screen.getByRole("button", { name: "Continue" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("could not be verified");
    const incoming = screen.getByRole("group", { name: "Incoming mail" });
    expect(within(incoming).getByRole("textbox", { name: "Server" })).toHaveValue(
      "imap.attacker.example",
    );
    expect(preferredCandidate([forged, srv])).toBe(1);
    expect(preferredCandidate([forged])).toBe(0);
    expect(preferredCandidate([])).toBe(0);
  });

  it("builds configurations and secrets", () => {
    const candidate = manualCandidate("ada@x.example");
    const config = configFrom(candidate, "ada@x.example", " ");
    expect(config).toMatchObject({ id: "", name: "ada@x.example", enabled: true });
    expect(config.identities[0]).toMatchObject({ email: "ada@x.example", name: null });
    const oauth = withAccountSignIn(config, { kind: "oauth2", provider: "microsoft" });
    expect(accountSignIn(oauth)).toEqual({ kind: "oauth2", provider: "microsoft" });
    expect(oauth.outgoing?.server.auth).toMatchObject({
      method: "oauth2",
      username: "ada@x.example",
    });
    expect(secretsFor(oauth, "", null)).toEqual([]);
    expect(secretsFor(oauth, "", "f1").map((s) => s.value)).toEqual([
      { kind: "oauth2", flow_id: "f1" },
      { kind: "oauth2", flow_id: "f1" },
    ]);
    expect(secretsFor({ ...config, outgoing: null }, "pw", null)).toHaveLength(1);
    expect(defaultPort("smtp", "starttls")).toBe(587);
    const custom = withTls("imap", { ...candidate.incoming.server, port: 1993 }, "starttls");
    expect(custom.port).toBe(1993);
    const pop = withProtocol(candidate.incoming, "pop3");
    expect(pop).toMatchObject({ protocol: "pop3", server: { port: 995 } });
    expect(withProtocol(pop, "pop3")).toBe(pop);
  });
});

describe("account-settings", () => {
  it("edits an account and its identities", async () => {
    const user = userEvent.setup();
    const env = createEnv();
    renderNode(widget("account-settings"), env);
    const list = await screen.findByRole("list", { name: "Accounts" });
    await user.click(await within(list).findByRole("button", { name: /Work/ }));
    const name = screen.getByRole("textbox", { name: "Account name" });
    expect(name).toHaveValue("Work");
    await user.clear(name);
    await user.type(name, "Office");
    await user.click(screen.getByRole("button", { name: "Add identity" }));
    const addresses = screen.getAllByRole("textbox", { name: "Sender address" });
    expect(addresses).toHaveLength(2);
    expect(screen.getByRole("alert")).toHaveTextContent(
      "Every identity needs a valid email address.",
    );
    expect(screen.getByRole("button", { name: "Save" })).toBeDisabled();
    await user.type(must(addresses[1]), "sales@work.example");
    await user.type(must(screen.getAllByRole("textbox", { name: "Signature" })[1]), "Sales");
    await user.type(screen.getByLabelText("New password"), "new-secret");
    await user.click(screen.getByRole("button", { name: "Save" }));
    expect(await screen.findByText("Settings saved.")).toBeInTheDocument();
    const update = env.transport.requests.find((r) => r.type === "UpdateAccount");
    expect(update).toMatchObject({
      config: { id: "acc-work", name: "Office" },
      secrets: [
        { endpoint: "incoming", value: { kind: "password", password: "new-secret" } },
        { endpoint: "outgoing", value: { kind: "password", password: "new-secret" } },
      ],
    });
    const identities = update?.type === "UpdateAccount" ? update.config.identities : [];
    expect(identities[1]).toMatchObject({
      id: "sales@work.example",
      account_id: "acc-work",
      signature: { text: "Sales" },
    });
    await waitFor(() =>
      expect(within(list).getByRole("button", { name: /Office/ })).toBeInTheDocument(),
    );
  });

  it("removes an account after confirmation and opens setup for new ones", async () => {
    const user = userEvent.setup();
    const env = createEnv();
    renderNode(widget("account-settings"), env);
    await screen.findByRole("textbox", { name: "Account name" });
    await user.click(screen.getByRole("button", { name: "Remove account" }));
    expect(screen.getByText(/Remove “Personal”\?/)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    await user.click(screen.getByRole("button", { name: "Remove account" }));
    await user.click(screen.getByRole("button", { name: "Remove" }));
    await waitFor(() =>
      expect(screen.getByRole("textbox", { name: "Account name" })).toHaveValue("Work"),
    );
    expect(env.transport.data.accounts.map((a) => a.id)).toEqual(["acc-work"]);
    await user.click(screen.getByRole("button", { name: "Add account" }));
    expect(env.store.getState().windows).toEqual(["account-setup"]);
  });

  it("edits the trusted receiving servers, with a suggestion to confirm", async () => {
    const user = userEvent.setup();
    const env = createEnv();
    env.transport.data.features.security.bySender = securityDemoBySender();
    renderNode(widget("account-settings"), env);
    const list = await screen.findByRole("list", { name: "Accounts" });
    await user.click(await within(list).findByRole("button", { name: /Work/ }));
    const field = screen.getByRole("textbox", { name: "Trusted receiving servers (authserv-id)" });
    expect(field).toHaveValue("");
    expect(field).toHaveAccessibleDescription(/only for results from these servers/);
    await user.click(screen.getByRole("button", { name: "Suggest from recent mail" }));
    await waitFor(() => expect(field).toHaveValue("imap.example.org"));
    expect(screen.getByRole("status")).toHaveTextContent(/Compare it with the source/);
    // Nothing is trusted until the user saves.
    expect(env.transport.requests.some((r) => r.type === "UpdateAccount")).toBe(false);
    await user.type(field, ", mx2.work.example");
    await user.click(screen.getByRole("button", { name: "Save" }));
    expect(await screen.findByText("Settings saved.")).toBeInTheDocument();
    const update = env.transport.requests.find((r) => r.type === "UpdateAccount");
    expect(update?.type === "UpdateAccount" && update.config.authserv_ids).toEqual([
      "imap.example.org",
      "mx2.work.example",
    ]);
    // A second suggestion does not add the id twice.
    await user.click(screen.getByRole("button", { name: "Suggest from recent mail" }));
    await waitFor(() =>
      expect(screen.getByRole("status")).toHaveTextContent(/Compare it with the source/),
    );
    expect(field).toHaveValue("imap.example.org, mx2.work.example");
  });

  it("says when recent mail suggests no receiving server", async () => {
    const user = userEvent.setup();
    const env = createEnv();
    renderNode(widget("account-settings"), env);
    await screen.findByRole("textbox", { name: "Account name" });
    await user.click(screen.getByRole("button", { name: "Suggest from recent mail" }));
    expect(
      await screen.findByText("Your recent mail names no receiving server Nail could suggest."),
    ).toBeInTheDocument();
    expect(parseAuthservIds(" a.example,b.example  a.example ")).toEqual([
      "a.example",
      "b.example",
    ]);
  });

  it("shows POP3 options for POP3 accounts", async () => {
    const env = createEnv();
    const personal = must(env.transport.data.accountsConfig.accounts[0]);
    personal.incoming = withProtocol(personal.incoming, "pop3");
    renderNode(widget("account-settings"), env);
    expect(
      await screen.findByRole("checkbox", { name: "Keep messages on the server" }),
    ).toBeChecked();
    expect(screen.queryByLabelText(/Check for new mail every/)).toBeNull();
  });

  it("keeps Gmail API accounts Gmail and edits their polling", async () => {
    // Review M3b: the protocol select showed "IMAP" for a Gmail account and
    // turned it into an IMAP account when touched.
    const user = userEvent.setup();
    const env = createEnv();
    const personal = must(env.transport.data.accountsConfig.accounts[0]);
    personal.incoming = {
      protocol: "gmail",
      server: { ...personal.incoming.server, host: "gmail.googleapis.com", port: 443 },
      options: { poll_interval_secs: null, max_poll_interval_secs: null },
    };
    expect(protocolChoices(personal.incoming).map((c) => c.value)).toEqual(["gmail"]);
    renderNode(widget("account-settings"), env);
    const protocol = await screen.findByRole("combobox", { name: "Protocol" });
    expect(protocol).toHaveValue("gmail");
    expect(
      within(protocol)
        .getAllByRole("option")
        .map((o) => o.textContent),
    ).toEqual(["Gmail API"]);
    expect(screen.queryByLabelText(/Check for new mail every/)).toBeNull();
    await user.type(
      screen.getByRole("spinbutton", { name: "Shortest check interval (seconds)" }),
      "20",
    );
    await user.type(
      screen.getByRole("spinbutton", { name: "Longest check interval (seconds)" }),
      "600",
    );
    await user.click(screen.getByRole("button", { name: "Save" }));
    expect(await screen.findByText("Settings saved.")).toBeInTheDocument();
    const update = env.transport.requests.find((r) => r.type === "UpdateAccount");
    expect(update?.type === "UpdateAccount" && update.config.incoming).toMatchObject({
      protocol: "gmail",
      options: { poll_interval_secs: 20, max_poll_interval_secs: 600 },
    });
    // IMAP / POP3 accounts still switch between each other.
    expect(protocolChoices(withProtocol(personal.incoming, "imap")).map((c) => c.value)).toEqual([
      "imap",
      "pop3",
    ]);
  });

  it("lists problems", () => {
    const env = createEnv();
    const config = structuredClone(must(env.transport.data.accountsConfig.accounts[0]));
    expect(accountProblems(config)).toEqual([]);
    config.email = "x";
    config.incoming.server.host = "";
    config.identities = [];
    expect(accountProblems(config)).toEqual([
      "settings-error-email",
      "settings-error-host",
      "settings-error-no-identity",
    ]);
    expect(newIdentity("a")).toMatchObject({ id: "", account_id: "a" });
  });
});

describe("virtual-account-editor", () => {
  it("creates a virtual account with a folder mapping and a domain rule", async () => {
    const user = userEvent.setup();
    const env = createEnv();
    renderNode(widget("virtual-account-editor"), env);
    expect(await screen.findByText(/No virtual accounts yet/)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "New virtual account" }));
    expect(screen.getByRole("button", { name: "Create" })).toBeDisabled();
    await user.type(screen.getByRole("textbox", { name: "Name" }), "Everything");
    expect(screen.getByRole("checkbox", { name: "Personal" })).toBeChecked();
    expect(screen.getByRole("checkbox", { name: /Merge Inbox, Sent/ })).toBeChecked();

    await user.click(screen.getByRole("button", { name: "Add merged folder" }));
    await user.type(screen.getByRole("textbox", { name: "Folder name" }), "Receipts");
    await user.click(screen.getByRole("button", { name: "Add folder" }));
    const folder = screen.getByRole("combobox", { name: "Folder" });
    await within(folder).findByRole("option", { name: "Inbox/Receipts" });
    fireEvent.change(folder, { target: { value: "Inbox/Receipts" } });

    fireEvent.change(screen.getByRole("combobox", { name: "Identity for new messages" }), {
      target: { value: "ada@work.example" },
    });
    await user.click(screen.getByRole("button", { name: "Add rule" }));
    await user.type(screen.getByRole("textbox", { name: "Recipients" }), "*@client.example");
    fireEvent.change(screen.getByRole("combobox", { name: "Send as" }), {
      target: { value: "ada@work.example" },
    });

    await user.click(screen.getByRole("button", { name: "Create" }));
    expect(await screen.findByText("Virtual account saved.")).toBeInTheDocument();
    expect(env.transport.data.accountsConfig.virtual_accounts).toEqual([
      {
        id: "everything",
        name: "Everything",
        members: ["acc-personal", "acc-work"],
        merge_roles: true,
        folder_mappings: [
          { name: "Receipts", sources: [{ account_id: "acc-personal", path: "Inbox/Receipts" }] },
        ],
        default_identity: "ada@work.example",
        identity_rules: [{ pattern: "*@client.example", identity: "ada@work.example" }],
      },
    ]);
    expect(await screen.findByRole("button", { name: "Everything" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
  });

  it("edits members and deletes a virtual account", async () => {
    const user = userEvent.setup();
    const env = createEnv();
    env.transport.data.accountsConfig.virtual_accounts.push({
      ...newVirtualAccount(["acc-personal", "acc-work"]),
      id: "both",
      name: "Both",
      default_identity: "ada@work.example",
    });
    renderNode(widget("virtual-account-editor"), env);
    await user.click(await screen.findByRole("checkbox", { name: "Work" }));
    await user.click(screen.getByRole("button", { name: "Save" }));
    await screen.findByText("Virtual account saved.");
    expect(env.transport.data.accountsConfig.virtual_accounts[0]).toMatchObject({
      members: ["acc-personal"],
      // The work identity left with the work account.
      default_identity: null,
    });
    await user.click(screen.getByRole("button", { name: "Delete virtual account" }));
    await waitFor(() => {
      expect(env.transport.data.accountsConfig.virtual_accounts).toEqual([]);
    });
  });

  it("validates and prunes", () => {
    const va = newVirtualAccount([]);
    expect(virtualProblems(va)).toEqual(["virtual-error-name", "virtual-error-members"]);
    const identities = [
      { id: "p", account_id: "a" },
      { id: "w", account_id: "b" },
    ] as unknown as Parameters<typeof pruneToMembers>[1];
    const pruned = pruneToMembers(
      {
        ...va,
        name: "X",
        members: ["a"],
        default_identity: "w",
        folder_mappings: [{ name: "M", sources: [{ account_id: "b", path: "x" }] }],
        identity_rules: [{ pattern: "*@x", identity: "w" }],
      },
      identities,
    );
    expect(pruned).toMatchObject({ default_identity: null, identity_rules: [] });
    expect(virtualProblems(pruned)).toEqual(["virtual-error-mapping"]);
  });
});

describe("form helpers", () => {
  it("know the default ports", () => {
    expect(defaultPort("pop3", "implicit")).toBe(995);
    expect(defaultPort("imap", "plaintext")).toBe(143);
  });
});
